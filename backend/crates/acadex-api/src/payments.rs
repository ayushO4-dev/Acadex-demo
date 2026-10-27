//! The payment gateway, from the API's side: creating orders, and checking the
//! signature on the webhooks it sends back. Locally the gateway is WireMock
//! plus a mock checkout page; Razorpay test mode replaces both in Phase 4.

use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

#[derive(Clone)]
pub struct Payments {
    pub gateway: Gateway,
    /// Shared with the gateway; signs its webhooks.
    pub webhook_secret: Arc<str>,
    /// The gateway's hosted checkout page.
    pub checkout_url: Arc<str>,
}

impl Payments {
    /// Where to send the browser to pay for an order.
    pub fn checkout_link(&self, order_id: &str, amount_paise: i64, attempt_id: Uuid) -> String {
        format!(
            "{}?order_id={order_id}&amount={amount_paise}&attempt={attempt_id}",
            self.checkout_url
        )
    }
}

pub use acadex_cloud::gateway::{Gateway, GatewayError};

/// Header carrying `t=<unix seconds>,v1=<hex HMAC-SHA256 of "<t>.<body>">`.
pub const SIGNATURE_HEADER: &str = "x-gateway-signature";
/// Older signatures are refused, so a captured request can't be replayed later.
const TOLERANCE_SECS: i64 = 300;

/// The signature the gateway sends for `body` at `timestamp`.
pub fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
    hex::encode(mac(secret, timestamp, body).finalize().into_bytes())
}

fn mac(secret: &str, timestamp: i64, body: &[u8]) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// Checks a webhook's signature header against its raw body, in constant time.
pub fn verify(secret: &str, header: &str, body: &[u8], now: i64) -> Result<(), &'static str> {
    let mut timestamp = None;
    let mut signature = None;
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
            Some(("v1", value)) => signature = hex::decode(value).ok(),
            _ => {}
        }
    }
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return Err("malformed signature header");
    };
    if (now - timestamp).abs() > TOLERANCE_SECS {
        return Err("signature timestamp outside the allowed window");
    }
    mac(secret, timestamp, body)
        .verify_slice(&signature)
        .map_err(|_| "signature does not match")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_000_000;

    #[test]
    fn a_signed_body_verifies() {
        let header = format!("t={NOW},v1={}", sign("s3cret", NOW, b"{}"));
        assert_eq!(verify("s3cret", &header, b"{}", NOW + 10), Ok(()));
    }

    #[test]
    fn tampering_or_the_wrong_secret_fails() {
        let header = format!("t={NOW},v1={}", sign("s3cret", NOW, b"{\"amount\":100}"));
        assert!(verify("s3cret", &header, b"{\"amount\":999}", NOW).is_err());
        assert!(verify("other", &header, b"{\"amount\":100}", NOW).is_err());
        // Changing the timestamp breaks the signature too: it's signed.
        let moved = header.replace(&format!("t={NOW}"), &format!("t={}", NOW + 1));
        assert!(verify("s3cret", &moved, b"{\"amount\":100}", NOW).is_err());
    }

    #[test]
    fn old_signatures_are_refused() {
        let header = format!("t={NOW},v1={}", sign("s3cret", NOW, b"{}"));
        assert_eq!(
            verify("s3cret", &header, b"{}", NOW + 301),
            Err("signature timestamp outside the allowed window")
        );
        assert!(verify("s3cret", "v1=abcd", b"{}", NOW).is_err());
    }
}
