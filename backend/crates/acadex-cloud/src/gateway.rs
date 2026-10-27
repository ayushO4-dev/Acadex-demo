//! The payment gateway's REST API, in Razorpay's shape: orders, the list of
//! payments (for reconciliation) and refunds. Locally the gateway is the
//! web app's test gateway (MOCK_GATEWAY); Razorpay test mode in Phase 4.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone)]
pub struct Gateway {
    client: reqwest::Client,
    base_url: Arc<str>,
    key: Arc<str>,
}

#[derive(Debug, thiserror::Error)]
#[error("payment gateway: {0}")]
pub struct GatewayError(pub String);

fn failed(err: impl std::fmt::Display) -> GatewayError {
    GatewayError(err.to_string())
}

/// A payment as the gateway reports it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GatewayPayment {
    pub id: String,
    pub order_id: Option<String>,
    pub amount: i64,
    pub currency: String,
    /// `captured`, `failed`, `refunded`, …
    pub status: String,
    pub method: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct GatewayRefund {
    pub id: String,
    pub status: String,
}

impl Gateway {
    pub fn new(base_url: &str, key: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("static client configuration"),
            base_url: base_url.trim_end_matches('/').into(),
            key: key.into(),
        }
    }

    /// Creates an order for `amount_paise`, tagged with our reference, and
    /// returns the gateway's order id.
    #[tracing::instrument(name = "gateway.create_order", skip(self), fields(otel.kind = "client"))]
    pub async fn create_order(
        &self,
        amount_paise: i64,
        receipt: &str,
    ) -> Result<String, GatewayError> {
        #[derive(Deserialize)]
        struct Order {
            id: String,
        }
        let response = self
            .client
            .post(format!("{}/v1/orders", self.base_url))
            .basic_auth(&*self.key, Some(""))
            .json(&json!({ "amount": amount_paise, "currency": "INR", "receipt": receipt }))
            .send()
            .await
            .map_err(failed)?;
        if !response.status().is_success() {
            return Err(failed(format!(
                "create order returned {}",
                response.status()
            )));
        }
        Ok(response.json::<Order>().await.map_err(failed)?.id)
    }

    /// Every payment created in `[from, to)` (unix seconds), page by page.
    #[tracing::instrument(name = "gateway.list_payments", skip(self), fields(otel.kind = "client"))]
    pub async fn payments(&self, from: i64, to: i64) -> Result<Vec<GatewayPayment>, GatewayError> {
        #[derive(Deserialize)]
        struct Collection {
            items: Vec<GatewayPayment>,
        }
        const PAGE: usize = 100;
        let mut all = Vec::new();
        loop {
            let url = format!(
                "{}/v1/payments?from={from}&to={to}&count={PAGE}&skip={}",
                self.base_url,
                all.len()
            );
            let response = self
                .client
                .get(url)
                .basic_auth(&*self.key, Some(""))
                .send()
                .await
                .map_err(failed)?;
            if !response.status().is_success() {
                return Err(failed(format!(
                    "list payments returned {}",
                    response.status()
                )));
            }
            let page = response.json::<Collection>().await.map_err(failed)?.items;
            let done = page.len() < PAGE;
            all.extend(page);
            if done {
                return Ok(all);
            }
        }
    }

    /// Refunds `amount_paise` of a payment. `key` makes a retry return the
    /// same refund instead of paying twice (the gateway's idempotency header).
    #[tracing::instrument(name = "gateway.refund", skip(self), fields(otel.kind = "client"))]
    pub async fn refund(
        &self,
        payment_id: &str,
        amount_paise: i64,
        key: &str,
    ) -> Result<GatewayRefund, GatewayError> {
        if !payment_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(failed(format!("unexpected payment id {payment_id:?}")));
        }
        let response = self
            .client
            .post(format!("{}/v1/payments/{payment_id}/refund", self.base_url))
            .basic_auth(&*self.key, Some(""))
            .header("x-refund-idempotency", key)
            .json(&json!({ "amount": amount_paise, "speed": "normal" }))
            .send()
            .await
            .map_err(failed)?;
        if !response.status().is_success() {
            return Err(failed(format!("refund returned {}", response.status())));
        }
        response.json().await.map_err(failed)
    }
}
