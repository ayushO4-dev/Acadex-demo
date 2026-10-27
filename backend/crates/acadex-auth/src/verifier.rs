//! Verifies access tokens: RS256 only, issuer and audience pinned. The full
//! product fetches Auth0's keys (architecture.md §7.4); the demo trusts one
//! public key generated at setup ([`Verifier::with_pem`]).

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::RwLock;

/// Keys are refetched after this long, so rotated-out keys stop being trusted.
const KEY_MAX_AGE: Duration = Duration::from_secs(600);
/// An unknown `kid` forces a refetch, but at most this often.
const MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token signed with an unknown key")]
    UnknownKey,
    #[error("identity provider unavailable: {0}")]
    Provider(String),
}

pub struct Verifier {
    issuer: String,
    audience: String,
    /// `None` when keys are fixed at construction (tests).
    endpoints: Option<Endpoints>,
    http: reqwest::Client,
    keys: RwLock<KeyCache>,
}

struct Endpoints {
    jwks: String,
    userinfo: String,
}

#[derive(Default)]
struct KeyCache {
    by_kid: HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

impl Verifier {
    pub fn auth0(domain: &str, audience: &str) -> Self {
        Self {
            issuer: format!("https://{domain}/"),
            audience: audience.to_owned(),
            endpoints: Some(Endpoints {
                jwks: format!("https://{domain}/.well-known/jwks.json"),
                userinfo: format!("https://{domain}/userinfo"),
            }),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("static HTTP client configuration"),
            keys: RwLock::default(),
        }
    }

    /// The demo's verifier: trusts one RSA public key (PEM), the one whose
    /// private half the demo website signs persona tokens with. `kid` names it.
    pub fn with_pem(issuer: &str, audience: &str, kid: &str, pem: &str) -> Result<Self, AuthError> {
        let key = DecodingKey::from_rsa_pem(pem.as_bytes()).map_err(invalid)?;
        Ok(Self {
            issuer: issuer.to_owned(),
            audience: audience.to_owned(),
            endpoints: None,
            http: reqwest::Client::new(),
            keys: RwLock::new(KeyCache {
                by_kid: HashMap::from([(kid.to_owned(), key)]),
                fetched_at: Some(Instant::now()),
            }),
        })
    }

    /// A verifier that trusts exactly `jwks` and never calls out. Used in tests.
    pub fn with_keys(issuer: &str, audience: &str, jwks: &JwkSet) -> Result<Self, AuthError> {
        Ok(Self {
            issuer: issuer.to_owned(),
            audience: audience.to_owned(),
            endpoints: None,
            http: reqwest::Client::new(),
            keys: RwLock::new(KeyCache {
                by_kid: decode_keys(jwks)?,
                fetched_at: Some(Instant::now()),
            }),
        })
    }

