//! The Acadex worker: receives events from the API (Pub/Sub pushes in the full
//! product, direct pushes in the demo) and makes the PDFs; with
//! LOCAL_SCHEDULER=true it also runs the scheduled jobs on a timer.

use std::sync::Arc;

use acadex_cloud::Storage;
use acadex_worker::{WorkerState, documents::Documents, receipt::Renderer, router};
use anyhow::Context;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry = acadex_telemetry::init("acadex-worker")?;

    // WORKER_PORT locally, where the API already has PORT; Cloud Run sets PORT.
    let port: u16 = std::env::var("WORKER_PORT")
        .or_else(|_| std::env::var("PORT"))
        .map(|value| value.parse())
        .unwrap_or(Ok(8081))
        .context("WORKER_PORT must be a port number")?;
    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL is not set — see .env.example")?;
    let receipts_bucket = std::env::var("RECEIPTS_BUCKET")
        .context("RECEIPTS_BUCKET is not set — see .env.example")?;
    let uploads_bucket =
        std::env::var("UPLOADS_BUCKET").context("UPLOADS_BUCKET is not set — see .env.example")?;

    let state = WorkerState {
        pool: acadex_db::connect_lazy(&database_url).context("invalid DATABASE_URL")?,
        storage: Storage::from_env()?,
        receipts_bucket: receipts_bucket.into(),
        uploads_bucket: uploads_bucket.into(),
        // Loading fonts takes a moment; do it once, before taking traffic.
        renderer: Arc::new(Renderer::default()),
        documents: Arc::new(Documents::default()),
        public_url: std::env::var("APP_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_owned())
            .into(),
        // Optional: without SMTP_URL, notices are delivered in-app only.
        mailer: acadex_worker::notify::Mailer::from_env()?,
        pubsub: acadex_cloud::PubSub::from_env().ok(),
        gateway: match (
            std::env::var("PAYMENT_GATEWAY_URL"),
            std::env::var("PAYMENT_GATEWAY_KEY"),
        ) {
            (Ok(url), Ok(key)) if !url.is_empty() => {
                Some(acadex_cloud::gateway::Gateway::new(&url, &key))
            }
            _ => None,
        },
    };

    let args: Vec<String> = std::env::args().collect();

    // `acadex-worker release-notices`: releases scheduled notices (Cloud Scheduler, every minute).
    if args.get(1).map(String::as_str) == Some("release-notices") {
        let done = acadex_worker::release_notices(&state).await?;
        tracing::info!(?done, "notice release job finished");
        return Ok(());
    }

    // The rest of the Cloud Run Jobs that Cloud Scheduler starts (§11.2).
    let job = match args.get(1).map(String::as_str) {
        Some("outbox-sweep") => Some(acadex_worker::sweep_outbox(&state).await?),
        Some("audit-verify") => Some(acadex_worker::verify_audit_chain(&state).await?),
        Some("partition-maintain") => Some(acadex_worker::maintain_partitions(&state).await?),
        Some("attendance-rollup") => Some(acadex_worker::rollup_attendance(&state, None).await?),
        _ => None,
    };
    if let Some(done) = job {
        tracing::info!(job = %args[1], ?done, "job finished");
        return Ok(());
    }

    // `acadex-worker fees-reconcile`: the fees.reconcile job (Cloud Scheduler, every 5 minutes).
    if args.get(1).map(String::as_str) == Some("fees-reconcile") {
        let done = acadex_worker::reconcile_payments(&state).await?;
        tracing::info!(?done, "reconciliation job finished");
        return Ok(());
    }

    // Locally there's no Cloud Scheduler: run the scheduled jobs on a timer
    // here, through the same functions.
    if std::env::var("LOCAL_SCHEDULER").is_ok_and(|v| v == "true") {
        let jobs = state.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(60));
            let mut minute: u64 = 0;
            loop {
                every.tick().await;
                minute += 1;
                if let Err(err) = acadex_worker::sweep_outbox(&jobs).await {
                    tracing::warn!(error = %err, "outbox sweep failed");
                }
                // Hourly on Google Cloud; every 15 minutes here, so it shows.
                if minute % 15 == 1
                    && let Err(err) = acadex_worker::rollup_attendance(&jobs, None).await
                {
                    tracing::warn!(error = %err, "attendance rollup failed");
                }
                // Nightly and weekly on Google Cloud; at start and daily here.
                if minute % 1440 == 1 {
                    if let Err(err) = acadex_worker::verify_audit_chain(&jobs).await {
                        tracing::warn!(error = %err, "audit chain verification failed");
                    }
                    if let Err(err) = acadex_worker::maintain_partitions(&jobs).await {
                        tracing::warn!(error = %err, "partition maintenance failed");
                    }
                }
                // From minute 5, so a fresh `just containers` has seeded first.
                if minute.is_multiple_of(5)
                    && let Err(err) = acadex_worker::reconcile_payments(&jobs).await
                {
                    tracing::warn!(error = %err, "reconciliation job failed");
                }
                if let Err(err) = acadex_worker::release_notices(&jobs).await {
                    tracing::warn!(error = %err, "notice release job failed");
                }
            }
        });
        tracing::info!(
            "local scheduler: notices.release and outbox.sweep every minute; fees.reconcile every 5; attendance.rollup every 15; audit.verify_chain and partition.maintain daily"
        );
    }

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!(port, "acadex-worker listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
