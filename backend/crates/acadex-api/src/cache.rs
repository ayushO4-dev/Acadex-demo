//! Caching for per-user summaries (architecture.md §10): Redis when
//! `REDIS_URL` is set (Memorystore on Google Cloud), otherwise in-process.
//!
//! Keys embed an epoch: bumping it invalidates a whole family of keys in one
//! step, with nothing to scan. Every key also embeds the *viewer*, so a cached
//! response was produced under that same user's row-level security and goes
//! back only to them. The cache is an optimisation, never a dependency: any
//! cache error is logged and treated as a miss.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use acadex_auth::Role;
use acadex_db::settings::Settings;
use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;

/// How long a summary may be stale (architecture.md §2.6, tier 3).
const TTL: Duration = Duration::from_secs(30);
/// A slow cache must not make the request slow: give up and go to the database.
const TIMEOUT: Duration = Duration::from_millis(100);
const PREFIX: &str = "acadex:v1";

#[derive(Clone)]
pub struct Cache {
    backend: Backend,
    stats: Arc<Mutex<HashMap<&'static str, Counts>>>,
    /// Auth0 subject → user, so a request needn't look it up in Postgres.
    /// In-process on purpose: tiny, and per-instance staleness of a minute is
    /// fine for something that changes only when an account is relinked.
    subjects: moka::future::Cache<String, (Uuid, Role)>,
    /// Institution settings: read on most requests, changed rarely.
    settings: moka::future::Cache<(), Settings>,
}

#[derive(Clone)]
enum Backend {
    Redis(ConnectionManager),
    Memory {
        values: moka::future::Cache<String, Arc<[u8]>>,
        epochs: Arc<Mutex<HashMap<String, u64>>>,
    },
}

#[derive(Clone, Copy, Default, Debug)]
struct Counts {
    hits: u64,
    misses: u64,
}

/// Whether a response came from the cache. Also sent as `x-cache`, so a load
/// test can measure the hit ratio from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Hit,
    Miss,
}

impl Cache {
    pub fn in_process() -> Self {
        Self::with(Backend::Memory {
            values: moka::future::Cache::builder()
                .time_to_live(TTL)
                .max_capacity(50_000)
                .build(),
            epochs: Arc::default(),
        })
    }

    pub async fn redis(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self::with(Backend::Redis(manager)))
    }

    fn with(backend: Backend) -> Self {
        Self {
            backend,
            stats: Arc::default(),
            subjects: moka::future::Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(100_000)
                .support_invalidation_closures()
                .build(),
            settings: moka::future::Cache::builder()
                .time_to_live(Duration::from_secs(30))
                .build(),
        }
    }

    /// The Redis connection, when the cache is in Redis (the rate limiter shares it).
    pub fn redis_connection(&self) -> Option<ConnectionManager> {
        match &self.backend {
            Backend::Redis(conn) => Some(conn.clone()),
            Backend::Memory { .. } => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self.backend {
            Backend::Redis(_) => "redis",
            Backend::Memory { .. } => "in-process",
        }
    }

    pub(crate) fn subjects(&self) -> &moka::future::Cache<String, (Uuid, Role)> {
        &self.subjects
    }

    /// The institution's settings, at most 30 s old on other instances.
    pub async fn settings(&self, pool: &PgPool) -> Result<Settings, ApiError> {
        self.settings
            .try_get_with((), async {
                let mut conn = pool.acquire().await?;
                acadex_db::settings::load(&mut conn).await
            })
            .await
            .map_err(|err| {
                tracing::error!(error = %err, "loading settings failed");
                ApiError::internal()
            })
    }

    /// The cached JSON for `key` under the current epoch of `scope`, or
    /// `load`'s result, stored for next time. `name` groups the hit-ratio
    /// statistics ("results", "attendance").
    pub async fn json<T, F, Fut>(
        &self,
        name: &'static str,
        scope: &str,
        key: &str,
        load: F,
    ) -> Result<(Vec<u8>, Lookup), ApiError>
    where
        T: Serialize,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let epoch = self.epoch(scope).await;
        let full_key = format!("{PREFIX}:{name}:{key}:e{epoch}");
        if let Some(bytes) = self.get(&full_key).await {
            self.count(name, Lookup::Hit);
            return Ok((bytes, Lookup::Hit));
        }
        self.count(name, Lookup::Miss);
        let bytes = serde_json::to_vec(&load().await?).map_err(|err| {
            tracing::error!(error = %err, "serialising a response failed");
            ApiError::internal()
        })?;
        self.set(&full_key, &bytes).await;
        Ok((bytes, Lookup::Miss))
    }

    /// Invalidates every key under these scopes. Call after the commit that
    /// changed the data, so a reader can't refill the cache with the old state.
    pub async fn bump(&self, scopes: &[String]) {
        if scopes.is_empty() {
            return;
        }
        match &self.backend {
            Backend::Redis(manager) => {
                let mut pipe = redis::pipe();
                for scope in scopes {
                    pipe.incr(epoch_key(scope), 1).ignore();
                }
                let mut conn = manager.clone();
                let result =
                    tokio::time::timeout(TIMEOUT * 5, pipe.query_async::<()>(&mut conn)).await;
                if !matches!(result, Ok(Ok(()))) {
                    // Entries still expire within the TTL.
                    tracing::warn!(
                        scopes = scopes.len(),
                        "cache invalidation failed; entries expire within 30 s"
                    );
                }
            }
            Backend::Memory { epochs, .. } => {
                let mut epochs = epochs.lock().expect("epoch map");
                for scope in scopes {
                    *epochs.entry(epoch_key(scope)).or_default() += 1;
                }
            }
        }
    }

    async fn epoch(&self, scope: &str) -> u64 {
        match &self.backend {
            Backend::Redis(manager) => {
                let mut conn = manager.clone();
                match tokio::time::timeout(TIMEOUT, conn.get::<_, Option<u64>>(epoch_key(scope)))
                    .await
                {
                    Ok(Ok(epoch)) => epoch.unwrap_or(0),
                    other => {
                        log_failure("epoch", &other);
                        0
                    }
                }
            }
            Backend::Memory { epochs, .. } => epochs
                .lock()
                .expect("epoch map")
                .get(&epoch_key(scope))
                .copied()
                .unwrap_or(0),
        }
    }

    async fn get(&self, key: &str) -> Option<Vec<u8>> {
        match &self.backend {
            Backend::Redis(manager) => {
                let mut conn = manager.clone();
                match tokio::time::timeout(TIMEOUT, conn.get::<_, Option<Vec<u8>>>(key)).await {
                    Ok(Ok(value)) => value,
                    other => {
                        log_failure("get", &other);
                        None
                    }
                }
            }
            Backend::Memory { values, .. } => values.get(key).await.map(|bytes| bytes.to_vec()),
        }
    }

    async fn set(&self, key: &str, bytes: &[u8]) {
        match &self.backend {
            Backend::Redis(manager) => {
                let mut conn = manager.clone();
                let ttl = jittered_ttl(key);
                let result =
                    tokio::time::timeout(TIMEOUT, conn.set_ex::<_, _, ()>(key, bytes, ttl)).await;
                if !matches!(result, Ok(Ok(()))) {
                    log_failure("set", &result);
                }
            }
            Backend::Memory { values, .. } => values.insert(key.to_owned(), bytes.into()).await,
        }
    }

    fn count(&self, name: &'static str, lookup: Lookup) {
        let mut stats = self.stats.lock().expect("cache stats");
        let counts = stats.entry(name).or_default();
        match lookup {
            Lookup::Hit => counts.hits += 1,
            Lookup::Miss => counts.misses += 1,
        }
    }

    /// Logs each cache's hit ratio every 10 s while there is traffic, so it
    /// can be watched during a load test.
    pub fn spawn_reporter(&self) {
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let mut last: HashMap<&'static str, Counts> = HashMap::new();
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let now = stats.lock().expect("cache stats").clone();
                for (name, counts) in &now {
                    let before = last.get(name).copied().unwrap_or_default();
                    let (hits, misses) = (counts.hits - before.hits, counts.misses - before.misses);
                    if hits + misses > 0 {
                        tracing::info!(
                            cache = name,
                            hits,
                            misses,
                            hit_ratio =
                                format!("{:.1}%", hits as f64 * 100.0 / (hits + misses) as f64),
                            "cache hit ratio, last 10 s"
                        );
                    }
                }
                last = now;
            }
        });
    }
}

