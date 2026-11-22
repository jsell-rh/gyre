//! SPIFFE ID parsing and construction (SPIFFE SPIFFE-ID specification).

use std::fmt;

/// A parsed SPIFFE ID: `spiffe://{trust-domain}/{path}`.
///
/// Gyre agent workloads use the identity-security.md Layer 1 form
/// `spiffe://{trust-domain}/agent/{role}/session/{id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiffeId {
    /// Trust domain (e.g. `gyre.example.com`). Lowercase; no `/`, no scheme.
    pub trust_domain: String,
    /// Path segments after the trust domain (e.g.
    /// `["agent", "worker", "session", "<uuid>"]`).
    pub path: Vec<String>,
}

/// The parsed `/agent/{role}/session/{id}` path of an agent workload SVID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPath {
    /// Role segment (persona slug or orchestrator tier, e.g. `worker`).
    pub role: String,
    /// Session id segment — the session's JWT subject for Gyre-minted
    /// sessions (`agent:<uuid>`; see gyre-server `auth::agent_subject`),
    /// so a federated verifier can bind it to the caller's `sub` by
    /// direct string comparison.
    pub session_id: String,
}

impl SpiffeId {
    /// Parse a `spiffe://` URI. Fails closed on any deviation from the
    /// SPIFFE ID format: the scheme must be exactly `spiffe://`, the trust
    /// domain must be non-empty and consist only of the characters the
    /// SPIFFE ID specification allows (lowercase letters, digits, dots,
    /// hyphens, underscores), and the path must be non-empty, absolute,
    /// and free of `.`/`..` segments, query strings, and fragments.
    pub fn parse(s: &str) -> Result<Self, IdError> {
        let rest = s.strip_prefix("spiffe://").ok_or(IdError::Malformed(
            "SPIFFE ID must start with spiffe://".to_string(),
        ))?;
        // Split trust domain from path. `?`/`#` are not allowed anywhere.
        if rest.contains('?') || rest.contains('#') {
            return Err(IdError::Malformed(
                "SPIFFE ID must not contain ? or #".to_string(),
            ));
        }
        let (td, path) = match rest.split_once('/') {
            Some((td, path)) => (td, path),
            None => (rest, ""),
        };
        if td.is_empty() {
            return Err(IdError::Malformed(
                "trust domain must not be empty".to_string(),
            ));
        }
        if !td.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_'
        }) {
            return Err(IdError::Malformed(
                "trust domain characters must be lowercase letters, digits, '.', '-', '_' (per the SPIFFE ID specification)"
                    .to_string(),
            ));
        }
        if path.is_empty() {
            return Err(IdError::Malformed(
                "SPIFFE ID path must not be empty".to_string(),
            ));
        }
        let segments: Vec<String> = path.split('/').map(|s| s.to_string()).collect();
        if segments.iter().any(|s| s.is_empty()) {
            return Err(IdError::Malformed(
                "SPIFFE ID path must not have empty segments".to_string(),
            ));
        }
        if segments.iter().any(|s| s == "." || s == "..") {
            return Err(IdError::Malformed(
                "SPIFFE ID path must not contain '.' or '..' segments".to_string(),
            ));
        }
        Ok(Self {
            trust_domain: td.to_string(),
            path: segments,
        })
    }

    /// Build the agent workload SPIFFE ID
    /// `spiffe://{trust_domain}/agent/{role}/session/{session_id}`.
    ///
    /// `role` and `session_id` must be non-empty and free of `/` so the
    /// produced URI round-trips through [`SpiffeId::parse`].
    pub fn agent(trust_domain: &str, role: &str, session_id: &str) -> Result<Self, IdError> {
        if trust_domain.is_empty() {
            return Err(IdError::Malformed(
                "trust domain must not be empty".to_string(),
            ));
        }
        for (segment, name) in [(role, "role"), (session_id, "session id")] {
            if segment.is_empty() {
                return Err(IdError::Malformed(format!("{name} must not be empty")));
            }
            if segment.contains('/') {
                return Err(IdError::Malformed(format!("{name} must not contain '/'")));
            }
        }
        let s = format!("spiffe://{trust_domain}/agent/{role}/session/{session_id}");
        Self::parse(&s)
    }

    /// Parse the `/agent/{role}/session/{id}` path.
    ///
    /// Returns `None` when the path is not exactly four segments in the
    /// agent-workload shape.
    pub fn agent_path(&self) -> Option<AgentPath> {
        if self.path.len() == 4 && self.path[0] == "agent" && self.path[2] == "session" {
            Some(AgentPath {
                role: self.path[1].clone(),
                session_id: self.path[3].clone(),
            })
        } else {
            None
        }
    }
}

