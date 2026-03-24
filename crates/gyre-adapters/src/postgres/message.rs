use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{
    message::{Destination, Message, MessageKind, MessageOrigin},
    Id,
};
use gyre_ports::MessageRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::messages;

// ── Row structs ───────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Queryable, Selectable)]
#[diesel(table_name = messages)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MessageRow {
    id: String,
    tenant_id: String,
    from_type: String,
    from_id: Option<String>,
    workspace_id: String,
    to_type: String,
    to_id: Option<String>,
    kind: String,
    payload: Option<String>,
    created_at: i64,
    signature: Option<String>,
    key_id: Option<String>,
    acknowledged: i32,
    ack_reason: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = messages)]
struct NewMessageRow {
    id: String,
    tenant_id: String,
    from_type: String,
    from_id: Option<String>,
    workspace_id: String,
    to_type: String,
    to_id: Option<String>,
    kind: String,
    payload: Option<String>,
    created_at: i64,
    signature: Option<String>,
    key_id: Option<String>,
    acknowledged: i32,
    ack_reason: Option<String>,
}

// ── Mapping helpers ───────────────────────────────────────────────────────────

impl MessageRow {
    fn into_message(self) -> Result<Message> {
        let from = match self.from_type.as_str() {
            "server" => MessageOrigin::Server,
            "agent" => MessageOrigin::Agent(Id::new(
                self.from_id
                    .ok_or_else(|| anyhow!("agent origin missing from_id"))?,
            )),
            "user" => MessageOrigin::User(Id::new(
                self.from_id
                    .ok_or_else(|| anyhow!("user origin missing from_id"))?,
            )),
            other => return Err(anyhow!("unknown from_type: {other}")),
        };

        let to = match self.to_type.as_str() {
            "agent" => Destination::Agent(Id::new(
                self.to_id
                    .ok_or_else(|| anyhow!("agent dest missing to_id"))?,
            )),
            "workspace" => Destination::Workspace(Id::new(
                self.to_id
                    .ok_or_else(|| anyhow!("workspace dest missing to_id"))?,
            )),
            other => return Err(anyhow!("unknown to_type: {other}")),
        };

        let payload = self
            .payload
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .context("parse message payload JSON")?;

        let kind: MessageKind = serde_json::from_value(serde_json::Value::String(self.kind))
            .context("parse message kind")?;

        Ok(Message {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            from,
            workspace_id: Some(Id::new(self.workspace_id)),
            to,
            kind,
            payload,
            created_at: self.created_at as u64,
            signature: self.signature,
            key_id: self.key_id,
            acknowledged: self.acknowledged != 0,
        })
    }
}

fn origin_to_row(origin: &MessageOrigin) -> (String, Option<String>) {
    match origin {
        MessageOrigin::Server => ("server".to_string(), None),
        MessageOrigin::Agent(id) => ("agent".to_string(), Some(id.as_str().to_string())),
        MessageOrigin::User(id) => ("user".to_string(), Some(id.as_str().to_string())),
    }
}

fn dest_to_row(dest: &Destination) -> Result<(String, Option<String>)> {
    match dest {
        Destination::Agent(id) => Ok(("agent".to_string(), Some(id.as_str().to_string()))),
        Destination::Workspace(id) => Ok(("workspace".to_string(), Some(id.as_str().to_string()))),
        Destination::Broadcast => Err(anyhow!("Broadcast messages must not be stored in the DB")),
    }
}

fn message_to_new_row(m: &Message) -> Result<NewMessageRow> {
    let workspace_id = m
        .workspace_id
        .as_ref()
        .ok_or_else(|| anyhow!("cannot store Broadcast message: workspace_id is None"))?;

    let (from_type, from_id) = origin_to_row(&m.from);
    let (to_type, to_id) = dest_to_row(&m.to)?;
    let payload = m
        .payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("serialize payload")?;

    Ok(NewMessageRow {
        id: m.id.as_str().to_string(),
        tenant_id: m.tenant_id.as_str().to_string(),
        from_type,
        from_id,
        workspace_id: workspace_id.as_str().to_string(),
        to_type,
        to_id,
        kind: m.kind.to_string(),
        payload,
        created_at: m.created_at as i64,
        signature: m.signature.clone(),
        key_id: m.key_id.clone(),
        // Always store as unacknowledged — ack state is set only via acknowledge() calls.
        acknowledged: 0,
        ack_reason: None,
    })
}

