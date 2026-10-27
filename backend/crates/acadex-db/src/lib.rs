//! PostgreSQL access: connection pool, migrations, per-request RLS transactions,
//! and the queries the API runs.

pub mod academic;
pub mod activities;
pub mod admin;
pub mod attendance;
pub mod audit;
pub mod curriculum;
pub mod electives;
pub mod exams;
pub mod fees;
pub mod fees_office;
pub mod grading;
pub mod identity;
pub mod mooc;
pub mod notices;
pub mod offerings;
pub mod ops;
pub mod org;
pub mod outbox;
pub mod people;
pub mod platform;
pub mod privacy;
pub mod profile;
pub mod promotion;
pub mod results;
pub mod revaluation;
pub mod rls;
pub mod search;
pub mod seeds;
pub mod settings;
pub mod support;
pub mod timetable;

use std::time::Duration;

use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Per-instance pool cap. Cloud Run max instances × this must stay below
/// Cloud SQL `max_connections` (architecture.md rule R6).
const MAX_CONNECTIONS: u32 = 20;

/// Builds the pool without connecting, so the API can start — and report
/// not-ready — while the database is still coming up.
pub fn connect_lazy(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .acquire_timeout(Duration::from_secs(3))
        .connect_lazy(database_url)
}

/// Round-trips to the database; backs the API's `/ready` probe.
#[tracing::instrument(
    name = "db.ping",
    skip_all,
    fields(otel.kind = "client", db.system.name = "postgresql", db.query.text = "SELECT 1")
)]
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
