//! Acadex HTTP API: Axum routes over `acadex-db`. In the demo, tokens are
//! signed by the demo website's key instead of Auth0 (see `main.rs`).

mod academic;
mod admin;
mod auth;
pub mod cache;
pub mod client_addr;
pub mod config;
pub mod dispatch;
mod error;
mod fees;
pub mod limits;
mod logs;
mod notices;
mod offerings;
pub mod payments;
mod platform;
mod results;
mod routes;
mod timetable;
mod trace;

use std::sync::Arc;

use acadex_cloud::Storage;

use acadex_auth::Verifier;
use sqlx::PgPool;
use tokio::sync::Notify;

pub use routes::router;

#[derive(Clone)]
pub struct AppState {
    /// Connects as the app role, so row-level security applies to every query.
    pub pool: PgPool,
    pub verifier: Arc<Verifier>,
    /// Local development only: link logins whose email Auth0 hasn't verified.
    pub link_unverified_emails: bool,
    pub payments: payments::Payments,
    pub files: Files,
    pub cache: cache::Cache,
    /// The web app's address, where invitation links return to.
    pub app_base_url: Arc<str>,
    /// Wakes the outbox dispatcher after a commit that queued an event.
    pub outbox_wakeup: Arc<Notify>,
    /// Notices that just went live (from Postgres NOTIFY), for the bell's stream.
    pub notices_live: tokio::sync::broadcast::Sender<uuid::Uuid>,
    /// Load shedding and per-caller rate limits (architecture.md §2.8).
    pub limits: Arc<limits::Limits>,
    /// Where the caller's address may be read from (`client_addr`).
    pub forwarding: Arc<client_addr::Forwarding>,
}

/// Where generated documents live.
#[derive(Clone)]
pub struct Files {
    pub storage: Storage,
    pub receipts_bucket: Arc<str>,
    /// What users upload: certificates, documents, evidence.
    pub uploads_bucket: Arc<str>,
}
