//! SQLite adapter for the `BreakingChangeRepository` port (task-163).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::BreakingChange;
use gyre_ports::BreakingChangeRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::breaking_changes;

#[derive(Queryable, Selectable)]
#[diesel(table_name = breaking_changes)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct BreakingChangeRow {
    id: String,
    dependency_edge_id: String,
    source_repo_id: String,
    commit_sha: String,
    description: String,
    detected_at: i64,
    acknowledged: i32,
    acknowledged_by: Option<String>,
    acknowledged_at: Option<i64>,
}

impl BreakingChangeRow {
    fn into_breaking_change(self) -> Result<BreakingChange> {
        let acknowledged_by = self.acknowledged_by.filter(|s| !s.is_empty());
        Ok(BreakingChange {
            id: Id::new(self.id),
            dependency_edge_id: Id::new(self.dependency_edge_id),
            source_repo_id: Id::new(self.source_repo_id),
            commit_sha: self.commit_sha,
            description: self.description,
            detected_at: self.detected_at as u64,
            acknowledged: self.acknowledged != 0,
            acknowledged_by: acknowledged_by.clone(),
            acknowledged_at: self.acknowledged_at.map(|v| v as u64),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = breaking_changes)]
struct NewBreakingChangeRow<'a> {
    id: &'a str,
    dependency_edge_id: &'a str,
    source_repo_id: &'a str,
    commit_sha: &'a str,
    description: &'a str,
    detected_at: i64,
    acknowledged: i32,
    acknowledged_by: Option<&'a str>,
    acknowledged_at: Option<i64>,
}

#[async_trait]
impl BreakingChangeRepository for SqliteStorage {
    async fn create(&self, bc: &BreakingChange) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let bc = bc.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewBreakingChangeRow {
                id: bc.id.as_str(),
                dependency_edge_id: bc.dependency_edge_id.as_str(),
                source_repo_id: bc.source_repo_id.as_str(),
                commit_sha: &bc.commit_sha,
                description: &bc.description,
                detected_at: bc.detected_at as i64,
                acknowledged: 0,
                acknowledged_by: None,
                acknowledged_at: None,
            };
            diesel::insert_into(breaking_changes::table)
                .values(&row)
                .execute(&mut *conn)
                .context("create breaking change")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<BreakingChange>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<BreakingChange>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = breaking_changes::table
                .find(id.as_str())
                .first::<BreakingChangeRow>(&mut *conn)
                .optional()
                .context("find breaking change by id")?;
            row.map(BreakingChangeRow::into_breaking_change).transpose()
        })
        .await?
    }

    async fn list_unacknowledged(&self) -> Result<Vec<BreakingChange>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<BreakingChange>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = breaking_changes::table
                .filter(breaking_changes::acknowledged.eq(0))
                .order_by(breaking_changes::detected_at.asc())
                .load::<BreakingChangeRow>(&mut *conn)
                .context("list unacknowledged breaking changes")?;
            rows.into_iter()
                .map(BreakingChangeRow::into_breaking_change)
                .collect()
        })
        .await?
    }

    async fn list_by_source_repo(&self, source_repo_id: &Id) -> Result<Vec<BreakingChange>> {
        let pool = Arc::clone(&self.pool);
        let rid = source_repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<BreakingChange>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = breaking_changes::table
                .filter(breaking_changes::source_repo_id.eq(rid.as_str()))
                .order_by(breaking_changes::detected_at.asc())
                .load::<BreakingChangeRow>(&mut *conn)
                .context("list breaking changes by source repo")?;
            rows.into_iter()
                .map(BreakingChangeRow::into_breaking_change)
                .collect()
        })
        .await?
    }

    async fn acknowledge(&self, id: &Id, acknowledged_by: &str, at: u64) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let by = acknowledged_by.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::update(breaking_changes::table.find(id.as_str()))
                .set((
                    breaking_changes::acknowledged.eq(1),
                    breaking_changes::acknowledged_by.eq(&by),
                    breaking_changes::acknowledged_at.eq(at as i64),
                ))
                .execute(&mut *conn)
                .context("acknowledge breaking change")?;
            Ok(count > 0)
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

    fn make_bc(id: &str, edge_id: &str, repo_id: &str) -> BreakingChange {
        BreakingChange::new(
            Id::new(id),
            Id::new(edge_id),
            Id::new(repo_id),
            "abc1234567890",
            "remove deprecated API",
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find_roundtrip() {
        let (_tmp, s) = setup();
        let bc = make_bc("bc-1", "edge-1", "repo-b");
        BreakingChangeRepository::create(&s, &bc).await.unwrap();
        let found = BreakingChangeRepository::find_by_id(&s, &Id::new("bc-1"))
            .await
            .unwrap()
            .expect("record should exist");
        assert_eq!(found.id, bc.id);
        assert_eq!(found.dependency_edge_id, bc.dependency_edge_id);
        assert_eq!(found.source_repo_id, bc.source_repo_id);
        assert_eq!(found.commit_sha, "abc1234567890");
        assert_eq!(found.description, "remove deprecated API");
        assert_eq!(found.detected_at, 1000);
        assert!(!found.acknowledged);
        assert_eq!(found.acknowledged_by, None);
        assert_eq!(found.acknowledged_at, None);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        let found = BreakingChangeRepository::find_by_id(&s, &Id::new("nope"))
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_unacknowledged_filters_acknowledged() {
        let (_tmp, s) = setup();
        BreakingChangeRepository::create(&s, &make_bc("bc-1", "edge-1", "repo-b"))
            .await
            .unwrap();
        BreakingChangeRepository::create(&s, &make_bc("bc-2", "edge-2", "repo-b"))
            .await
            .unwrap();
        assert_eq!(
            BreakingChangeRepository::list_unacknowledged(&s)
                .await
                .unwrap()
                .len(),
            2
        );

        let acked = BreakingChangeRepository::acknowledge(&s, &Id::new("bc-1"), "user-1", 2000)
            .await
            .unwrap();
        assert!(acked);

        let unacked = BreakingChangeRepository::list_unacknowledged(&s)
            .await
            .unwrap();
        assert_eq!(unacked.len(), 1);
        assert_eq!(unacked[0].id.as_str(), "bc-2");

        // Acknowledged record round-trips the acknowledgment fields.
        let found = BreakingChangeRepository::find_by_id(&s, &Id::new("bc-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(found.acknowledged);
        assert_eq!(found.acknowledged_by.as_deref(), Some("user-1"));
        assert_eq!(found.acknowledged_at, Some(2000));
    }

    #[tokio::test]
    async fn acknowledge_missing_returns_false() {
        let (_tmp, s) = setup();
        let acked = BreakingChangeRepository::acknowledge(&s, &Id::new("ghost"), "user-1", 2000)
            .await
            .unwrap();
        assert!(!acked);
    }

    #[tokio::test]
    async fn list_by_source_repo_scopes_correctly() {
        let (_tmp, s) = setup();
        BreakingChangeRepository::create(&s, &make_bc("bc-1", "edge-1", "repo-b"))
            .await
            .unwrap();
        BreakingChangeRepository::create(&s, &make_bc("bc-2", "edge-2", "repo-b"))
            .await
            .unwrap();
        BreakingChangeRepository::create(&s, &make_bc("bc-3", "edge-3", "repo-c"))
            .await
            .unwrap();

        let for_b = BreakingChangeRepository::list_by_source_repo(&s, &Id::new("repo-b"))
            .await
            .unwrap();
        assert_eq!(for_b.len(), 2);
        assert!(for_b.iter().all(|bc| bc.source_repo_id.as_str() == "repo-b"));

        let for_c = BreakingChangeRepository::list_by_source_repo(&s, &Id::new("repo-c"))
            .await
            .unwrap();
        assert_eq!(for_c.len(), 1);
        assert_eq!(for_c[0].id.as_str(), "bc-3");
    }

    /// Persistence proof: a NEW storage instance over the SAME database file
    /// sees the records written by the first instance. Fails against an
    /// in-memory stand-in (a fresh instance would see an empty store).
    #[tokio::test]
    async fn records_survive_fresh_storage_instance() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let s = SqliteStorage::new(&path).unwrap();
            BreakingChangeRepository::create(&s, &make_bc("bc-persist", "edge-1", "repo-b"))
                .await
                .unwrap();
        }

        let s2 = SqliteStorage::new(&path).unwrap();
        let found = BreakingChangeRepository::find_by_id(&s2, &Id::new("bc-persist"))
            .await
            .unwrap()
            .expect("record must survive a fresh storage instance");
        assert_eq!(found.commit_sha, "abc1234567890");
        assert_eq!(
            BreakingChangeRepository::list_unacknowledged(&s2)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

