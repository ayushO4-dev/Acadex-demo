//! Load shedding and rate limiting (architecture.md §2.8, §13).
//!
//! Every request belongs to a class. Each class has its own cap on requests
//! in flight in this instance: above it the request is shed at once with
//! `503 Retry-After`, rather than queueing until everything is slow.
//! Exports, PDFs and search have the smallest caps, so they shed first and
//! sign-ins and changes keep working.
//!
//! Separately, each caller (their bearer token, or their IP without one) has
//! a token bucket per kind of request, shared by every instance through
//! Redis: reads, changes and sign-ins refill at different rates. An empty
//! bucket answers `429 Retry-After`. If Redis is slow or down the request
//! is let through: losing the limiter must not take the API down with it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use redis::aio::ConnectionManager;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::{AppState, client_addr::ClientAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Sign-ins and every change: shed last.
    Critical,
    Read,
    /// PDFs and CSV exports: slow, and nobody's blocked if they wait.
    Export,
    Search,
    /// The notice bell's server-sent events: long-lived, counted apart.
    Stream,
}

/// Which class a request is in, from its method and path.
pub fn classify(method: &Method, path: &str) -> Class {
    if path == "/notices/stream" {
        Class::Stream
    } else if method != Method::GET && method != Method::HEAD {
        Class::Critical
    } else if path == "/search" {
        Class::Search
    } else if path.ends_with("/pdf")
        || path.ends_with("/export")
        || path.ends_with("/file")
        || path.contains("/grade-card")
        || path.ends_with(".ics")
    {
        Class::Export
    } else {
        Class::Read
    }
}

/// What a caller's bucket is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Read,
    Mutation,
    Auth,
}

impl Kind {
    fn of(method: &Method, path: &str) -> Self {
        if path == "/sessions" && method == Method::POST {
            Kind::Auth
        } else if method == Method::GET || method == Method::HEAD {
            Kind::Read
        } else {
            Kind::Mutation
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Read => "read",
            Kind::Mutation => "mutation",
            Kind::Auth => "auth",
        }
    }
}

/// Requests per minute per caller (architecture.md §2.8).
#[derive(Clone, Copy, Debug)]
pub struct PerMinute {
    pub read: u32,
    pub mutation: u32,
    pub auth: u32,
}

impl PerMinute {
    pub const DEFAULT: Self = Self {
        read: 100,
        mutation: 20,
        auth: 5,
    };

    fn of(self, kind: Kind) -> u32 {
        match kind {
            Kind::Read => self.read,
            Kind::Mutation => self.mutation,
            Kind::Auth => self.auth,
        }
    }
}

pub struct Limits {
    critical: Arc<Semaphore>,
    read: Arc<Semaphore>,
    export: Arc<Semaphore>,
    search: Arc<Semaphore>,
    stream: Arc<Semaphore>,
    /// `None`: rate limiting off (load tests, most API tests).
    rate: Option<(PerMinute, Buckets)>,
}

enum Buckets {
    Redis(ConnectionManager),
    Memory(Mutex<HashMap<String, (f64, Instant)>>),
}

impl Limits {
    /// In-flight caps per instance, sized for one Cloud Run instance
    /// (1 vCPU, 512 MiB, a 20-connection pool).
    pub fn new(rate: Option<PerMinute>, redis: Option<ConnectionManager>) -> Self {
        Self {
            critical: Arc::new(Semaphore::new(256)),
            read: Arc::new(Semaphore::new(512)),
            export: Arc::new(Semaphore::new(16)),
            search: Arc::new(Semaphore::new(32)),
            stream: Arc::new(Semaphore::new(2000)),
            rate: rate.map(|per_minute| {
                let buckets = match redis {
                    Some(conn) => Buckets::Redis(conn),
                    None => Buckets::Memory(Mutex::default()),
                };
                (per_minute, buckets)
            }),
        }
    }

    /// `RATE_LIMITS=off` turns rate limiting off (for load tests); the
    /// in-flight caps always apply.
    pub fn from_env(redis: Option<ConnectionManager>) -> Self {
        let off = std::env::var("RATE_LIMITS").is_ok_and(|v| v == "off");
        Self::new((!off).then_some(PerMinute::DEFAULT), redis)
    }

    fn semaphore(&self, class: Class) -> &Arc<Semaphore> {
        match class {
            Class::Critical => &self.critical,
            Class::Read => &self.read,
            Class::Export => &self.export,
            Class::Search => &self.search,
            Class::Stream => &self.stream,
        }
    }

    /// `Ok` to go ahead, or `Err(seconds)` to wait before trying again.
    async fn take(&self, caller: &str, kind: Kind) -> Result<(), u64> {
        let Some((per_minute, buckets)) = &self.rate else {
            return Ok(());
        };
        let capacity = per_minute.of(kind);
        let key = format!("acadex:v1:rl:{}:{caller}", kind.name());
        match buckets {
            Buckets::Redis(conn) => {
                let script = redis::Script::new(TOKEN_BUCKET);
                let mut conn = conn.clone();
                let mut invocation = script.key(&key);
                invocation.arg(capacity).arg(60_000);
                let call = invocation.invoke_async::<(i64, i64)>(&mut conn);
                match tokio::time::timeout(Duration::from_millis(100), call).await {
                    Ok(Ok((1, _))) => Ok(()),
                    Ok(Ok((_, wait_ms))) => Err((wait_ms as u64).div_ceil(1000).max(1)),
                    Ok(Err(err)) => {
                        tracing::warn!(error = %err, "rate limiter unavailable; letting the request through");
                        Ok(())
                    }
                    Err(_) => Ok(()),
                }
            }
            Buckets::Memory(map) => {
                let mut map = map.lock().expect("not poisoned");
                if map.len() > 100_000 {
                    map.clear();
                }
                let now = Instant::now();
                let rate = f64::from(capacity) / 60.0; // tokens per second
                let (tokens, last) = map.entry(key).or_insert((f64::from(capacity), now));
                *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * rate)
                    .min(f64::from(capacity));
                *last = now;
                if *tokens >= 1.0 {
                    *tokens -= 1.0;
                    Ok(())
                } else {
                    Err(((1.0 - *tokens) / rate).ceil() as u64)
                }
            }
        }
    }
}

