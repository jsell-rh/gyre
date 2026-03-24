use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{
    message::{Destination, Message, MessageKind, MessageOrigin},
    Id,
};
use gyre_ports::MessageRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::messages;

// ── Row structs ───────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Queryable, Selectable)]
#[diesel(table_name = messages)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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

fn dest_to_row(dest: &Destination) -> (String, Option<String>) {
    match dest {
        Destination::Agent(id) => ("agent".to_string(), Some(id.as_str().to_string())),
        Destination::Workspace(id) => ("workspace".to_string(), Some(id.as_str().to_string())),
        Destination::Broadcast => ("broadcast".to_string(), None),
    }
}

fn message_to_new_row(m: &Message) -> Result<NewMessageRow> {
    let workspace_id = m
        .workspace_id
        .as_ref()
        .ok_or_else(|| anyhow!("cannot store Broadcast message: workspace_id is None"))?;

    let (from_type, from_id) = origin_to_row(&m.from);
    let (to_type, to_id) = dest_to_row(&m.to);
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
        acknowledged: m.acknowledged as i32,
        ack_reason: None,
    })
}

// ── MessageRepository impl ────────────────────────────────────────────────────

#[async_trait]
impl MessageRepository for SqliteStorage {
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

// ── Integration tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    fn make_directed(id: &str, agent_id: &str, workspace_id: &str, created_at: u64) -> Message {
        Message {
            id: Id::new(id),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new(workspace_id)),
            to: Destination::Agent(Id::new(agent_id)),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at,
            signature: Some("sig".to_string()),
            key_id: Some("kid-1".to_string()),
            acknowledged: false,
        }
    }

    fn make_workspace_event(
        id: &str,
        workspace_id: &str,
        kind: MessageKind,
        created_at: u64,
    ) -> Message {
        Message {
            id: Id::new(id),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new(workspace_id)),
            to: Destination::Workspace(Id::new(workspace_id)),
            kind,
            payload: None,
            created_at,
            signature: Some("sig".to_string()),
            key_id: Some("kid-1".to_string()),
            acknowledged: false,
        }
    }

    #[tokio::test]
    async fn store_and_find_by_id() {
        let (_tmp, storage) = tmp_storage();
        let msg = make_directed("msg-1", "agent-1", "ws-1", 1_000_000);
        storage.store(&msg).await.unwrap();
        let found = storage
            .find_by_id(&Id::new("msg-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, msg.id);
        assert_eq!(found.kind, MessageKind::TaskAssignment);
        assert_eq!(found.workspace_id, Some(Id::new("ws-1")));
        assert!(!found.acknowledged);
    }

    #[tokio::test]
    async fn find_by_id_missing_returns_none() {
        let (_tmp, storage) = tmp_storage();
        let result = storage.find_by_id(&Id::new("no-such-id")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn store_broadcast_returns_error() {
        let (_tmp, storage) = tmp_storage();
        let msg = Message {
            id: Id::new("msg-broadcast"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Server,
            workspace_id: None, // Broadcast
            to: Destination::Broadcast,
            kind: MessageKind::DataSeeded,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        assert!(storage.store(&msg).await.is_err());
    }

    #[tokio::test]
    async fn list_after_cursor_pagination() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-cursor");
        let ws = "ws-cursor";

        // Insert 3 messages with different timestamps
        for i in 1u64..=3 {
            let msg = make_directed(&format!("msg-{i}"), "agent-cursor", ws, i * 1000);
            storage.store(&msg).await.unwrap();
        }

        // First page: after_ts=0, no after_id → get all
        let page1 = storage.list_after(&agent_id, 0, None, 10).await.unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].id, Id::new("msg-1"));
        assert_eq!(page1[2].id, Id::new("msg-3"));

        // Cursor after first message: after_ts=1000, no after_id → strict gt
        let page2 = storage.list_after(&agent_id, 1000, None, 10).await.unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].id, Id::new("msg-2"));

        // Composite cursor: after_ts=1000, after_id="msg-1" → items after (1000, msg-1)
        let page3 = storage
            .list_after(&agent_id, 1000, Some(&Id::new("msg-1")), 10)
            .await
            .unwrap();
        assert_eq!(page3.len(), 2);
        assert_eq!(page3[0].id, Id::new("msg-2"));
    }

    #[tokio::test]
    async fn list_after_same_timestamp_composite_cursor() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-same-ts");
        let ws = "ws-same-ts";

        // Two messages at the same timestamp, different IDs
        let msg_a = make_directed("a-msg", "agent-same-ts", ws, 5000);
        let msg_b = make_directed("b-msg", "agent-same-ts", ws, 5000);
        storage.store(&msg_a).await.unwrap();
        storage.store(&msg_b).await.unwrap();

        // Composite cursor: after (5000, "a-msg") → only b-msg remains
        let results = storage
            .list_after(&agent_id, 5000, Some(&Id::new("a-msg")), 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("b-msg"));
    }

    #[tokio::test]
    async fn list_unacked_returns_unacked_only() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-unacked");

        for i in 1u64..=3 {
            storage
                .store(&make_directed(
                    &format!("u-msg-{i}"),
                    "agent-unacked",
                    "ws-u",
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        // Ack one
        storage
            .acknowledge(&Id::new("u-msg-1"), &agent_id)
            .await
            .unwrap();

        let unacked = storage.list_unacked(&agent_id, 100).await.unwrap();
        assert_eq!(unacked.len(), 2);
        assert!(unacked.iter().all(|m| m.id != Id::new("u-msg-1")));
    }

    #[tokio::test]
    async fn count_unacked() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-count");

        for i in 1u64..=4 {
            storage
                .store(&make_directed(
                    &format!("c-msg-{i}"),
                    "agent-count",
                    "ws-c",
                    i * 100,
                ))
                .await
                .unwrap();
        }

        assert_eq!(storage.count_unacked(&agent_id).await.unwrap(), 4);

        storage
            .acknowledge(&Id::new("c-msg-1"), &agent_id)
            .await
            .unwrap();
        assert_eq!(storage.count_unacked(&agent_id).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn ack_idempotency() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-idem");
        let msg = make_directed("idem-msg", "agent-idem", "ws-idem", 1_000);
        storage.store(&msg).await.unwrap();

        // Ack twice — both should return Ok
        storage
            .acknowledge(&Id::new("idem-msg"), &agent_id)
            .await
            .unwrap();
        storage
            .acknowledge(&Id::new("idem-msg"), &agent_id)
            .await
            .unwrap();

        // Should be acked exactly once
        let found = storage
            .find_by_id(&Id::new("idem-msg"))
            .await
            .unwrap()
            .unwrap();
        assert!(found.acknowledged);
    }

    #[tokio::test]
    async fn acknowledge_all_returns_count() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-bulk");

        for i in 1u64..=5 {
            storage
                .store(&make_directed(
                    &format!("bulk-{i}"),
                    "agent-bulk",
                    "ws-bulk",
                    i * 100,
                ))
                .await
                .unwrap();
        }

        let count = storage
            .acknowledge_all(&agent_id, "agent_completed")
            .await
            .unwrap();
        assert_eq!(count, 5);

        // Second bulk-ack → 0 since all already acked
        let count2 = storage
            .acknowledge_all(&agent_id, "agent_completed")
            .await
            .unwrap();
        assert_eq!(count2, 0);
    }

    #[tokio::test]
    async fn list_by_workspace_newest_first() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-newest");

        for i in 1u64..=4 {
            storage
                .store(&make_workspace_event(
                    &format!("ws-evt-{i}"),
                    "ws-newest",
                    MessageKind::AgentCreated,
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        let results = storage
            .list_by_workspace(&ws_id, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 4);
        // Newest first
        assert_eq!(results[0].id, Id::new("ws-evt-4"));
        assert_eq!(results[3].id, Id::new("ws-evt-1"));
    }

    #[tokio::test]
    async fn list_by_workspace_with_kind_filter() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-kind");

        storage
            .store(&make_workspace_event(
                "k-1",
                "ws-kind",
                MessageKind::AgentCreated,
                1000,
            ))
            .await
            .unwrap();
        storage
            .store(&make_workspace_event(
                "k-2",
                "ws-kind",
                MessageKind::TaskCreated,
                2000,
            ))
            .await
            .unwrap();
        storage
            .store(&make_workspace_event(
                "k-3",
                "ws-kind",
                MessageKind::AgentCreated,
                3000,
            ))
            .await
            .unwrap();

        let results = storage
            .list_by_workspace(&ws_id, Some("agent_created"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|m| m.kind == MessageKind::AgentCreated));
    }

    #[tokio::test]
    async fn list_by_workspace_since_filter() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-since");

        for i in 1u64..=5 {
            storage
                .store(&make_workspace_event(
                    &format!("s-{i}"),
                    "ws-since",
                    MessageKind::AgentCreated,
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        // since=2000 means created_at >= 2000
        let results = storage
            .list_by_workspace(&ws_id, None, Some(2000), None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 4); // ts 2000, 3000, 4000, 5000
    }

    #[tokio::test]
    async fn expire_events_removes_old_non_agent_messages() {
        let (_tmp, storage) = tmp_storage();
        let _ws_id = Id::new("ws-expire");

        // Old workspace event (to_type = 'workspace')
        storage
            .store(&make_workspace_event(
                "old-evt",
                "ws-expire",
                MessageKind::AgentCreated,
                1000,
            ))
            .await
            .unwrap();
        // New workspace event
        storage
            .store(&make_workspace_event(
                "new-evt",
                "ws-expire",
                MessageKind::AgentCreated,
                99_000,
            ))
            .await
            .unwrap();
        // Directed message to agent — should NOT be expired
        storage
            .store(&make_directed("agent-msg", "some-agent", "ws-expire", 500))
            .await
            .unwrap();

        // Expire everything older than 50_000 ms
        let deleted = storage.expire_events(50_000).await.unwrap();
        assert_eq!(deleted, 1); // only old-evt

        // new-evt and agent-msg should remain
        assert!(storage
            .find_by_id(&Id::new("new-evt"))
            .await
            .unwrap()
            .is_some());
        assert!(storage
            .find_by_id(&Id::new("agent-msg"))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn expire_acked_inboxes() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("dead-agent");

        for i in 1u64..=3 {
            storage
                .store(&make_directed(
                    &format!("dead-{i}"),
                    "dead-agent",
                    "ws-dead",
                    i * 100,
                ))
                .await
                .unwrap();
        }

        // Bulk-ack with agent_completed reason
        storage
            .acknowledge_all(&agent_id, "agent_completed")
            .await
            .unwrap();

        // Expire acked inboxes older than 1000 ms
        let deleted = storage.expire_acked_inboxes(1000).await.unwrap();
        assert_eq!(deleted, 3);
    }

    #[tokio::test]
    async fn list_by_workspace_before_cursor_pagination() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-cursor-page");

        // Store 5 events, each 1000ms apart
        for i in 1u64..=5 {
            storage
                .store(&make_workspace_event(
                    &format!("page-{i}"),
                    "ws-cursor-page",
                    MessageKind::AgentCreated,
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        // First page: newest 3 (no before cursor) → page-5, page-4, page-3
        let page1 = storage
            .list_by_workspace(&ws_id, None, None, None, None, Some(3))
            .await
            .unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].id, Id::new("page-5"));
        assert_eq!(page1[2].id, Id::new("page-3"));

        // Second page: before (3000, page-3) → page-2, page-1
        let page2 = storage
            .list_by_workspace(
                &ws_id,
                None,
                None,
                Some(3000),
                Some(&Id::new("page-3")),
                Some(3),
            )
            .await
            .unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].id, Id::new("page-2"));
        assert_eq!(page2[1].id, Id::new("page-1"));
    }

    #[tokio::test]
    async fn store_and_retrieve_with_payload() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-payload");
        let payload = serde_json::json!({
            "task_id": "TASK-42",
            "spec_ref": "specs/foo.md"
        });
        let msg = Message {
            id: Id::new("payload-msg"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::Agent(Id::new("sender-agent")),
            workspace_id: Some(ws_id),
            to: Destination::Agent(Id::new("recv-agent")),
            kind: MessageKind::TaskAssignment,
            payload: Some(payload.clone()),
            created_at: 50_000,
            signature: Some("sig".to_string()),
            key_id: Some("kid-1".to_string()),
            acknowledged: false,
        };
        storage.store(&msg).await.unwrap();
        let found = storage
            .find_by_id(&Id::new("payload-msg"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.payload.unwrap()["task_id"], "TASK-42");
        // Verify MessageOrigin::Agent round-trips
        assert_eq!(found.from, MessageOrigin::Agent(Id::new("sender-agent")));
        assert_eq!(found.to, Destination::Agent(Id::new("recv-agent")));
    }

    #[tokio::test]
    async fn store_and_retrieve_user_origin() {
        let (_tmp, storage) = tmp_storage();
        let msg = Message {
            id: Id::new("user-origin-msg"),
            tenant_id: Id::new("tenant-1"),
            from: MessageOrigin::User(Id::new("user-99")),
            workspace_id: Some(Id::new("ws-user")),
            to: Destination::Workspace(Id::new("ws-user")),
            kind: MessageKind::StatusUpdate,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        storage.store(&msg).await.unwrap();
        let found = storage
            .find_by_id(&Id::new("user-origin-msg"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.from, MessageOrigin::User(Id::new("user-99")));
        assert_eq!(found.to, Destination::Workspace(Id::new("ws-user")));
    }

    #[tokio::test]
    async fn expire_acked_inboxes_agent_orphaned() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("orphaned-agent");

        for i in 1u64..=2 {
            storage
                .store(&make_directed(
                    &format!("orph-{i}"),
                    "orphaned-agent",
                    "ws-orph",
                    i * 100,
                ))
                .await
                .unwrap();
        }

        // Bulk-ack with agent_orphaned reason
        storage
            .acknowledge_all(&agent_id, "agent_orphaned")
            .await
            .unwrap();

        let deleted = storage.expire_acked_inboxes(10_000).await.unwrap();
        assert_eq!(deleted, 2);
    }

    #[tokio::test]
    async fn list_by_workspace_excludes_directed_messages() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-directed-excl");

        // Store a workspace event (should appear)
        storage
            .store(&make_workspace_event(
                "ws-evt",
                "ws-directed-excl",
                MessageKind::AgentCreated,
                1000,
            ))
            .await
            .unwrap();
        // Store an agent-directed message in same workspace (should NOT appear)
        storage
            .store(&make_directed(
                "dir-msg",
                "some-agent",
                "ws-directed-excl",
                2000,
            ))
            .await
            .unwrap();

        let results = storage
            .list_by_workspace(&ws_id, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, Id::new("ws-evt"));
    }

    #[tokio::test]
    async fn list_after_limit_truncates() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-limit");

        for i in 1u64..=5 {
            storage
                .store(&make_directed(
                    &format!("lim-{i}"),
                    "agent-limit",
                    "ws-lim",
                    i * 100,
                ))
                .await
                .unwrap();
        }

        // Limit 2 — should return oldest 2
        let results = storage.list_after(&agent_id, 0, None, 2).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, Id::new("lim-1"));
        assert_eq!(results[1].id, Id::new("lim-2"));
    }

    #[tokio::test]
    async fn list_after_empty_inbox_returns_empty() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-empty");
        let results = storage.list_after(&agent_id, 0, None, 100).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn list_by_workspace_before_ts_only_no_before_id() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-before-ts-only");

        for i in 1u64..=4 {
            storage
                .store(&make_workspace_event(
                    &format!("bto-{i}"),
                    "ws-before-ts-only",
                    MessageKind::AgentCreated,
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        // before_ts=3000, no before_id → created_at < 3000 (strict lt)
        let results = storage
            .list_by_workspace(&ws_id, None, None, Some(3000), None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2); // bto-1 (1000) and bto-2 (2000)
    }

    #[tokio::test]
    async fn list_by_workspace_windowed_since_and_before() {
        let (_tmp, storage) = tmp_storage();
        let ws_id = Id::new("ws-windowed");

        for i in 1u64..=6 {
            storage
                .store(&make_workspace_event(
                    &format!("win-{i}"),
                    "ws-windowed",
                    MessageKind::AgentCreated,
                    i * 1000,
                ))
                .await
                .unwrap();
        }

        // since=2000 (inclusive), before_ts=5000 (exclusive) → win-2, win-3, win-4 (newest first)
        let results = storage
            .list_by_workspace(&ws_id, None, Some(2000), Some(5000), None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 3); // 2000, 3000, 4000
        assert_eq!(results[0].id, Id::new("win-4"));
        assert_eq!(results[2].id, Id::new("win-2"));
    }

    #[tokio::test]
    async fn expire_for_agents_empty_ids_returns_zero() {
        let (_tmp, storage) = tmp_storage();
        let count = storage.expire_for_agents(&[], 10_000).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn expire_acked_inboxes_preserves_recent_messages() {
        let (_tmp, storage) = tmp_storage();
        let agent_id = Id::new("agent-recent");

        // Old message — should be deleted
        storage
            .store(&make_directed("old-ack", "agent-recent", "ws-r", 100))
            .await
            .unwrap();
        // Recent message — should survive
        storage
            .store(&make_directed("new-ack", "agent-recent", "ws-r", 99_000))
            .await
            .unwrap();

        storage
            .acknowledge_all(&agent_id, "agent_completed")
            .await
            .unwrap();

        let deleted = storage.expire_acked_inboxes(50_000).await.unwrap();
        assert_eq!(deleted, 1); // only old-ack
        assert!(storage
            .find_by_id(&Id::new("new-ack"))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn expire_for_agents_removes_agent_messages() {
        let (_tmp, storage) = tmp_storage();

        for i in 1u64..=3 {
            storage
                .store(&make_directed(
                    &format!("ea-{i}"),
                    "dead-agent-2",
                    "ws-ea",
                    i * 100,
                ))
                .await
                .unwrap();
        }
        // A message for a different agent — should remain
        storage
            .store(&make_directed("ea-other", "live-agent", "ws-ea", 200))
            .await
            .unwrap();

        let deleted = storage
            .expire_for_agents(&[Id::new("dead-agent-2")], 10_000)
            .await
            .unwrap();
        assert_eq!(deleted, 3);

        assert!(storage
            .find_by_id(&Id::new("ea-other"))
            .await
            .unwrap()
            .is_some());
    }
}
