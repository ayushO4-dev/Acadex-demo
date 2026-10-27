//! Errors as RFC 9457 Problem Details, carrying the trace id so a support ticket
//! leads straight to the trace (architecture.md §5.4).

use acadex_auth::AuthError;
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::trace;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    /// Stable identifier the frontend switches on.
    kind: &'static str,
    title: &'static str,
    detail: Option<String>,
}

impl ApiError {
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            kind: "unauthenticated",
            title: "Sign-in required",
            detail: Some(detail.into()),
        }
    }

    pub fn forbidden(kind: &'static str, title: &'static str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            kind,
            title,
            detail: None,
        }
    }

    pub fn not_found(what: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            kind: "not-found",
            title: "Not found",
            detail: Some(format!("{what} not found")),
        }
    }

    pub fn is_conflict(&self) -> bool {
        self.status == StatusCode::CONFLICT
    }

    /// The request conflicts with the resource's current state (409).
    pub fn conflict(kind: &'static str, title: &'static str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            kind,
            title,
            detail: None,
        }
    }

    /// A service we depend on failed (502).
    pub fn upstream(kind: &'static str, title: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            kind,
            title,
            detail: None,
        }
    }

    /// The request was understood but breaks a rule (422).
    pub fn invalid(kind: &'static str, title: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            kind,
            title,
            detail: Some(detail.into()),
        }
    }

    /// The title, and the detail when there is one, for reporting inside
    /// another response (one row of a bulk import).
    pub fn message(&self) -> String {
        match &self.detail {
            Some(detail) if !detail.is_empty() => format!("{}: {detail}", self.title),
            _ => self.title.to_owned(),
        }
    }

    pub fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            kind: "internal",
            title: "Something went wrong",
            detail: None,
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "database error");
        Self::internal()
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Provider(reason) => {
                tracing::warn!(%reason, "identity provider unavailable");
                Self {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    kind: "identity-provider-unavailable",
                    title: "Sign-in is temporarily unavailable",
                    detail: None,
                }
            }
            other => Self::unauthorized(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "type": format!("https://acadex.edu/errors/{}", self.kind),
            "title": self.title,
            "status": self.status.as_u16(),
            "detail": self.detail,
            "trace_id": trace::current_trace_id(),
        });
        let mut response = (self.status, Json(body)).into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if self.status == StatusCode::UNAUTHORIZED {
            headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}
