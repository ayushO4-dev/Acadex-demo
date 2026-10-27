//! Request authentication: Auth0 access token → Acadex user and role.

use std::{convert::Infallible, net::IpAddr};

use acadex_auth::{Permission, Role, rbac};
use acadex_db::{audit, identity, identity::ResolvedUser, rls};
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, client_addr::ClientAddr, error::ApiError, trace};

/// The signed-in user, from the access token in `Authorization: Bearer`.
#[derive(Clone, Copy, Debug)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: Role,
}

impl AuthUser {
    /// `super_admin` is break-glass (architecture.md §7.3): every request it
    /// makes is audited and logged as a warning, which alerting picks up.
    /// A platform operator (the vendor) reaches the platform's own routes and
    /// nothing of the university's, even what every university member may
    /// read. Deny by default, so a new route is closed to them until listed.
    fn confine_platform(&self, parts: &Parts) -> Result<(), ApiError> {
        const PLATFORM_ROUTES: [&str; 5] = [
            "/me",
            "/sessions",
            "/platform/",
            "/admin/operations",
            "/admin/dead-letters/",
        ];
        let path = parts.uri.path();
        if self.role != Role::PlatformAdmin
            || PLATFORM_ROUTES
                .iter()
                .any(|p| path == *p || (p.ends_with('/') && path.starts_with(p)))
        {
            return Ok(());
        }
        Err(ApiError::forbidden(
            "university-only",
            "This belongs to the university, not the platform",
        ))
    }

    async fn audit_break_glass(&self, state: &AppState, parts: &Parts) -> Result<(), ApiError> {
        if self.role != Role::SuperAdmin {
            return Ok(());
        }
        tracing::warn!(user = %self.id, method = %parts.method, path = %parts.uri.path(), "super_admin request");
        let client = ClientInfo::from_parts(parts);
        let mut tx = rls::begin_as(&state.pool, self.id, self.role.as_str()).await?;
        let mut event = client.event("superadmin.request", "request", None);
        event.metadata = json!({ "method": parts.method.as_str(), "path": parts.uri.path() });
        audit::record(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Whether this role may do this (for shaping a response, not guarding it).
    pub fn has(&self, permission: Permission) -> bool {
        rbac::allows(self.role, permission)
    }

    /// RBAC: may this role do this at all? *Which rows* it reaches is left to RLS.
    pub fn require(&self, permission: Permission) -> Result<(), ApiError> {
        if rbac::allows(self.role, permission) {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "not-permitted",
                "Your role can't do this",
            ))
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer_token(&parts.headers)
            .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
        let claims = state.verifier.verify(token).await?;
        if let Some((id, role)) = state.cache.subjects().get(&claims.sub).await {
            let user = Self { id, role };
            user.confine_platform(parts)?;
            user.audit_break_glass(state, parts).await?;
            return Ok(user);
        }

        let resolved =
            match identity::resolve_subject(&state.pool, &claims.sub, None, false, false).await? {
                Some(user) => user,
                None => link_first_login(state, token, &claims.sub).await?,
            };
        if !resolved.active {
            // A deactivated user signing in shows on their log as a refused sign-in.
            if parts.uri.path() == "/sessions" && parts.method == axum::http::Method::POST {
                let client = ClientInfo::from_parts(parts);
                let mut tx = rls::begin_as(&state.pool, resolved.user_id, &resolved.role).await?;
                let mut event = client.event("auth.login", "session", None);
                event.outcome = audit::Outcome::Denied;
                event.metadata = serde_json::json!({ "reason": "account deactivated" });
                audit::record(&mut tx, &event).await?;
                tx.commit().await?;
            }
            return Err(ApiError::forbidden(
                "account-deactivated",
                "This account has been deactivated",
            ));
        }
        let role = resolved.role.parse().map_err(|err: String| {
            tracing::error!(%err, "user has an unrecognised role");
            ApiError::internal()
        })?;
        let user = Self {
            id: resolved.user_id,
            role,
        };

        if resolved.newly_linked {
            let client = ClientInfo::from_parts(parts);
            let mut tx = rls::begin_as(&state.pool, user.id, role.as_str()).await?;
            let event = client.event("identity.linked", "user", Some(user.id.to_string()));
            audit::record(&mut tx, &event).await?;
            tx.commit().await?;
        }
        state
            .cache
            .subjects()
            .insert(claims.sub, (user.id, role))
            .await;
        user.confine_platform(parts)?;
        user.audit_break_glass(state, parts).await?;
        Ok(user)
    }
}

/// First sign-in: ask Auth0 for the email and link it to the matching user.
async fn link_first_login(
    state: &AppState,
    token: &str,
    subject: &str,
) -> Result<ResolvedUser, ApiError> {
    let info = state.verifier.userinfo(token).await?;
    identity::resolve_subject(
        &state.pool,
        subject,
        info.email.as_deref(),
        info.email_verified,
        state.link_unverified_emails,
    )
    .await?
    .ok_or_else(|| {
        ApiError::forbidden("no-account", "This login isn't linked to an Acadex account")
    })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// The person on the other end: their address as `client_addr` worked it out
/// (the browser's, when the web app calls for one), and their user agent.
#[derive(Debug, Default)]
pub struct ClientInfo {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
}

impl ClientInfo {
    fn from_parts(parts: &Parts) -> Self {
        let ip = parts
            .extensions
            .get::<ClientAddr>()
            .and_then(|ClientAddr(ip)| *ip);
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Self { ip, user_agent }
    }

    /// This client as an audit origin, for code shared with background jobs.
    pub fn origin(&self) -> audit::Origin {
        audit::Origin {
            ip: self.ip,
            user_agent: self.user_agent.clone(),
            trace_id: trace::current_trace_id(),
        }
    }

    /// A successful audit event attributed to this client.
    pub fn event<'a>(
        &'a self,
        action: &'a str,
        resource_type: &'a str,
        resource_id: Option<String>,
    ) -> audit::Event<'a> {
        audit::Event {
            action,
            resource_type,
            resource_id,
            outcome: audit::Outcome::Success,
            ip: self.ip,
            user_agent: self.user_agent.as_deref(),
            trace_id: trace::current_trace_id(),
            metadata: json!({}),
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ClientInfo {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(Self::from_parts(parts))
    }
}
