//! Who is on the other end: the caller's address, taken only from where a
//! caller can't write it (architecture.md §13). The rate limiter keys
//! anonymous callers on it, and the audit trail records it.
//!
//! `X-Forwarded-For` is a list each proxy appends to, so everything left of
//! what our own proxies added is whatever the caller sent. Three sources, in
//! order:
//!
//! 1. The web app (BFF) calling for a browser sends that browser's address in
//!    `x-acadex-client-ip`, with the key the two share
//!    (`BFF_FORWARDING_KEY`). Without the right key the header is ignored.
//! 2. Behind `TRUSTED_PROXY_HOPS` proxies that append to `X-Forwarded-For`
//!    (1 on Cloud Run, whose front end appends the address it saw), the entry
//!    the outermost of them added.
//! 3. With none (local), the socket's peer.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::AppState;

/// The browser's address, as the web app forwards it.
pub const CLIENT_IP_HEADER: &str = "x-acadex-client-ip";
/// Proves a forwarded address comes from the web app.
pub const FORWARDING_KEY_HEADER: &str = "x-acadex-forwarding-key";

pub struct Forwarding {
    /// Proxies in front that append to `X-Forwarded-For`.
    hops: usize,
    /// SHA-256 of the key the web app sends; `None` trusts no forwarded address.
    key_digest: Option<[u8; 32]>,
}

impl Forwarding {
    pub fn new(hops: usize, key: Option<&str>) -> Self {
        Self {
            hops,
            key_digest: key
                .filter(|k| !k.is_empty())
                .map(|k| Sha256::digest(k).into()),
        }
    }

    /// `TRUSTED_PROXY_HOPS` (default 0) and `BFF_FORWARDING_KEY`.
    pub fn from_env() -> anyhow::Result<Self> {
        let hops = match std::env::var("TRUSTED_PROXY_HOPS") {
            Ok(v) if !v.is_empty() => v
                .parse()
                .map_err(|_| anyhow::anyhow!("TRUSTED_PROXY_HOPS must be a number, not {v:?}"))?,
            _ => 0,
        };
        Ok(Self::new(
            hops,
            std::env::var("BFF_FORWARDING_KEY").ok().as_deref(),
        ))
    }

    pub fn client_ip(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
        if self.sent_by_web_app(headers) {
            return header(headers, CLIENT_IP_HEADER).and_then(|v| v.trim().parse().ok());
        }
        if self.hops == 0 {
            return peer;
        }
        let chain: Vec<&str> = header(headers, "x-forwarded-for")
            .map(|v| v.split(',').map(str::trim).collect())
            .unwrap_or_default();
        // Fewer entries than proxies: the request didn't come through them.
        let at = chain.len().checked_sub(self.hops)?;
        chain[at].parse().ok()
    }

    /// The key compared as digests, so the comparison's timing says nothing
    /// about the key.
    fn sent_by_web_app(&self, headers: &HeaderMap) -> bool {
        let (Some(expected), Some(sent)) =
            (self.key_digest, header(headers, FORWARDING_KEY_HEADER))
        else {
            return false;
        };
        <[u8; 32]>::from(Sha256::digest(sent)) == expected
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// The caller's address for this request (`None` when it can't be told).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClientAddr(pub Option<IpAddr>);

/// Works out the caller's address once, before the limiter and the handlers.
pub async fn resolve(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let ip = state.forwarding.client_ip(request.headers(), peer);
    request.extensions_mut().insert(ClientAddr(ip));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "k3y-shared-by-web-and-api";

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in pairs {
            h.insert(*name, value.parse().unwrap());
        }
        h
    }

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn behind_cloud_run_a_forged_chain_does_not_move_the_address() {
        let f = Forwarding::new(1, Some(KEY));
        // The caller wrote the first entry; Cloud Run appended the last.
        let spoofed = headers(&[("x-forwarded-for", "1.2.3.4, 198.51.100.9")]);
        assert_eq!(f.client_ip(&spoofed, None), ip("198.51.100.9"));
        let other = headers(&[("x-forwarded-for", "9.9.9.9, 198.51.100.9")]);
        assert_eq!(f.client_ip(&other, None), ip("198.51.100.9"));
    }

    #[test]
    fn the_web_app_forwards_the_browser_with_its_key() {
        let f = Forwarding::new(1, Some(KEY));
        let from_web = headers(&[
            ("x-forwarded-for", "203.0.113.7, 10.8.0.1"),
            (CLIENT_IP_HEADER, "203.0.113.7"),
            (FORWARDING_KEY_HEADER, KEY),
        ]);
        assert_eq!(f.client_ip(&from_web, None), ip("203.0.113.7"));
    }

    #[test]
    fn a_forwarded_address_without_the_key_is_ignored() {
        let f = Forwarding::new(1, Some(KEY));
        let wrong = headers(&[
            ("x-forwarded-for", "198.51.100.9"),
            (CLIENT_IP_HEADER, "1.2.3.4"),
            (FORWARDING_KEY_HEADER, "guess"),
        ]);
        assert_eq!(f.client_ip(&wrong, None), ip("198.51.100.9"));
        let missing = headers(&[
            ("x-forwarded-for", "198.51.100.9"),
            (CLIENT_IP_HEADER, "1.2.3.4"),
        ]);
        assert_eq!(f.client_ip(&missing, None), ip("198.51.100.9"));
        // No key configured: nothing is trusted, even an empty one sent.
        let unset = Forwarding::new(1, Some(""));
        let empty = headers(&[
            ("x-forwarded-for", "198.51.100.9"),
            (CLIENT_IP_HEADER, "1.2.3.4"),
            (FORWARDING_KEY_HEADER, ""),
        ]);
        assert_eq!(unset.client_ip(&empty, None), ip("198.51.100.9"));
    }

    #[test]
    fn without_proxies_the_socket_peer_counts() {
        let f = Forwarding::new(0, Some(KEY));
        let forged = headers(&[("x-forwarded-for", "1.2.3.4")]);
        assert_eq!(f.client_ip(&forged, ip("172.18.0.5")), ip("172.18.0.5"));
    }

    #[test]
    fn a_chain_shorter_than_the_proxies_gives_no_address() {
        let f = Forwarding::new(2, None);
        assert_eq!(
            f.client_ip(&headers(&[("x-forwarded-for", "1.2.3.4")]), None),
            None
        );
        assert_eq!(f.client_ip(&HeaderMap::new(), ip("10.0.0.1")), None);
    }
}