fn epoch_key(scope: &str) -> String {
    format!("{PREFIX}:epoch:{scope}")
}

/// 27–33 s, varied per key, so entries written together don't all expire
/// together and send a burst of misses to the database at once.
fn jittered_ttl(key: &str) -> u64 {
    let spread = key.bytes().fold(0u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(u64::from(b))
    }) % 7;
    TTL.as_secs() - 3 + spread
}

fn log_failure<T, E: std::fmt::Debug>(op: &str, result: &Result<T, E>) {
    if let Err(err) = result {
        tracing::warn!(op, error = ?err, "cache unavailable; using the database");
    }
}

/// A JSON response from cached (or freshly built) bytes.
pub fn json_response((bytes, lookup): (Vec<u8>, Lookup)) -> Response {
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        "x-cache",
        HeaderValue::from_static(match lookup {
            Lookup::Hit => "hit",
            Lookup::Miss => "miss",
        }),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caches_until_the_epoch_is_bumped() {
        let cache = Cache::in_process();
        let load = |n: u32| move || async move { Ok::<_, ApiError>(n) };

        let (first, lookup) = cache.json("t", "s", "k", load(1)).await.unwrap();
        assert_eq!((first, lookup), (b"1".to_vec(), Lookup::Miss));
        let (second, lookup) = cache.json("t", "s", "k", load(2)).await.unwrap();
        assert_eq!(
            (second, lookup),
            (b"1".to_vec(), Lookup::Hit),
            "served from cache"
        );

        cache.bump(&["s".to_owned()]).await;
        let (third, lookup) = cache.json("t", "s", "k", load(3)).await.unwrap();
        assert_eq!(
            (third, lookup),
            (b"3".to_vec(), Lookup::Miss),
            "bump invalidated it"
        );

        cache.bump(&["other".to_owned()]).await;
        assert_eq!(
            cache.json("t", "s", "k", load(4)).await.unwrap().1,
            Lookup::Hit
        );
    }

    #[tokio::test]
    async fn a_failed_load_is_not_cached() {
        let cache = Cache::in_process();
        let failing = || async { Err::<u32, _>(ApiError::internal()) };
        assert!(cache.json("t", "s", "k", failing).await.is_err());
        let (value, lookup) = cache
            .json("t", "s", "k", || async { Ok::<_, ApiError>(7) })
            .await
            .unwrap();
        assert_eq!((value, lookup), (b"7".to_vec(), Lookup::Miss));
    }

    #[test]
    fn ttl_is_jittered_within_three_seconds() {
        for key in ["a", "b", "results:v1:s2", "attendance:x"] {
            assert!((27..=33).contains(&jittered_ttl(key)), "{key}");
        }
    }
}
