//! Trusted reverse-proxy configuration for forensic source-IP resolution
//! (observability.md §Audit Event Schema, task-102 F3 / CWE-348).
//!
//! `X-Forwarded-For`, `X-Real-Ip` and `Forwarded` headers are
//! caller-controlled: in the default deployment (no reverse proxy) any
//! client can forge them, and a forged value persisted as an audit
//! event's `source_ip` defeats the forensic attribution the spec added
//! the field for. The standard pattern — implemented here — is:
//!
//! - the source of truth is the **socket peer address**
//!   (`ConnectInfo<SocketAddr>`), which the transport guarantees;
//! - forwarded headers are honored **only** when the immediate peer is a
//!   configured trusted proxy (`GYRE_TRUSTED_PROXIES`, a comma-separated
//!   list of CIDRs or bare IPs; empty by default = always use the peer
//!   address);
//! - when the peer is a trusted proxy, `X-Forwarded-For` is read
//!   rightmost-untrusted (the rightmost entry that is not itself a
//!   trusted proxy is the client the chain started from), falling back
//!   to `X-Real-Ip`, then to the peer itself;
//! - with no socket information at all the resolution fails closed to
//!   `None` — headers are never trusted on their own.

use axum::http::HeaderMap;
use std::net::IpAddr;

/// A CIDR network (or single IP, parsed as a full-length prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpNet {
    network: IpAddr,
    prefix: u8,
}

impl IpNet {
    /// Parse `10.0.0.0/8`, `192.168.1.5` (bare IP → `/32` or `/128`).
    /// Returns `None` for malformed input or a prefix wider than the
    /// address family allows.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (addr_s, prefix_s) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = addr_s.trim().parse().ok()?;
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix_s {
            Some(p) => p.trim().parse::<u8>().ok().filter(|&p| p <= max)?,
            None => max,
        };
        Some(Self {
            network: apply_mask(addr, prefix),
            prefix,
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        apply_mask(ip, self.prefix) == self.network
    }
}

/// Zero out the host bits of `ip` for a `/prefix` network.
fn apply_mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let prefix = prefix.min(32);
            let bits = u32::from(v4);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            IpAddr::V4((bits & mask).into())
        }
        IpAddr::V6(v6) => {
            let prefix = prefix.min(128);
            let bits = u128::from(v6);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            IpAddr::V6((bits & mask).into())
        }
    }
}

/// The configured trusted-proxy list. Empty (the default) means every
/// connection is treated as direct: forwarded headers are ignored and
/// the socket peer is always the source.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies {
    nets: Vec<IpNet>,
}

impl TrustedProxies {
    /// Parse a comma-separated list of CIDRs / bare IPs. Malformed
    /// entries are skipped (an unparseable proxy list must not widen
    /// trust to everyone).
    pub fn from_csv(s: &str) -> Self {
        Self {
            nets: s.split(',').filter_map(IpNet::parse).collect(),
        }
    }

    /// Read `GYRE_TRUSTED_PROXIES` (comma-separated CIDRs / bare IPs).
    pub fn from_env() -> Self {
        Self::from_csv(&std::env::var("GYRE_TRUSTED_PROXIES").unwrap_or_default())
    }

    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    pub fn contains(&self, ip: &IpAddr) -> bool {
        self.nets.iter().any(|n| n.contains(*ip))
    }

