//! The Acadex API server.
//!
//! `acadex-api migrate` applies database migrations and exits, and
//! `acadex-api seed` loads the demo data; Docker Compose runs both first.

use std::sync::Arc;

use acadex_api::{
    AppState, Files,
    cache::Cache,
    config::Config,
    payments::{Gateway, Payments},
    router,
};
use acadex_auth::Verifier;
use acadex_cloud::{PubSub, Storage};
use anyhow::Context;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _telemetry = acadex_telemetry::init("acadex-api")?;

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("migrate") => return migrate().await,
        Some("seed") => return seed().await,
        Some("add-user") => return add_user(&args[2..]).await,
        _ => {}
    }

    let config = Config::from_env()?;
    if config.link_unverified_emails {
        tracing::warn!("AUTH_LINK_UNVERIFIED_EMAILS is on — local development only");
    }
    let cache = match std::env::var("REDIS_URL") {
        Ok(url) if !url.is_empty() => Cache::redis(&url).await.context("connecting to Redis")?,
        _ => Cache::in_process(),
    };
    let limits = Arc::new(acadex_api::limits::Limits::from_env(
        cache.redis_connection(),
    ));
    let state = AppState {
        pool: acadex_db::connect_lazy(&config.database_url).context("invalid DATABASE_URL")?,
        verifier: Arc::new(verifier(&config)?),
        link_unverified_emails: config.link_unverified_emails,
        payments: Payments {
            gateway: Gateway::new(&config.payment_gateway_url, &config.payment_gateway_key),
            webhook_secret: config.payment_webhook_secret.into(),
            checkout_url: config.payment_checkout_url.into(),
        },
        files: Files {
            storage: Storage::from_env()?,
            receipts_bucket: config.receipts_bucket.into(),
            uploads_bucket: config.uploads_bucket.into(),
        },
        outbox_wakeup: Arc::default(),
        notices_live: tokio::sync::broadcast::channel(256).0,
        app_base_url: std::env::var("APP_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_owned())
            .into(),
        cache,
        limits,
        forwarding: Arc::new(acadex_api::client_addr::Forwarding::from_env()?),
    };
    tokio::spawn(listen_for_notices(
        state.pool.clone(),
        state.notices_live.clone(),
    ));
    tracing::info!(cache = state.cache.kind(), "response cache ready");
    state.cache.spawn_reporter();
    acadex_api::dispatch::spawn(
        state.pool.clone(),
        PubSub::from_env()?,
        state.outbox_wakeup.clone(),
    );

    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!(port = config.port, "acadex-api listening");
    // The peer address, for `client_addr` when no proxy is in front.
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Trusts the one key the demo website signs persona tokens with, with the
/// issuer, audience and algorithm pinned, as the full product checks Auth0's.
fn verifier(config: &Config) -> anyhow::Result<Verifier> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let pem = STANDARD
        .decode(config.demo_jwt_public_key.trim())
        .context("DEMO_JWT_PUBLIC_KEY isn't base64: run `just setup`")?;
    let pem = String::from_utf8(pem).context("DEMO_JWT_PUBLIC_KEY isn't a PEM key")?;
    Verifier::with_pem("acadex-demo", "acadex-api", "acadex-demo-1", &pem)
        .context("DEMO_JWT_PUBLIC_KEY isn't an RSA public key")
}

/// Migrations run as the schema owner, not the app role the server uses.
async fn migrate() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_ADMIN_URL")
        .context("DATABASE_ADMIN_URL is not set — see .env.example")?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .context("connecting to the database")?;
    acadex_db::MIGRATOR
        .run(&pool)
        .await
        .context("migration failed")?;
    // The API's login gets the app role's privileges (locally the roles
    // script already did this; on Cloud SQL, Terraform creates the login).
    sqlx::raw_sql(
        "DO $$ BEGIN
             IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'acadex_api') THEN
                 GRANT acadex_app TO acadex_api;
             END IF;
         END $$",
    )
    .execute(&pool)
    .await
    .context("granting the app role to its login")?;
    tracing::info!("migrations applied");
    Ok(())
}

/// `acadex-api add-user <email> <role> <display name…>`: creates a staff
/// account (or changes an existing one's role), for bootstrapping the first
/// administrator before anyone can use the Users page. They link their
/// Auth0 login by email on first sign-in.
async fn add_user(args: &[String]) -> anyhow::Result<()> {
    let [email, role, name @ ..] = args else {
        anyhow::bail!("usage: acadex-api add-user <email> <role> <display name>");
    };
    let role: acadex_auth::Role = role.parse().map_err(|err: String| anyhow::anyhow!(err))?;
    anyhow::ensure!(
        !matches!(
            role,
            acadex_auth::Role::Student | acadex_auth::Role::Faculty | acadex_auth::Role::Hod
        ),
        "students, faculty and HODs need program details: add them on the Users page"
    );
    let name = if name.is_empty() {
        email.clone()
    } else {
        name.join(" ")
    };
    let url = std::env::var("DATABASE_ADMIN_URL")
        .context("DATABASE_ADMIN_URL is not set — see .env.example")?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .context("connecting to the database")?;
    sqlx::query(
        "INSERT INTO identity.users (institution_id, email, display_name, role)
         VALUES ((SELECT min(id) FROM org.institutions), lower($1), $2, $3::text::identity.user_role)
         ON CONFLICT (email) DO UPDATE SET role = EXCLUDED.role, display_name = EXCLUDED.display_name",
    )
    .bind(email)
    .bind(&name)
    .bind(role.as_str())
    .execute(&pool)
    .await
    .context("adding the user")?;
    tracing::info!(%email, role = role.as_str(), "user added");
    Ok(())
}

/// The demo data: every seed file not yet applied.
async fn seed() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_ADMIN_URL")
        .context("DATABASE_ADMIN_URL is not set — see .env.example")?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .context("connecting to the database")?;
    let applied = acadex_db::seeds::apply(&pool)
        .await
        .context("seeding failed")?;
    tracing::info!(?applied, "seed files applied");
    Ok(())
}

/// Cloud Run sends SIGTERM before stopping an instance; let in-flight requests finish.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
        }
    };
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

/// Relays Postgres `NOTIFY acadex_notices` (a notice went live) to this
/// instance's connected browsers. Every instance listens, so it doesn't
/// matter which one a browser is connected to. Reconnects after a failure.
async fn listen_for_notices(pool: sqlx::PgPool, live: tokio::sync::broadcast::Sender<uuid::Uuid>) {
    loop {
        match sqlx::postgres::PgListener::connect_with(&pool).await {
            Ok(mut listener) => {
                if let Err(err) = listener.listen("acadex_notices").await {
                    tracing::warn!(error = %err, "LISTEN acadex_notices failed");
                } else {
                    loop {
                        match listener.recv().await {
                            Ok(note) => {
                                if let Ok(id) = note.payload().parse() {
                                    // No receivers is fine: nobody's connected.
                                    let _ = live.send(id);
                                }
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "notice listener lost its connection");
                                break;
                            }
                        }
                    }
                }
            }
            Err(err) => tracing::warn!(error = %err, "notice listener can't connect"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
