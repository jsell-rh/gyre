use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::NetworkPeer;
use gyre_ports::NetworkPeerRepository;

use super::{open_conn, SqliteStorage};

fn row_to_peer(row: &rusqlite::Row<'_>) -> Result<NetworkPeer> {
    let allowed_ips_json: String = row.get(4)?;
    let allowed_ips: Vec<String> =
        serde_json::from_str(&allowed_ips_json).context("parse allowed_ips JSON")?;
    Ok(NetworkPeer {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        wireguard_pubkey: row.get(2)?,
        endpoint: row.get(3)?,
        allowed_ips,
        registered_at: row.get::<_, i64>(5)? as u64,
        last_seen: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
    })
}

#[async_trait]
impl NetworkPeerRepository for SqliteStorage {
    async fn register(&self, peer: &NetworkPeer) -> Result<()> {
        let path = self.db_path();
        let p = peer.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            let allowed_ips_json = serde_json::to_string(&p.allowed_ips)?;
            conn.execute(
                "INSERT INTO network_peers
                     (id, agent_id, wireguard_pubkey, endpoint, allowed_ips,
                      registered_at, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    p.id.as_str(),
                    p.agent_id.as_str(),
                    p.wireguard_pubkey,
                    p.endpoint,
                    allowed_ips_json,
                    p.registered_at as i64,
                    p.last_seen.map(|v| v as i64),
                ],
            )
            .context("insert network_peer")?;
            Ok(())
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<NetworkPeer>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<NetworkPeer>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, wireguard_pubkey, endpoint, allowed_ips,
                        registered_at, last_seen
                 FROM network_peers ORDER BY registered_at",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_peer(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Option<NetworkPeer>> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<NetworkPeer>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, wireguard_pubkey, endpoint, allowed_ips,
                        registered_at, last_seen
                 FROM network_peers WHERE agent_id = ?1",
            )?;
            let mut rows = stmt.query([agent_id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_peer(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn update_last_seen(&self, id: &Id, now: u64) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE network_peers SET last_seen = ?1 WHERE id = ?2",
                rusqlite::params![now as i64, id.as_str()],
            )
            .context("update_last_seen")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM network_peers WHERE id = ?1", [id.as_str()])
                .context("delete network_peer")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_peer(id: &str, agent_id: &str) -> NetworkPeer {
        NetworkPeer::new(
            Id::new(id),
            Id::new(agent_id),
            "pubkey==",
            Some("10.0.0.1:51820".to_string()),
            vec!["10.100.0.2/32".to_string()],
            1000,
        )
    }

    #[tokio::test]
    async fn register_and_list() {
        let (_tmp, s) = setup();
        let p = make_peer("p1", "a1");
        s.register(&p).await.unwrap();
        let peers = s.list().await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].wireguard_pubkey, "pubkey==");
    }

    #[tokio::test]
    async fn find_by_agent() {
        let (_tmp, s) = setup();
        let p = make_peer("p1", "agent-42");
        s.register(&p).await.unwrap();
        let found = s.find_by_agent(&Id::new("agent-42")).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, Id::new("p1"));
    }

    #[tokio::test]
    async fn find_by_agent_missing() {
        let (_tmp, s) = setup();
        let found = s.find_by_agent(&Id::new("ghost")).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn update_last_seen() {
        let (_tmp, s) = setup();
        let p = make_peer("p1", "a1");
        s.register(&p).await.unwrap();
        s.update_last_seen(&Id::new("p1"), 9999).await.unwrap();
        let found = s.find_by_agent(&Id::new("a1")).await.unwrap().unwrap();
        assert_eq!(found.last_seen, Some(9999));
    }

    #[tokio::test]
    async fn delete_peer() {
        let (_tmp, s) = setup();
        let p = make_peer("p1", "a1");
        s.register(&p).await.unwrap();
        s.delete(&Id::new("p1")).await.unwrap();
        assert!(s.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn allowed_ips_roundtrip() {
        let (_tmp, s) = setup();
        let mut p = make_peer("p1", "a1");
        p.allowed_ips = vec!["10.0.0.0/8".to_string(), "192.168.1.0/24".to_string()];
        s.register(&p).await.unwrap();
        let found = s.find_by_agent(&Id::new("a1")).await.unwrap().unwrap();
        assert_eq!(found.allowed_ips, vec!["10.0.0.0/8", "192.168.1.0/24"]);
    }
}