/// A token bucket in one round trip, on Redis's own clock so every instance
/// agrees. KEYS[1] the bucket; ARGV capacity, and the milliseconds it takes
/// to refill completely. Returns {1, 0} to go ahead or {0, ms to wait}.
const TOKEN_BUCKET: &str = r"
local capacity = tonumber(ARGV[1])
local per_ms = capacity / tonumber(ARGV[2])
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local state = redis.call('HMGET', KEYS[1], 'tokens', 'at')
local tokens = tonumber(state[1]) or capacity
local at = tonumber(state[2]) or now
tokens = math.min(capacity, tokens + (now - at) * per_ms)
local allowed, wait = 0, 0
if tokens >= 1 then
  tokens = tokens - 1
  allowed = 1
else
  wait = math.ceil((1 - tokens) / per_ms)
end
redis.call('HSET', KEYS[1], 'tokens', tostring(tokens), 'at', now)
redis.call('PEXPIRE', KEYS[1], tonumber(ARGV[2]) + 1000)
return {allowed, wait}
";

/// Who a request counts against: the user its bearer token names (read
/// from the token without checking the signature, which the auth layer does
/// next; a forged subject only earns a 401), or without one the caller's
/// address, as `client_addr` worked it out (one the caller can't forge).
/// Per user, not per token, so refreshing a token doesn't refill the bucket.
fn caller(request: &Request) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let headers = request.headers();
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        let subject = token
            .split('.')
            .nth(1)
            .and_then(|claims| URL_SAFE_NO_PAD.decode(claims).ok())
            .and_then(|claims| serde_json::from_slice::<serde_json::Value>(&claims).ok())
            .and_then(|claims| claims["sub"].as_str().map(str::to_owned));
        let who = subject.unwrap_or_else(|| token.to_owned());
        return hex::encode(&Sha256::digest(who.as_bytes())[..12]);
    }
    match request.extensions().get::<ClientAddr>() {
        Some(ClientAddr(Some(ip))) => format!("ip:{ip}"),
        _ => "ip:unknown".to_owned(),
    }
}

fn refuse(status: StatusCode, retry_after: u64, kind: &str, title: &str) -> Response {
    let body = json!({
        "type": format!("https://acadex.example/problems/{kind}"),
        "title": title,
        "status": status.as_u16(),
    });
    let mut response = (status, axum::Json(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
        headers.insert(header::RETRY_AFTER, value);
    }
    response
}

/// The middleware: shed above the class's cap, then limit the caller's rate.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if path == "/health" || path == "/ready" {
        return next.run(request).await;
    }
    let method = request.method().clone();
    let class = classify(&method, &path);
    let Ok(_permit) = state.limits.semaphore(class).clone().try_acquire_owned() else {
        tracing::warn!(?class, %path, "shedding load");
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            2,
            "overloaded",
            "Acadex is busy. Please try again in a moment.",
        );
    };
    // The gateway's webhook is signed and has its own replay protection.
    if !path.starts_with("/webhooks/") && class != Class::Stream {
        let kind = Kind::of(&method, &path);
        if let Err(wait) = state.limits.take(&caller(&request), kind).await {
            tracing::info!(kind = kind.name(), %path, wait, "rate limited");
            return refuse(
                StatusCode::TOO_MANY_REQUESTS,
                wait,
                "rate-limited",
                "Too many requests. Please slow down and try again shortly.",
            );
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_classified_by_cost() {
        assert_eq!(classify(&Method::POST, "/sessions"), Class::Critical);
        assert_eq!(
            classify(&Method::PUT, "/offerings/x/attendance"),
            Class::Critical
        );
        assert_eq!(classify(&Method::GET, "/me"), Class::Read);
        assert_eq!(classify(&Method::GET, "/search"), Class::Search);
        assert_eq!(
            classify(&Method::GET, "/fees/receipts/x/pdf"),
            Class::Export
        );
        assert_eq!(classify(&Method::GET, "/audit/logs/export"), Class::Export);
        assert_eq!(classify(&Method::GET, "/notices/stream"), Class::Stream);
    }

    #[tokio::test]
    async fn a_bucket_empties_and_refills() {
        let limits = Limits::new(
            Some(PerMinute {
                read: 3,
                mutation: 1,
                auth: 1,
            }),
            None,
        );
        for _ in 0..3 {
            assert_eq!(limits.take("a", Kind::Read).await, Ok(()));
        }
        let Err(wait) = limits.take("a", Kind::Read).await else {
            panic!("should be empty")
        };
        assert!((1..=20).contains(&wait), "{wait}");
        // Another caller, and another kind, have buckets of their own.
        assert_eq!(limits.take("b", Kind::Read).await, Ok(()));
        assert_eq!(limits.take("a", Kind::Mutation).await, Ok(()));
    }
}
