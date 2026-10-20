//! Where a request came from (`middleware/RequestOrigin.ts`, MIGRATION_PLAN.md M9, M10).
//!
//! In public mode the metagame listens on loopback behind the gateway, which connects from
//! 127.0.0.1, writes the player's address into `X-Forwarded-For` and adds `X-Dauntless-Gateway:
//! <GATEWAY_SECRET>`. So:
//!
//! - the player's address comes from `X-Forwarded-For` only on a loopback connection with the right
//!   secret; otherwise it is the connection's peer;
//! - a request is *proxied* when it carries any forwarding header; proxied requests may never use
//!   the admin key or the game-server key;
//! - the game-server key also needs a caller on this machine: loopback, a connection to one of this
//!   machine's addresses, or an address in `GAMESERVER_ALLOW_FROM`.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use service_kit::serve::ConnectionInfo;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const GATEWAY_HEADER: &str = "x-dauntless-gateway";

/// Any of these means some proxy relayed the request; the game, its servers and the host scripts
/// never send them.
const PROXY_HEADERS: [&str; 7] = [
    GATEWAY_HEADER,
    "x-forwarded-for",
    "forwarded",
    "x-real-ip",
    "x-forwarded-host",
    "x-forwarded-proto",
    "via",
];

/// An address as Node's `NormalizeAddress` gives it: IPv4-mapped IPv6 unwrapped.
pub fn normalize(address: IpAddr) -> IpAddr {
    address.to_canonical()
}

/// `NormalizeAddress` on text: an IP (IPv4-mapped unwrapped, IPv6 in lower case), or `None`.
pub fn normalize_text(text: &str) -> Option<IpAddr> {
    text.trim().parse::<IpAddr>().ok().map(normalize)
}

/// 127.0.0.0/8 and ::1, also as IPv4-mapped IPv6.
pub fn is_loopback(address: IpAddr) -> bool {
    normalize(address).is_loopback()
}

pub fn is_proxied(headers: &HeaderMap) -> bool {
    PROXY_HEADERS.iter().any(|name| headers.contains_key(*name))
}

fn digest(text: &[u8]) -> [u8; 32] {
    Sha256::digest(text).into()
}

/// The machine's own interface addresses, read again at most every 30 s.
#[derive(Default)]
pub struct OwnAddresses {
    cache: Mutex<Option<(Instant, HashSet<IpAddr>)>>,
}

impl OwnAddresses {
    fn contains(&self, address: IpAddr) -> bool {
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let stale = cache
            .as_ref()
            .is_none_or(|(read_at, _)| read_at.elapsed() > Duration::from_secs(30));
        if stale {
            let addresses = if_addrs::get_if_addrs()
                .map(|list| list.into_iter().map(|interface| normalize(interface.ip())).collect())
                .unwrap_or_default();
            *cache = Some((Instant::now(), addresses));
        }
        cache
            .as_ref()
            .is_some_and(|(_, addresses)| addresses.contains(&address))
    }
}

/// The settings the origin rules depend on.
#[derive(Debug, Clone, Default)]
pub struct OriginConfig {
    /// `GATEWAY_SECRET`: set in public mode.
    pub gateway_secret: Option<String>,
    /// `GAMESERVER_ALLOW_FROM`: more hosts whose game servers may use the key.
    pub gameserver_allow_from: Vec<IpAddr>,
}

/// One request's origin.
pub struct Origin<'a> {
    pub connection: ConnectionInfo,
    pub headers: &'a HeaderMap,
}

