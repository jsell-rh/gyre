use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPeer {
    pub id: Id,
    pub agent_id: Id,
    pub wireguard_pubkey: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<String>,
    pub registered_at: u64,
    pub last_seen: Option<u64>,
}

impl NetworkPeer {
    pub fn new(
        id: Id,
        agent_id: Id,
        wireguard_pubkey: impl Into<String>,
        endpoint: Option<String>,
        allowed_ips: Vec<String>,
        registered_at: u64,
    ) -> Self {
        Self {
            id,
            agent_id,
            wireguard_pubkey: wireguard_pubkey.into(),
            endpoint,
            allowed_ips,
            registered_at,
            last_seen: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer() -> NetworkPeer {
        NetworkPeer::new(
            Id::new("p1"),
            Id::new("a1"),
            "base64pubkey==",
            Some("10.0.0.1:51820".to_string()),
            vec!["10.0.0.2/32".to_string()],
            1000,
        )
    }

    #[test]
    fn new_peer_has_no_last_seen() {
        let p = make_peer();
        assert!(p.last_seen.is_none());
    }

    #[test]
    fn peer_fields_set_correctly() {
        let p = make_peer();
        assert_eq!(p.wireguard_pubkey, "base64pubkey==");
        assert_eq!(p.endpoint, Some("10.0.0.1:51820".to_string()));
        assert_eq!(p.allowed_ips, vec!["10.0.0.2/32"]);
        assert_eq!(p.registered_at, 1000);
    }
}
