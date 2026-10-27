//! Mints tokens signed by a test-only RSA key, for tests that exercise the real
//! verification path. The key in `testdata/` exists only for this purpose.

use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::JwkSet};
use serde_json::{Value, json};

use crate::Verifier;

pub const ISSUER: &str = "https://acadex-test.local/";
pub const AUDIENCE: &str = "https://api.acadex.test";
pub const KEY_ID: &str = "acadex-test-key";
pub const JWKS: &str = include_str!("../testdata/test-jwks.json");
const SIGNING_KEY: &str = include_str!("../testdata/test-signing-key.pem");

pub fn verifier() -> Verifier {
    let jwks: JwkSet = serde_json::from_str(JWKS).expect("valid test JWKS");
    Verifier::with_keys(ISSUER, AUDIENCE, &jwks).expect("valid test key")
}

/// A valid token for `sub`, good for five minutes.
pub fn token(sub: &str) -> String {
    token_with(sub, ISSUER, AUDIENCE, 300)
}

/// A token with the given issuer, audience and lifetime (negative = already expired).
pub fn token_with(sub: &str, issuer: &str, audience: &str, ttl_secs: i64) -> String {
    let header = Header {
        kid: Some(KEY_ID.into()),
        ..Header::new(Algorithm::RS256)
    };
    let key = EncodingKey::from_rsa_pem(SIGNING_KEY.as_bytes()).expect("valid test key");
    jsonwebtoken::encode(&header, &claims(sub, issuer, audience, ttl_secs), &key)
        .expect("signable claims")
}

pub fn claims(sub: &str, issuer: &str, audience: &str, ttl_secs: i64) -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
        .cast_signed();
    json!({ "sub": sub, "iss": issuer, "aud": audience, "iat": now, "exp": now + ttl_secs })
}