impl Origin<'_> {
    pub fn is_proxied(&self) -> bool {
        is_proxied(self.headers)
    }

    /// From loopback and with the right `X-Dauntless-Gateway` secret (compared in constant time).
    pub fn is_trusted_gateway(&self, config: &OriginConfig) -> bool {
        let (Some(secret), Some(presented)) = (&config.gateway_secret, self.headers.get(GATEWAY_HEADER)) else {
            return false;
        };
        is_loopback(self.connection.peer.ip())
            && bool::from(digest(presented.as_bytes()).ct_eq(&digest(secret.as_bytes())))
    }

    /// The right-most `X-Forwarded-For` entry (the one the gateway wrote), all headers joined.
    fn forwarded(&self) -> Option<IpAddr> {
        let joined: Vec<&str> = self
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        let joined = joined.join(",");
        let last = joined.split(',').map(str::trim).rfind(|entry| !entry.is_empty())?;
        normalize_text(last)
    }

    /// The address to log and count the request against: the player's behind the gateway, else
    /// the connection's peer.
    pub fn client_address(&self, config: &OriginConfig) -> IpAddr {
        let peer = normalize(self.connection.peer.ip());
        if !self.is_trusted_gateway(config) {
            return peer;
        }
        self.forwarded().unwrap_or_else(|| {
            tracing::warn!("Gateway request without a usable X-Forwarded-For; using the connection's address");
            peer
        })
    }

    /// A caller on this machine (or in `GAMESERVER_ALLOW_FROM`) that came straight to the metagame.
    pub fn is_direct_local(&self, config: &OriginConfig, own: &OwnAddresses) -> bool {
        if self.is_proxied() {
            return false;
        }
        let peer = normalize(self.connection.peer.ip());
        peer.is_loopback()
            || peer == normalize(self.connection.local.ip())
            || own.contains(peer)
            || config.gameserver_allow_from.contains(&peer)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::http::HeaderValue;

    use super::*;

    fn origin<'a>(peer: &str, local: &str, headers: &'a HeaderMap) -> Origin<'a> {
        Origin {
            connection: ConnectionInfo {
                peer: peer.parse::<SocketAddr>().unwrap(),
                local: local.parse().unwrap(),
            },
            headers,
        }
    }

    fn config() -> OriginConfig {
        OriginConfig {
            gateway_secret: Some("a-long-gateway-secret".into()),
            gameserver_allow_from: vec![],
        }
    }

    #[test]
    fn the_gateway_is_trusted_only_from_loopback_with_its_secret() {
        let mut headers = HeaderMap::new();
        headers.insert(GATEWAY_HEADER, HeaderValue::from_static("a-long-gateway-secret"));
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.9.9.9, 203.0.113.7"));
        let from_gateway = origin("127.0.0.1:50000", "127.0.0.1:61000", &headers);
        assert_eq!(from_gateway.client_address(&config()).to_string(), "203.0.113.7");
        let elsewhere = origin("198.51.100.4:50000", "0.0.0.0:61000", &headers);
        assert_eq!(elsewhere.client_address(&config()).to_string(), "198.51.100.4");
        headers.insert(GATEWAY_HEADER, HeaderValue::from_static("wrong"));
        let wrong = origin("127.0.0.1:50000", "127.0.0.1:61000", &headers);
        assert_eq!(wrong.client_address(&config()).to_string(), "127.0.0.1");
        assert!(wrong.is_proxied());
    }

    #[test]
    fn local_callers_are_loopback_own_addresses_or_allowed_hosts_never_proxied() {
        let none = HeaderMap::new();
        let own = OwnAddresses::default();
        assert!(origin("[::ffff:127.0.0.1]:5", "[::ffff:127.0.0.1]:61000", &none).is_direct_local(&config(), &own));
        assert!(
            origin("100.64.1.2:5", "100.64.1.2:61000", &none).is_direct_local(&config(), &own),
            "own VPN address"
        );
        assert!(!origin("100.64.1.3:5", "100.64.1.2:61000", &none).is_direct_local(&config(), &own));
        let allowed = OriginConfig {
            gameserver_allow_from: vec!["100.64.1.3".parse().unwrap()],
            ..config()
        };
        assert!(origin("100.64.1.3:5", "100.64.1.2:61000", &none).is_direct_local(&allowed, &own));
        let mut via = HeaderMap::new();
        via.insert("via", HeaderValue::from_static("1.1 proxy"));
        assert!(!origin("127.0.0.1:5", "127.0.0.1:61000", &via).is_direct_local(&config(), &own));
    }
}
