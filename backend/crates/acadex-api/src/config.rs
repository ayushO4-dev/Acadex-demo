use anyhow::{Context, Result};

pub struct Config {
    pub port: u16,
    pub database_url: String,
    /// The demo website's public signing key: PEM, base64-encoded.
    pub demo_jwt_public_key: String,
    pub link_unverified_emails: bool,
    pub payment_gateway_url: String,
    pub payment_gateway_key: String,
    pub payment_checkout_url: String,
    pub payment_webhook_secret: String,
    pub receipts_bucket: String,
    pub uploads_bucket: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let port = match std::env::var("PORT") {
            Ok(value) => value.parse().context("PORT must be a port number")?,
            Err(_) => 8080, // Cloud Run's default
        };
        Ok(Self {
            port,
            database_url: required("DATABASE_URL")?,
            demo_jwt_public_key: required("DEMO_JWT_PUBLIC_KEY")?,
            link_unverified_emails: std::env::var("AUTH_LINK_UNVERIFIED_EMAILS")
                .is_ok_and(|value| value == "true"),
            payment_gateway_url: required("PAYMENT_GATEWAY_URL")?,
            payment_gateway_key: required("PAYMENT_GATEWAY_KEY")?,
            payment_checkout_url: required("PAYMENT_CHECKOUT_URL")?,
            payment_webhook_secret: required("PAYMENT_WEBHOOK_SECRET")?,
            receipts_bucket: required("RECEIPTS_BUCKET")?,
            uploads_bucket: required("UPLOADS_BUCKET")?,
        })
    }
}

fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{name} is not set — see .env.example"))
}