    pub async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(invalid)?;
        // Pin the algorithm rather than trusting the header (blocks alg-confusion attacks).
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::InvalidToken(format!(
                "{:?} is not accepted",
                header.alg
            )));
        }
        let kid = header
            .kid
            .ok_or_else(|| AuthError::InvalidToken("no key id".into()))?;
        let key = self.key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(invalid)
    }

    /// Email for linking a first-time login to its Acadex account. Called once per
    /// Auth0 user, so an extra round trip is cheaper than an Auth0 Action.
    pub async fn userinfo(&self, token: &str) -> Result<UserInfo, AuthError> {
        // Fixed-key verifiers (tests) have no provider to ask: nothing to link with.
        let Some(endpoints) = &self.endpoints else {
            return Ok(UserInfo {
                email: None,
                email_verified: false,
            });
        };
        self.http
            .get(&endpoints.userinfo)
            .bearer_auth(token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(provider)?
            .json()
            .await
            .map_err(provider)
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, AuthError> {
        {
            let cache = self.keys.read().await;
            let fresh = cache
                .fetched_at
                .is_some_and(|at| at.elapsed() < KEY_MAX_AGE);
            if fresh && let Some(key) = cache.by_kid.get(kid) {
                return Ok(key.clone());
            }
        }

        let mut cache = self.keys.write().await;
        let refetch_due = cache
            .fetched_at
            .is_none_or(|at| at.elapsed() >= MIN_REFETCH_INTERVAL);
        if refetch_due && let Some(endpoints) = &self.endpoints {
            match self.fetch_keys(&endpoints.jwks).await {
                Ok(keys) => {
                    cache.by_kid = keys;
                    cache.fetched_at = Some(Instant::now());
                }
                // Keep serving the cached keys: a short Auth0 outage shouldn't
                // sign everyone out (plan.md risk 7).
                Err(err) => tracing::warn!(error = %err, "JWKS refresh failed"),
            }
        }
        cache.by_kid.get(kid).cloned().ok_or(AuthError::UnknownKey)
    }

    async fn fetch_keys(&self, url: &str) -> Result<HashMap<String, DecodingKey>, AuthError> {
        let jwks: JwkSet = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(provider)?
            .json()
            .await
            .map_err(provider)?;
        decode_keys(&jwks)
    }
}

fn decode_keys(jwks: &JwkSet) -> Result<HashMap<String, DecodingKey>, AuthError> {
    jwks.keys
        .iter()
        .filter_map(|jwk| jwk.common.key_id.clone().map(|kid| (kid, jwk)))
        .map(|(kid, jwk)| Ok((kid, DecodingKey::from_jwk(jwk).map_err(invalid)?)))
        .collect()
}

fn invalid(err: impl std::fmt::Display) -> AuthError {
    AuthError::InvalidToken(err.to_string())
}

fn provider(err: impl std::fmt::Display) -> AuthError {
    AuthError::Provider(err.to_string())
}

#[cfg(test)]
mod tests {
    use crate::testing::{self, AUDIENCE, ISSUER};

    use super::*;

    #[tokio::test]
    async fn accepts_a_valid_token() {
        let claims = testing::verifier()
            .verify(&testing::token("auth0|alice"))
            .await
            .unwrap();
        assert_eq!(claims.sub, "auth0|alice");
    }

    #[tokio::test]
    async fn rejects_another_audience() {
        let token = testing::token_with("auth0|alice", ISSUER, "https://other.api", 300);
        let result = testing::verifier().verify(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[tokio::test]
    async fn rejects_another_issuer() {
        let token = testing::token_with("auth0|alice", "https://evil.example/", AUDIENCE, 300);
        let result = testing::verifier().verify(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[tokio::test]
    async fn rejects_an_expired_token() {
        let token = testing::token_with("auth0|alice", ISSUER, AUDIENCE, -600);
        let result = testing::verifier().verify(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[tokio::test]
    async fn rejects_an_hmac_token() {
        // HS256 signed with the public key as the secret: the classic alg-confusion attack.
        let header = jsonwebtoken::Header {
            kid: Some(testing::KEY_ID.into()),
            ..jsonwebtoken::Header::new(Algorithm::HS256)
        };
        let token = jsonwebtoken::encode(
            &header,
            &testing::claims("auth0|alice", ISSUER, AUDIENCE, 300),
            &jsonwebtoken::EncodingKey::from_secret(testing::JWKS.as_bytes()),
        )
        .unwrap();
        let result = testing::verifier().verify(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[tokio::test]
    async fn rejects_an_unknown_key() {
        let mut jwks: JwkSet = serde_json::from_str(testing::JWKS).unwrap();
        jwks.keys[0].common.key_id = Some("rotated-away".into());
        let verifier = Verifier::with_keys(ISSUER, AUDIENCE, &jwks).unwrap();
        let result = verifier.verify(&testing::token("auth0|alice")).await;
        assert!(matches!(result, Err(AuthError::UnknownKey)));
    }

    #[tokio::test]
    async fn rejects_garbage() {
        let result = testing::verifier().verify("not.a.token").await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }
}
