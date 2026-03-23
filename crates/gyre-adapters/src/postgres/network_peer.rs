use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::NetworkPeer;
use gyre_ports::NetworkPeerRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::network_peers;

#[derive(Queryable, Selectable)]
#[diesel(table_name = network_peers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct NetworkPeerRow {
    id: String,
    agent_id: String,
    wireguard_pubkey: String,
    endpoint: Option<String>,
    allowed_ips: String,
    registered_at: i64,
    last_seen: Option<i64>,
    mesh_ip: Option<String>,
    is_stale: bool,
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
            mesh_ip: self.mesh_ip,
            is_stale: self.is_stale,
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
    mesh_ip: Option<&'a str>,
    is_stale: bool,
}

#[async_trait]
impl NetworkPeerRepository for PgStorage {
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
                mesh_ip: p.mesh_ip.as_deref(),
                is_stale: p.is_stale,
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

    async fn update_endpoint(&self, id: &Id, endpoint: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let endpoint = endpoint.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(network_peers::table.find(id.as_str()))
                .set(network_peers::endpoint.eq(Some(endpoint)))
                .execute(&mut *conn)
                .context("update_endpoint")?;
            Ok(())
        })
        .await?
    }

    async fn mark_stale_older_than(&self, cutoff: u64) -> Result<usize> {
        let pool = Arc::clone(&self.pool);
        let cutoff_i64 = cutoff as i64;
        tokio::task::spawn_blocking(move || -> Result<usize> {
            let mut conn = pool.get().context("get db connection")?;
            let n = diesel::update(
                network_peers::table
                    .filter(network_peers::is_stale.eq(false))
                    .filter(
                        network_peers::last_seen
                            .lt(cutoff_i64)
                            .or(network_peers::last_seen
                                .is_null()
                                .and(network_peers::registered_at.lt(cutoff_i64))),
                    ),
            )
            .set(network_peers::is_stale.eq(true))
            .execute(&mut *conn)
            .context("mark_stale_older_than")?;
            Ok(n)
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
