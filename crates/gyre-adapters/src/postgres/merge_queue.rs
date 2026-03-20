use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};
use gyre_ports::MergeQueueRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::merge_queue;

fn status_to_str(s: &MergeQueueEntryStatus) -> &'static str {
    match s {
        MergeQueueEntryStatus::Queued => "Queued",
        MergeQueueEntryStatus::Processing => "Processing",
        MergeQueueEntryStatus::Merged => "Merged",
        MergeQueueEntryStatus::Failed => "Failed",
        MergeQueueEntryStatus::Cancelled => "Cancelled",
    }
}

fn str_to_status(s: &str) -> Result<MergeQueueEntryStatus> {
    match s {
        "Queued" => Ok(MergeQueueEntryStatus::Queued),
        "Processing" => Ok(MergeQueueEntryStatus::Processing),
        "Merged" => Ok(MergeQueueEntryStatus::Merged),
        "Failed" => Ok(MergeQueueEntryStatus::Failed),
        "Cancelled" => Ok(MergeQueueEntryStatus::Cancelled),
        other => Err(anyhow!("unknown merge queue status: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = merge_queue)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MergeQueueRow {
    id: String,
    merge_request_id: String,
    priority: i32,
    status: String,
    enqueued_at: i64,
    processed_at: Option<i64>,
    error_message: Option<String>,
}

impl MergeQueueRow {
    fn into_entry(self) -> Result<MergeQueueEntry> {
        Ok(MergeQueueEntry {
            id: Id::new(self.id),
            merge_request_id: Id::new(self.merge_request_id),
            priority: self.priority as u32,
            status: str_to_status(&self.status)?,
            enqueued_at: self.enqueued_at as u64,
            processed_at: self.processed_at.map(|v| v as u64),
            error_message: self.error_message,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = merge_queue)]
struct MergeQueueRecord<'a> {
    id: &'a str,
    merge_request_id: &'a str,
    priority: i32,
    status: &'a str,
    enqueued_at: i64,
    processed_at: Option<i64>,
    error_message: Option<&'a str>,
}

#[async_trait]
impl MergeQueueRepository for PgStorage {
    async fn enqueue(&self, entry: &MergeQueueEntry) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = entry.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = MergeQueueRecord {
                id: e.id.as_str(),
                merge_request_id: e.merge_request_id.as_str(),
                priority: e.priority as i32,
                status: status_to_str(&e.status),
                enqueued_at: e.enqueued_at as i64,
                processed_at: e.processed_at.map(|v| v as i64),
                error_message: e.error_message.as_deref(),
            };
            diesel::insert_into(merge_queue::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert merge_queue entry")?;
            Ok(())
        })
        .await?
    }

    async fn next_pending(&self) -> Result<Option<MergeQueueEntry>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Option<MergeQueueEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = merge_queue::table
                .filter(merge_queue::status.eq("Queued"))
                .order((merge_queue::priority.desc(), merge_queue::enqueued_at.asc()))
                .limit(1)
                .first::<MergeQueueRow>(&mut *conn)
                .optional()
                .context("next pending merge queue entry")?;
            result.map(|r| r.into_entry()).transpose()
        })
        .await?
    }

    async fn update_status(
        &self,
        id: &Id,
        status: MergeQueueEntryStatus,
        error: Option<String>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let is_terminal = matches!(
            status,
            MergeQueueEntryStatus::Merged
                | MergeQueueEntryStatus::Failed
                | MergeQueueEntryStatus::Cancelled
        );
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let status_str = status_to_str(&status);
            if is_terminal {
                diesel::update(merge_queue::table.find(id.as_str()))
                    .set((
                        merge_queue::status.eq(status_str),
                        merge_queue::error_message.eq(error.as_deref()),
                        merge_queue::processed_at.eq(Some(now as i64)),
                    ))
                    .execute(&mut *conn)
                    .context("update merge_queue status (terminal)")?;
            } else {
                diesel::update(merge_queue::table.find(id.as_str()))
                    .set((
                        merge_queue::status.eq(status_str),
                        merge_queue::error_message.eq(error.as_deref()),
                    ))
                    .execute(&mut *conn)
                    .context("update merge_queue status")?;
            }
            Ok(())
        })
        .await?
    }

    async fn list_queue(&self) -> Result<Vec<MergeQueueEntry>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeQueueEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let terminal = ["Merged", "Failed", "Cancelled"];
            let rows = merge_queue::table
                .filter(diesel::dsl::not(merge_queue::status.eq_any(terminal)))
                .order((merge_queue::priority.desc(), merge_queue::enqueued_at.asc()))
                .load::<MergeQueueRow>(&mut *conn)
                .context("list merge_queue")?;
            rows.into_iter().map(|r| r.into_entry()).collect()
        })
        .await?
    }

    async fn cancel(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                merge_queue::table
                    .filter(merge_queue::id.eq(id.as_str()))
                    .filter(merge_queue::status.eq_any(["Queued", "Processing"])),
            )
            .set((
                merge_queue::status.eq("Cancelled"),
                merge_queue::processed_at.eq(Some(now as i64)),
            ))
            .execute(&mut *conn)
            .context("cancel merge_queue entry")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeQueueEntry>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<MergeQueueEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = merge_queue::table
                .find(id.as_str())
                .first::<MergeQueueRow>(&mut *conn)
                .optional()
                .context("find merge_queue entry by id")?;
            result.map(|r| r.into_entry()).transpose()
        })
        .await?
    }
}
