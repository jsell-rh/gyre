use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};
use gyre_ports::MergeQueueRepository;

use super::{open_conn, SqliteStorage};

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

fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<MergeQueueEntry> {
    let status_str: String = row.get(3)?;
    Ok(MergeQueueEntry {
        id: Id::new(row.get::<_, String>(0)?),
        merge_request_id: Id::new(row.get::<_, String>(1)?),
        priority: row.get::<_, i64>(2)? as u32,
        status: str_to_status(&status_str)?,
        enqueued_at: row.get::<_, i64>(4)? as u64,
        processed_at: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        error_message: row.get(6)?,
    })
}

#[async_trait]
impl MergeQueueRepository for SqliteStorage {
    async fn enqueue(&self, entry: &MergeQueueEntry) -> Result<()> {
        let path = self.db_path();
        let e = entry.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO merge_queue (id, merge_request_id, priority, status, enqueued_at, processed_at, error_message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    e.id.as_str(),
                    e.merge_request_id.as_str(),
                    e.priority as i64,
                    status_to_str(&e.status),
                    e.enqueued_at as i64,
                    e.processed_at.map(|v| v as i64),
                    e.error_message,
                ],
            )
            .context("insert merge_queue entry")?;
            Ok(())
        })
        .await?
    }

    async fn next_pending(&self) -> Result<Option<MergeQueueEntry>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Option<MergeQueueEntry>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message
                 FROM merge_queue
                 WHERE status = 'Queued'
                 ORDER BY priority DESC, enqueued_at ASC
                 LIMIT 1",
            )?;
            let mut rows = stmt.query([])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_entry(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn update_status(
        &self,
        id: &Id,
        status: MergeQueueEntryStatus,
        error: Option<String>,
    ) -> Result<()> {
        let path = self.db_path();
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
            let conn = open_conn(&path)?;
            let processed_at: Option<i64> = if is_terminal { Some(now as i64) } else { None };
            conn.execute(
                "UPDATE merge_queue SET status=?1, error_message=?2, processed_at=COALESCE(?3, processed_at) WHERE id=?4",
                rusqlite::params![
                    status_to_str(&status),
                    error,
                    processed_at,
                    id.as_str(),
                ],
            )
            .context("update merge_queue status")?;
            Ok(())
        })
        .await?
    }

    async fn list_queue(&self) -> Result<Vec<MergeQueueEntry>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeQueueEntry>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message
                 FROM merge_queue
                 WHERE status NOT IN ('Merged', 'Failed', 'Cancelled')
                 ORDER BY priority DESC, enqueued_at ASC",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_entry(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn cancel(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE merge_queue SET status='Cancelled', processed_at=?1 WHERE id=?2 AND status IN ('Queued', 'Processing')",
                rusqlite::params![now as i64, id.as_str()],
            )
            .context("cancel merge_queue entry")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeQueueEntry>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<MergeQueueEntry>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message
                 FROM merge_queue WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_entry(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::{MergeRequest, Project, Repository};
    use gyre_ports::{MergeRequestRepository, ProjectRepository, RepoRepository};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    async fn setup_mr(s: &SqliteStorage, mr_id: &str) {
        let p = Project::new(Id::new("p1"), "proj".to_string(), 1000);
        let _ = ProjectRepository::create(s, &p).await;
        let r = Repository::new(
            Id::new("r1"),
            Id::new("p1"),
            "repo".to_string(),
            "/repos/r1".to_string(),
            1000,
        );
        let _ = RepoRepository::create(s, &r).await;
        let mr = MergeRequest::new(
            Id::new(mr_id),
            Id::new("r1"),
            "MR title",
            "feat/x",
            "main",
            1000,
        );
        MergeRequestRepository::create(s, &mr).await.unwrap();
    }

    fn make_entry(id: &str, mr_id: &str, priority: u32) -> MergeQueueEntry {
        MergeQueueEntry::new(Id::new(id), Id::new(mr_id), priority, 1000)
    }

    #[tokio::test]
    async fn enqueue_and_find() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let entry = make_entry("e1", "mr1", 50);
        MergeQueueRepository::enqueue(&s, &entry).await.unwrap();
        let found = MergeQueueRepository::find_by_id(&s, &Id::new("e1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.merge_request_id, entry.merge_request_id);
        assert_eq!(found.status, MergeQueueEntryStatus::Queued);
        assert_eq!(found.priority, 50);
    }

    #[tokio::test]
    async fn next_pending_returns_highest_priority() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let mr2 = MergeRequest::new(Id::new("mr2"), Id::new("r1"), "MR2", "feat/y", "main", 1000);
        MergeRequestRepository::create(&s, &mr2).await.unwrap();

        let low = make_entry("e-low", "mr1", 25);
        let high = make_entry("e-high", "mr2", 100);
        MergeQueueRepository::enqueue(&s, &low).await.unwrap();
        MergeQueueRepository::enqueue(&s, &high).await.unwrap();

        let next = MergeQueueRepository::next_pending(&s)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.id, Id::new("e-high"));
    }

    #[tokio::test]
    async fn next_pending_same_priority_oldest_first() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let mr2 = MergeRequest::new(Id::new("mr2"), Id::new("r1"), "MR2", "feat/y", "main", 1000);
        MergeRequestRepository::create(&s, &mr2).await.unwrap();

        let old = MergeQueueEntry::new(Id::new("e-old"), Id::new("mr1"), 50, 1000);
        let new = MergeQueueEntry::new(Id::new("e-new"), Id::new("mr2"), 50, 2000);
        MergeQueueRepository::enqueue(&s, &old).await.unwrap();
        MergeQueueRepository::enqueue(&s, &new).await.unwrap();

        let next = MergeQueueRepository::next_pending(&s)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.id, Id::new("e-old"));
    }

    #[tokio::test]
    async fn update_status_to_processing() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let entry = make_entry("e1", "mr1", 50);
        MergeQueueRepository::enqueue(&s, &entry).await.unwrap();

        MergeQueueRepository::update_status(
            &s,
            &Id::new("e1"),
            MergeQueueEntryStatus::Processing,
            None,
        )
        .await
        .unwrap();

        let found = MergeQueueRepository::find_by_id(&s, &Id::new("e1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, MergeQueueEntryStatus::Processing);
    }

    #[tokio::test]
    async fn update_status_failed_records_error() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let entry = make_entry("e1", "mr1", 50);
        MergeQueueRepository::enqueue(&s, &entry).await.unwrap();

        MergeQueueRepository::update_status(
            &s,
            &Id::new("e1"),
            MergeQueueEntryStatus::Failed,
            Some("merge conflict".to_string()),
        )
        .await
        .unwrap();

        let found = MergeQueueRepository::find_by_id(&s, &Id::new("e1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, MergeQueueEntryStatus::Failed);
        assert_eq!(found.error_message.as_deref(), Some("merge conflict"));
        assert!(found.processed_at.is_some());
    }

    #[tokio::test]
    async fn list_queue_excludes_terminal() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let mr2 = MergeRequest::new(Id::new("mr2"), Id::new("r1"), "MR2", "feat/y", "main", 1000);
        let mr3 = MergeRequest::new(Id::new("mr3"), Id::new("r1"), "MR3", "feat/z", "main", 1000);
        MergeRequestRepository::create(&s, &mr2).await.unwrap();
        MergeRequestRepository::create(&s, &mr3).await.unwrap();

        MergeQueueRepository::enqueue(&s, &make_entry("e1", "mr1", 50))
            .await
            .unwrap();
        MergeQueueRepository::enqueue(&s, &make_entry("e2", "mr2", 75))
            .await
            .unwrap();
        MergeQueueRepository::enqueue(&s, &make_entry("e3", "mr3", 100))
            .await
            .unwrap();

        // Mark e3 as Merged (terminal)
        MergeQueueRepository::update_status(
            &s,
            &Id::new("e3"),
            MergeQueueEntryStatus::Merged,
            None,
        )
        .await
        .unwrap();

        let queue = MergeQueueRepository::list_queue(&s).await.unwrap();
        assert_eq!(queue.len(), 2);
        // Ordered by priority desc: e2 (75) before e1 (50)
        assert_eq!(queue[0].id, Id::new("e2"));
        assert_eq!(queue[1].id, Id::new("e1"));
    }

    #[tokio::test]
    async fn cancel_queued_entry() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;
        let entry = make_entry("e1", "mr1", 50);
        MergeQueueRepository::enqueue(&s, &entry).await.unwrap();

        MergeQueueRepository::cancel(&s, &Id::new("e1"))
            .await
            .unwrap();

        let found = MergeQueueRepository::find_by_id(&s, &Id::new("e1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, MergeQueueEntryStatus::Cancelled);

        // Should not appear in list_queue
        let queue = MergeQueueRepository::list_queue(&s).await.unwrap();
        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn next_pending_skips_processing() {
        let (_tmp, s) = setup();
        setup_mr(&s, "mr1").await;

        let entry = make_entry("e1", "mr1", 50);
        MergeQueueRepository::enqueue(&s, &entry).await.unwrap();
        MergeQueueRepository::update_status(
            &s,
            &Id::new("e1"),
            MergeQueueEntryStatus::Processing,
            None,
        )
        .await
        .unwrap();

        // No Queued entries remain
        let next = MergeQueueRepository::next_pending(&s).await.unwrap();
        assert!(next.is_none());
    }
}