    /// Resolve the forensic source IP for a request.
    ///
    /// `peer` is the socket peer address (transport-guaranteed);
    /// `headers` are the request headers (caller-controlled).
    pub fn resolve_source_ip(&self, peer: Option<IpAddr>, headers: &HeaderMap) -> Option<String> {
        let peer = peer?;
        if !self.contains(&peer) {
            // Direct connection: the peer address IS the source; forwarded
            // headers are caller-controlled and ignored (CWE-348).
            return Some(peer.to_string());
        }
        // forwarded-header-trust:ok — peer is validated against trusted proxies
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            // Rightmost-untrusted: with a trusted reverse proxy in front, the
            // rightmost XFF entry that is not itself a trusted proxy is the
            // client the chain started from.
            for candidate in xff.split(',').rev().map(|s| s.trim()) {
                if let Ok(ip) = candidate.parse::<IpAddr>() {
                    if !self.contains(&ip) {
                        return Some(ip.to_string());
                    }
                }
            }
        }
        // forwarded-header-trust:ok — peer is validated against trusted proxies
        if let Some(rip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = rip.trim().parse::<IpAddr>() {
                return Some(ip.to_string());
            }
        }
        Some(peer.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                axum::http::HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn ipnet_parse_and_contains() {
        let net = IpNet::parse("10.0.0.0/8").unwrap();
        assert!(net.contains("10.1.2.3".parse().unwrap()));
        assert!(!net.contains("192.168.1.1".parse().unwrap()));
        // Bare IP → /32.
        let single = IpNet::parse("192.168.1.5").unwrap();
        assert!(single.contains("192.168.1.5".parse().unwrap()));
        assert!(!single.contains("192.168.1.6".parse().unwrap()));
        // IPv6 and /0.
        let v6 = IpNet::parse("fd00::/16").unwrap();
        assert!(v6.contains("fd00::1".parse().unwrap()));
        assert!(!v6.contains("fe80::1".parse().unwrap()));
        assert!(IpNet::parse("0.0.0.0/0")
            .unwrap()
            .contains("1.2.3.4".parse().unwrap()));
        // Host bits are masked into the network at parse time.
        assert_eq!(
            IpNet::parse("10.1.2.3/8").unwrap(),
            IpNet::parse("10.0.0.0/8").unwrap()
        );
        // Malformed input is rejected, never widened.
        assert!(IpNet::parse("not-an-ip").is_none());
        assert!(IpNet::parse("10.0.0.0/33").is_none());
        assert!(IpNet::parse("fd00::/129").is_none());
        assert!(IpNet::parse("").is_none());
    }

    #[test]
    fn from_csv_skips_malformed_entries() {
        let tp = TrustedProxies::from_csv("10.0.0.0/8, bogus, 192.168.0.0/16");
        assert!(tp.contains(&"10.0.0.1".parse().unwrap()));
        assert!(tp.contains(&"192.168.5.5".parse().unwrap()));
        assert!(!tp.contains(&"172.16.0.1".parse().unwrap()));
    }

    #[test]
    fn resolve_direct_peer_ignores_forwarded_headers() {
        // Default deployment: no trusted proxies → the socket peer is the
        // source and forged X-Forwarded-For is ignored (CWE-348, F3).
        let tp = TrustedProxies::default();
        let h = headers(&[("x-forwarded-for", "1.2.3.4"), ("x-real-ip", "5.6.7.8")]);
        assert_eq!(
            tp.resolve_source_ip(Some("203.0.113.9".parse().unwrap()), &h),
            Some("203.0.113.9".to_string())
        );
    }

    #[test]
    fn resolve_trusted_peer_uses_rightmost_untrusted_xff() {
        let tp = TrustedProxies::from_csv("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "198.51.100.7, 10.0.0.1, 10.0.0.2")]);
        assert_eq!(
            tp.resolve_source_ip(Some("10.0.0.2".parse().unwrap()), &h),
            Some("198.51.100.7".to_string())
        );
    }

    #[test]
    fn resolve_trusted_peer_all_trusted_xff_falls_to_x_real_ip_then_peer() {
        let tp = TrustedProxies::from_csv("10.0.0.0/8");
        // Every XFF entry is itself a trusted proxy → X-Real-Ip.
        let h = headers(&[
            ("x-forwarded-for", "10.0.0.1"),
            ("x-real-ip", "198.51.100.7"),
        ]);
        assert_eq!(
            tp.resolve_source_ip(Some("10.0.0.2".parse().unwrap()), &h),
            Some("198.51.100.7".to_string())
        );
        // Neither header yields a usable value → the trusted peer itself.
        let h = headers(&[("x-forwarded-for", "garbage")]);
        assert_eq!(
            tp.resolve_source_ip(Some("10.0.0.2".parse().unwrap()), &h),
            Some("10.0.0.2".to_string())
        );
    }

    #[test]
    fn resolve_fails_closed_without_socket_peer() {
        // No ConnectInfo (e.g. a mis-served route): headers are never
        // trusted on their own.
        let tp = TrustedProxies::from_csv("0.0.0.0/0");
        let h = headers(&[("x-forwarded-for", "1.2.3.4")]);
        assert_eq!(tp.resolve_source_ip(None, &h), None);
    }

    #[test]
    fn untrusted_peer_with_trusted_xff_still_uses_peer() {
        // A non-proxy client cannot buy trust by naming a proxy IP.
        let tp = TrustedProxies::from_csv("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "10.0.0.1")]);
        assert_eq!(
            tp.resolve_source_ip(Some("203.0.113.9".parse().unwrap()), &h),
            Some("203.0.113.9".to_string())
        );
    }
}