impl fmt::Display for SpiffeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "spiffe://{}/{}", self.trust_domain, self.path.join("/"))
    }
}

/// Build the agent workload SPIFFE ID for a session. Convenience wrapper
/// around [`SpiffeId::agent`] that panics only on programmatically-invalid
/// inputs (empty/`/`-containing segments) — callers pass server-derived
/// role/session values.
pub fn agent_spiffe_id(trust_domain: &str, role: &str, session_id: &str) -> SpiffeId {
    SpiffeId::agent(trust_domain, role, session_id)
        .expect("agent SPIFFE ID inputs are server-derived and non-empty")
}

/// Errors from SPIFFE ID parsing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("{0}")]
    Malformed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_form() {
        let id = SpiffeId::parse("spiffe://gyre.example.com/agent/worker/session/abc-123").unwrap();
        assert_eq!(id.trust_domain, "gyre.example.com");
        assert_eq!(id.path, vec!["agent", "worker", "session", "abc-123"]);
        let p = id.agent_path().unwrap();
        assert_eq!(p.role, "worker");
        assert_eq!(p.session_id, "abc-123");
        assert_eq!(
            id.to_string(),
            "spiffe://gyre.example.com/agent/worker/session/abc-123"
        );
    }

    #[test]
    fn roundtrips_construction() {
        let id = SpiffeId::agent("gyre.example.com", "security", "uuid-1").unwrap();
        assert_eq!(
            id.to_string(),
            "spiffe://gyre.example.com/agent/security/session/uuid-1"
        );
        let parsed = SpiffeId::parse(&id.to_string()).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn rejects_foreign_schemes_and_shapes() {
        // Wrong/missing scheme.
        assert!(SpiffeId::parse("https://gyre.example.com/agent").is_err());
        assert!(SpiffeId::parse("spiffe:/gyre.example.com/agent").is_err());
        // No path.
        assert!(SpiffeId::parse("spiffe://gyre.example.com").is_err());
        assert!(SpiffeId::parse("spiffe://gyre.example.com/").is_err());
        // Empty/relative/parent segments.
        assert!(SpiffeId::parse("spiffe://td/a//b").is_err());
        assert!(SpiffeId::parse("spiffe://td/a/../b").is_err());
        assert!(SpiffeId::parse("spiffe://td/a/./b").is_err());
        // Query / fragment.
        assert!(SpiffeId::parse("spiffe://td/a?x=1").is_err());
        assert!(SpiffeId::parse("spiffe://td/a#f").is_err());
        // Uppercase trust domain (SPIFFE IDs are case-sensitive and trust
        // domains are lowercase).
        assert!(SpiffeId::parse("spiffe://TD/a").is_err());
    }

    #[test]
    fn agent_path_rejects_non_agent_shapes() {
        let id = SpiffeId::parse("spiffe://td/agent/worker/not-session/x").unwrap();
        assert!(id.agent_path().is_none());
        let id = SpiffeId::parse("spiffe://td/agent/worker/session").unwrap();
        assert!(id.agent_path().is_none());
    }
}
