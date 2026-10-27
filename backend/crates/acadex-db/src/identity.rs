use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct ResolvedUser {
    pub user_id: Uuid,
    pub role: String,
    /// Deactivated users resolve, so they can be told so, but get no access.
    pub active: bool,
    /// True on the first sign-in, when the Auth0 subject was just linked.
    pub newly_linked: bool,
}

/// Finds the Acadex user behind an Auth0 subject, linking it by email on first
/// sign-in. See `identity.resolve_subject` for the linking rules.
#[tracing::instrument(name = "db.resolve_subject", skip_all, fields(otel.kind = "client"))]
pub async fn resolve_subject(
    pool: &PgPool,
    subject: &str,
    email: Option<&str>,
    email_verified: bool,
    allow_unverified: bool,
) -> Result<Option<ResolvedUser>, sqlx::Error> {
    sqlx::query_as(
        "SELECT user_id, role, active, newly_linked FROM identity.resolve_subject($1, $2, $3, $4)",
    )
    .bind(subject)
    .bind(email)
    .bind(email_verified)
    .bind(allow_unverified)
    .fetch_optional(pool)
    .await
}