// ── MessageRepository impl ────────────────────────────────────────────────────

#[async_trait]
impl MessageRepository for PgStorage {
    async fn store(&self, message: &Message) -> Result<()> {
        let row = message_to_new_row(message)?;
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::insert_into(messages::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert message")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Message>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Message>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = messages::table
                .find(id.as_str())
                .first::<MessageRow>(&mut *conn)
                .optional()
                .context("find message by id")?;
            result.map(MessageRow::into_message).transpose()
        })
        .await?
    }

    async fn list_after(
        &self,
        agent_id: &Id,
        after_ts: u64,
        after_id: Option<&Id>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.clone();
        let after_id_str = after_id.map(|id| id.as_str().to_string());
        let after_ts_i64 = after_ts as i64;
        let lim = limit as i64;

        tokio::task::spawn_blocking(move || -> Result<Vec<Message>> {
            let mut conn = pool.get().context("get db connection")?;

            let mut query = messages::table
                .filter(messages::to_type.eq("agent"))
                .filter(messages::to_id.eq(aid.as_str()))
                .order((messages::created_at.asc(), messages::id.asc()))
                .limit(lim)
                .into_boxed();

            if let Some(ref aid_str) = after_id_str {
                // Composite cursor: (created_at, id) > (after_ts, after_id)
                // Expressed as: created_at > after_ts OR (created_at = after_ts AND id > after_id)
                query = query.filter(
                    messages::created_at
                        .gt(after_ts_i64)
                        .or(messages::created_at
                            .eq(after_ts_i64)
                            .and(messages::id.gt(aid_str.as_str()))),
                );
            } else {
                query = query.filter(messages::created_at.gt(after_ts_i64));
            }

            let rows = query.load::<MessageRow>(&mut *conn).context("list_after")?;
            rows.into_iter().map(MessageRow::into_message).collect()
        })
        .await?
    }

    async fn list_unacked(&self, agent_id: &Id, limit: usize) -> Result<Vec<Message>> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.clone();
        let lim = limit as i64;

        tokio::task::spawn_blocking(move || -> Result<Vec<Message>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = messages::table
                .filter(messages::to_type.eq("agent"))
                .filter(messages::to_id.eq(aid.as_str()))
                .filter(messages::acknowledged.eq(0_i32))
                .order((messages::created_at.asc(), messages::id.asc()))
                .limit(lim)
                .load::<MessageRow>(&mut *conn)
                .context("list_unacked")?;
            rows.into_iter().map(MessageRow::into_message).collect()
        })
        .await?
    }

    async fn count_unacked(&self, agent_id: &Id) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.clone();

        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = messages::table
                .filter(messages::to_type.eq("agent"))
                .filter(messages::to_id.eq(aid.as_str()))
                .filter(messages::acknowledged.eq(0_i32))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count_unacked")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn acknowledge(&self, message_id: &Id, agent_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mid = message_id.clone();
        let aid = agent_id.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Only update if not already acknowledged — idempotent, returns Ok on 0 rows.
            diesel::update(
                messages::table
                    .filter(messages::id.eq(mid.as_str()))
                    .filter(messages::to_type.eq("agent"))
                    .filter(messages::to_id.eq(aid.as_str()))
                    .filter(messages::acknowledged.eq(0_i32)),
            )
            .set((
                messages::acknowledged.eq(1_i32),
                messages::ack_reason.eq("explicit"),
            ))
            .execute(&mut *conn)
            .context("acknowledge message")?;
            Ok(())
        })
        .await?
    }

    async fn acknowledge_all(&self, agent_id: &Id, reason: &str) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let aid = agent_id.clone();
        let reason_str = reason.to_string();

        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::update(
                messages::table
                    .filter(messages::to_type.eq("agent"))
                    .filter(messages::to_id.eq(aid.as_str()))
                    .filter(messages::acknowledged.eq(0_i32)),
            )
            .set((
                messages::acknowledged.eq(1_i32),
                messages::ack_reason.eq(&reason_str),
            ))
            .execute(&mut *conn)
            .context("acknowledge_all")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn list_by_workspace(
        &self,
        workspace_id: &Id,
        kind: Option<&str>,
        since: Option<u64>,
        before_ts: Option<u64>,
        before_id: Option<&Id>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.clone();
        let kind_str = kind.map(|s| s.to_string());
        let before_id_str = before_id.map(|id| id.as_str().to_string());
        let lim = limit.unwrap_or(100) as i64;

        tokio::task::spawn_blocking(move || -> Result<Vec<Message>> {
            let mut conn = pool.get().context("get db connection")?;

            let mut query = messages::table
                .filter(messages::workspace_id.eq(ws_id.as_str()))
                // list_by_workspace returns Event-tier workspace messages only;
                // Directed (agent-inbox) messages are accessed via list_after/list_unacked.
                .filter(messages::to_type.ne("agent"))
                .order((messages::created_at.desc(), messages::id.desc()))
                .limit(lim)
                .into_boxed();

            if let Some(ref k) = kind_str {
                query = query.filter(messages::kind.eq(k.as_str()));
            }

            if let Some(since_ts) = since {
                query = query.filter(messages::created_at.ge(since_ts as i64));
            }

            // Before cursor: (created_at, id) < (before_ts, before_id)
            // Expressed as: created_at < before_ts OR (created_at = before_ts AND id < before_id)
            if let Some(bts) = before_ts {
                let bts_i64 = bts as i64;
                if let Some(ref bid) = before_id_str {
                    query = query.filter(
                        messages::created_at.lt(bts_i64).or(messages::created_at
                            .eq(bts_i64)
                            .and(messages::id.lt(bid.as_str()))),
                    );
                } else {
                    query = query.filter(messages::created_at.lt(bts_i64));
                }
            }

            let rows = query
                .load::<MessageRow>(&mut *conn)
                .context("list_by_workspace")?;
            rows.into_iter().map(MessageRow::into_message).collect()
        })
        .await?
    }

    async fn expire_events(&self, older_than: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let older_than_i64 = older_than as i64;

        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::delete(
                messages::table
                    .filter(messages::to_type.ne("agent"))
                    .filter(messages::created_at.lt(older_than_i64)),
            )
            .execute(&mut *conn)
            .context("expire_events")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn expire_acked_inboxes(&self, older_than: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let older_than_i64 = older_than as i64;

        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::delete(
                messages::table
                    .filter(messages::to_type.eq("agent"))
                    .filter(
                        messages::ack_reason
                            .eq("agent_completed")
                            .or(messages::ack_reason.eq("agent_orphaned")),
                    )
                    .filter(messages::created_at.lt(older_than_i64)),
            )
            .execute(&mut *conn)
            .context("expire_acked_inboxes")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn expire_for_agents(&self, agent_ids: &[Id], older_than: u64) -> Result<u64> {
        if agent_ids.is_empty() {
            return Ok(0);
        }
        let pool = Arc::clone(&self.pool);
        let ids: Vec<String> = agent_ids.iter().map(|id| id.as_str().to_string()).collect();
        let older_than_i64 = older_than as i64;

        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let ids_ref: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            let count = diesel::delete(
                messages::table
                    .filter(messages::to_type.eq("agent"))
                    .filter(messages::to_id.eq_any(ids_ref))
                    .filter(messages::created_at.lt(older_than_i64)),
            )
            .execute(&mut *conn)
            .context("expire_for_agents")?;
            Ok(count as u64)
        })
        .await?
    }
}
