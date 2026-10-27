//! Per-request transactions bound to the acting user, so row-level security
//! policies know who is asking (architecture.md §6.4).

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Begins a transaction in which every query runs as `user_id` with `role`.
///
/// `set_config(..., true)` is `SET LOCAL`: it ends with the transaction, so it
/// can't leak into the next request that reuses the pooled connection.
#[tracing::instrument(name = "db.begin_as", skip_all, fields(otel.kind = "client", app.role = role))]
pub async fn begin_as(
    pool: &PgPool,
    user_id: Uuid,
    role: &str,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.user_id', $1, true), set_config('app.role', $2, true)")
        .bind(user_id.to_string())
        .bind(role)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Non-user actors. Each may only be started on its own code path, after its
/// own authentication (the gateway's webhook signature, Pub/Sub's push auth).
#[derive(Clone, Copy, Debug)]
pub enum System {
    /// The payment webhook.
    Gateway,
    /// The background worker.
    Worker,
    /// Publishes outbox events to Pub/Sub.
    Dispatcher,
}

impl System {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
            Self::Worker => "worker",
            Self::Dispatcher => "dispatcher",
        }
    }
}

/// Begins a transaction acting as a system context: no user, so policies
/// written for users stay closed, and only that context's policies apply.
#[tracing::instrument(name = "db.begin_system", skip_all, fields(otel.kind = "client", app.role = system.as_str()))]
pub async fn begin_system(
    pool: &PgPool,
    system: System,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.role', $1, true)")
        .bind(system.as_str())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
