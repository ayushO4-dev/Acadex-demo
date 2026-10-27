//! The demo's stand-ins for the Google Cloud services the full product uses,
//! with the same interfaces, so the API and worker code is unchanged:
//!
//! - [`PubSub`] delivers an event straight to the worker's push endpoint, in
//!   the envelope Pub/Sub push subscriptions use (`WORKER_PUSH_URL`).
//! - [`Storage`] keeps objects as files under `STORAGE_DIR`, one folder per
//!   bucket, instead of in Cloud Storage.
//!
//! [`gateway`] is the payment gateway's API (the test gateway in the demo).

pub mod gateway;

use std::{path::PathBuf, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("{0} is not set; see .env.example")]
    NotConfigured(&'static str),
    #[error("{service}: {message}")]
    Request {
        service: &'static str,
        message: String,
    },
    #[error("object not found: {0}")]
    NotFound(String),
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("static client configuration")
}

fn failed(service: &'static str, err: impl std::fmt::Display) -> CloudError {
    CloudError::Request {
        service,
        message: err.to_string(),
    }
}

/// Delivers events to the worker as a Pub/Sub push subscription would.
#[derive(Clone)]
pub struct PubSub {
    client: reqwest::Client,
    push_url: String,
}

impl PubSub {
    pub fn new(push_url: impl Into<String>) -> Self {
        Self {
            client: client(),
            push_url: push_url.into(),
        }
    }

    /// `WORKER_PUSH_URL`, e.g. `http://worker:8081/pubsub/push`.
    pub fn from_env() -> Result<Self, CloudError> {
        std::env::var("WORKER_PUSH_URL")
            .ok()
            .filter(|v| !v.is_empty())
            .map(Self::new)
            .ok_or(CloudError::NotConfigured("WORKER_PUSH_URL"))
    }

    /// Delivers an outbox event: the topic is the event name, and the
    /// attributes let the worker route and de-duplicate without parsing the
    /// body. Returns the message id (the outbox row's). A failure leaves the
    /// event in the outbox, and the sweep delivers it again.
    #[tracing::instrument(name = "pubsub.publish", skip(self, payload), fields(otel.kind = "producer"))]
    pub async fn publish_event(
        &self,
        topic: &str,
        payload: &serde_json::Value,
        outbox_id: i64,
    ) -> Result<String, CloudError> {
        let id = outbox_id.to_string();
        let envelope = json!({
            "message": {
                "data": BASE64.encode(payload.to_string()),
                "attributes": { "event": topic, "outbox_id": id },
                "messageId": id,
            },
            "subscription": format!("local/{topic}.worker"),
        });
        let response = self
            .client
            .post(&self.push_url)
            .json(&envelope)
            .send()
            .await
            .map_err(|err| failed("pubsub", err))?;
        if !response.status().is_success() {
            return Err(failed(
                "pubsub",
                format!("the worker answered {} for {topic}", response.status()),
            ));
        }
        Ok(id)
    }
}

/// Objects as files: `STORAGE_DIR/<bucket>/<object name>`.
#[derive(Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `STORAGE_DIR`, or `./storage` when unset.
    pub fn from_env() -> Result<Self, CloudError> {
        Ok(Self::new(
            std::env::var("STORAGE_DIR")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "storage".to_owned()),
        ))
    }

    fn path(&self, bucket: &str, name: &str) -> PathBuf {
        self.root
            .join(encode_object(bucket))
            .join(encode_object(name))
    }

    /// Writes (or overwrites) an object.
    #[tracing::instrument(name = "storage.put", skip(self, bytes), fields(otel.kind = "client", size = bytes.len()))]
    pub async fn put(
        &self,
        bucket: &str,
        name: &str,
        _content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), CloudError> {
        let path = self.path(bucket, name);
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|err| failed("storage", err))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|err| failed("storage", err))
    }

    #[tracing::instrument(name = "storage.get", skip(self), fields(otel.kind = "client"))]
    pub async fn get(&self, bucket: &str, name: &str) -> Result<Vec<u8>, CloudError> {
        match tokio::fs::read(self.path(bucket, name)).await {
            Ok(bytes) => Ok(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(CloudError::NotFound(name.to_owned()))
            }
            Err(err) => Err(failed("storage", err)),
        }
    }
}

/// An object name as one safe file name: everything but letters, digits and
/// `-_.~` is percent-encoded, so `/` and `..` can't leave the bucket's folder.
fn encode_object(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_names_are_one_path_segment() {
        assert_eq!(
            encode_object("receipts/2026-27/RCPT 1.pdf"),
            "receipts%2F2026-27%2FRCPT%201%2Epdf"
        );
        assert_eq!(encode_object(".."), "%2E%2E");
    }
}
