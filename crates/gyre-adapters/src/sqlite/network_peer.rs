use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::NetworkPeer;
use gyre_ports::NetworkPeerRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::network_peers;

#[derive(Queryable, Selectable)]
#[diesel(table_name = network_peers)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct NetworkPeerRow {
    id: String,
    agent_id: String,
    wireguard_pubkey: String,
    endpoint: Option<String>,
    allowed_ips: String,
    registered_at: i64,
    last_seen: Option<i64>,
}

impl NetworkPeerRow {
    fn into_peer(self) -> Result<NetworkPeer> {
        let allowed_ips: Vec<String> =
            serde_json::from_str(&self.allowed_ips).context("parse allowed_ips JSON")?;
        Ok(NetworkPeer {
            id: Id::new(self.id),
            agent_id: Id::new(self.agent_id),
            wireguard_pubkey: self.wireguard_pubkey,
            endpoint: self.endpoint,
            allowed_ips,
            registered_at: self.registered_at as u64,
            last_seen: self.last_seen.map(|v| v as u64),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = network_peers)]
struct NetworkPeerRecord<'a> {
    id: &'a str,
    agent_id: &'a str,
    wireguard_pubkey: &'a str,
    endpoint: Option<&'a str>,
    allowed_ips: String,
    registered_at: i64,
    last_seen: Option<i64>,
}

#[async_trait]
impl NetworkPeerRepository for SqliteStorage {
    async fn register(&self, peer: &NetworkPeer) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = peer.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let allowed_ips = serde_json::to_string(&p.allowed_ips)?;
            let record = NetworkPeerRecord {
                id: p.id.as_str(),
                agent_id: p.agent_id.as_str(),
                wireguard_pubkey: &p.wireguard_pubkey,
                endpoint: p.endpoint.as_deref(),
                allowed_ips,
                registered_at: p.registered_at as i64,
                last_seen: p.last_seen.map(|v| v as i64),
            };
            diesel::insert_into(network_peers::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert network_peer")?;
            Ok(())
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<NetworkPeer>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<NetworkPeer>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = network_peers::table
                .order(network_peers::registered_at.asc())
                .load::<NetworkPeerRow>(&mut *conn)
                .context("list network_peers")?;
            rows.into_iter().map(|r| r.into_peer()).collect()
        })
        .await?
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Option<NetworkPeer>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<NetworkPeer>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = network_peers::table
                .filter(network_peers::agent_id.eq(agent_id.as_str()))
                .first::<NetworkPeerRow>(&mut *conn)
                .optional()
                .context("find network_peer by agent")?;
            result.map(|r| r.into_peer()).transpose()
        })
        .await?
    }

    async fn update_last_seen(&self, id: &Id, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(network_peers::table.find(id.as_str()))
                .set(network_peers::last_seen.eq(Some(now as i64)))
                .execute(&mut *conn)
                .context("update_last_seen")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(network_peers::table.find(id.as_str()))
                .execute(&mut *conn)
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
