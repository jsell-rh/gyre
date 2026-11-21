//! SQLite adapter for the `CascadeTestResultRepository` port (task-280).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::CascadeTestResult;
use gyre_ports::CascadeTestResultRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::cascade_test_results;

#[derive(Queryable, Selectable)]
#[diesel(table_name = cascade_test_results)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct CascadeTestResultRow {
    id: String,
    dependent_repo_id: String,
    merged_repo_id: String,
    merge_commit_sha: String,
    cascade_task_id: String,
    passed: bool,
    failure_details: Option<String>,
    recorded_at: i64,
}

impl CascadeTestResultRow {
    fn into_record(self) -> CascadeTestResult {
        CascadeTestResult {
            id: Id::new(self.id),
            dependent_repo_id: Id::new(self.dependent_repo_id),
            merged_repo_id: Id::new(self.merged_repo_id),
            merge_commit_sha: self.merge_commit_sha,
            cascade_task_id: Id::new(self.cascade_task_id),
            passed: self.passed,
            failure_details: self.failure_details,
            recorded_at: self.recorded_at.max(0) as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = cascade_test_results)]
struct NewCascadeTestResultRow<'a> {
    id: &'a str,
    dependent_repo_id: &'a str,
    merged_repo_id: &'a str,
    merge_commit_sha: &'a str,
    cascade_task_id: &'a str,
    passed: bool,
    failure_details: Option<&'a str>,
    recorded_at: i64,
}

/// Upsert keyed on `cascade_task_id` (port contract: one result row per
/// cascade test task — re-reporting replaces the row, never duplicates).
/// Shared by the SQLite and PostgreSQL adapters so both enforce the
/// identical replace semantics via `ON CONFLICT (cascade_task_id)
/// DO UPDATE`.
#[async_trait]
impl CascadeTestResultRepository for SqliteStorage {
    async fn record(&self, result: &CascadeTestResult) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = result.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewCascadeTestResultRow {
                id: r.id.as_str(),
                dependent_repo_id: r.dependent_repo_id.as_str(),
                merged_repo_id: r.merged_repo_id.as_str(),
                merge_commit_sha: &r.merge_commit_sha,
                cascade_task_id: r.cascade_task_id.as_str(),
                passed: r.passed,
                failure_details: r.failure_details.as_deref(),
                recorded_at: r.recorded_at as i64,
            };
            diesel::insert_into(cascade_test_results::table)
                .values(&row)
                .on_conflict(cascade_test_results::cascade_task_id)
                .do_update()
                .set((
                    cascade_test_results::dependent_repo_id.eq(row.dependent_repo_id.to_string()),
                    cascade_test_results::merged_repo_id.eq(row.merged_repo_id.to_string()),
                    cascade_test_results::merge_commit_sha.eq(row.merge_commit_sha.to_string()),
                    cascade_test_results::passed.eq(row.passed),
                    cascade_test_results::failure_details
                        .eq(row.failure_details.map(|d| d.to_string())),
                    cascade_test_results::recorded_at.eq(row.recorded_at),
                ))
                .execute(&mut *conn)
                .context("record cascade test result")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_task(&self, task_id: &Id) -> Result<Option<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let tid = task_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = cascade_test_results::table
                .filter(cascade_test_results::cascade_task_id.eq(tid))
                .first::<CascadeTestResultRow>(&mut *conn)
                .optional()
                .context("find cascade test result by task")?;
            Ok(row.map(|r| r.into_record()))
        })
        .await?
    }

    async fn list_by_dependent_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = cascade_test_results::table
                .filter(cascade_test_results::dependent_repo_id.eq(rid))
                .order_by(cascade_test_results::recorded_at.desc())
                .load::<CascadeTestResultRow>(&mut *conn)
                .context("list cascade test results by dependent repo")?;
            Ok(rows.into_iter().map(|r| r.into_record()).collect())
        })
        .await?
    }

    async fn list_by_merged_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = cascade_test_results::table
                .filter(cascade_test_results::merged_repo_id.eq(rid))
                .order_by(cascade_test_results::recorded_at.desc())
                .load::<CascadeTestResultRow>(&mut *conn)
                .context("list cascade test results by merged repo")?;
            Ok(rows.into_iter().map(|r| r.into_record()).collect())
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

    fn sample(task_id: &str, passed: bool) -> CascadeTestResult {
        CascadeTestResult {
            id: Id::new("ctr-1"),
            dependent_repo_id: Id::new("repo-a"),
            merged_repo_id: Id::new("repo-b"),
            merge_commit_sha: "abc123def456".to_string(),
            cascade_task_id: Id::new(task_id),
            passed,
            failure_details: if passed {
                None
            } else {
                Some("2 tests failed: test_foo, test_bar".to_string())
            },
            recorded_at: 1000,
        }
    }

    #[tokio::test]
    async fn record_and_find_by_task_roundtrip() {
        let (_tmp, s) = setup();
        CascadeTestResultRepository::record(&s, &sample("task-1", true))
            .await
            .unwrap();

        let found = CascadeTestResultRepository::find_by_task(&s, &Id::new("task-1"))
            .await
            .unwrap()
            .expect("result recorded");
        assert_eq!(found.dependent_repo_id, Id::new("repo-a"));
        assert_eq!(found.merged_repo_id, Id::new("repo-b"));
        assert_eq!(found.merge_commit_sha, "abc123def456");
        assert_eq!(found.cascade_task_id, Id::new("task-1"));
        assert!(found.passed);
        assert!(found.failure_details.is_none());
        assert_eq!(found.recorded_at, 1000);

        // Unknown task → None.
        assert!(
            CascadeTestResultRepository::find_by_task(&s, &Id::new("task-none"))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn failure_details_persisted() {
        let (_tmp, s) = setup();
        CascadeTestResultRepository::record(&s, &sample("task-f", false))
            .await
            .unwrap();

        let found = CascadeTestResultRepository::find_by_task(&s, &Id::new("task-f"))
            .await
            .unwrap()
            .expect("result recorded");
        assert!(!found.passed);
        assert_eq!(
            found.failure_details.as_deref(),
            Some("2 tests failed: test_foo, test_bar")
        );
    }

    /// Port contract: one result row per cascade task — re-reporting the
    /// same task REPLACES the row (last report wins), never duplicates.
    #[tokio::test]
    async fn record_replaces_row_for_same_task() {
        let (_tmp, s) = setup();
        CascadeTestResultRepository::record(&s, &sample("task-1", false))
            .await
            .unwrap();
        // Re-report: the agent first failed, then a retry passed.
        let mut retry = sample("task-1", true);
        retry.id = Id::new("ctr-2");
        retry.recorded_at = 2000;
        CascadeTestResultRepository::record(&s, &retry)
            .await
            .unwrap();

        let found = CascadeTestResultRepository::find_by_task(&s, &Id::new("task-1"))
            .await
            .unwrap()
            .expect("result recorded");
        assert!(found.passed, "latest report wins");
        assert_eq!(found.recorded_at, 2000);
        // Exactly one row for the task (no duplicates).
        let all = CascadeTestResultRepository::list_by_dependent_repo(&s, &Id::new("repo-a"))
            .await
            .unwrap();
        assert_eq!(
            all.iter()
                .filter(|r| r.cascade_task_id == Id::new("task-1"))
                .count(),
            1,
            "one row per cascade task"
        );
    }

    #[tokio::test]
    async fn list_by_dependent_and_merged_repo() {
        let (_tmp, s) = setup();
        let mut first = sample("task-1", true);
        first.recorded_at = 1000;
        let mut second = sample("task-2", false);
        second.id = Id::new("ctr-2");
        second.recorded_at = 2000;
        CascadeTestResultRepository::record(&s, &first)
            .await
            .unwrap();
        CascadeTestResultRepository::record(&s, &second)
            .await
            .unwrap();

        // Dependent repo: newest first.
        let deps = CascadeTestResultRepository::list_by_dependent_repo(&s, &Id::new("repo-a"))
            .await
            .unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].cascade_task_id, Id::new("task-2"));
        assert_eq!(deps[1].cascade_task_id, Id::new("task-1"));

        // Merged repo sees both cascade runs it triggered.
        let merged = CascadeTestResultRepository::list_by_merged_repo(&s, &Id::new("repo-b"))
            .await
            .unwrap();
        assert_eq!(merged.len(), 2);

        // Unrelated repo sees none.
        assert!(
            CascadeTestResultRepository::list_by_merged_repo(&s, &Id::new("repo-x"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// Persistence proof (task-280 acceptance): a result written by one
    /// storage instance is retrievable by a FRESH instance over the same
    /// DB file — the record survives what a server restart is.
    #[tokio::test]
    async fn result_survives_fresh_storage_instance() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let s = SqliteStorage::new(&path).unwrap();
            CascadeTestResultRepository::record(&s, &sample("task-persist", true))
                .await
                .unwrap();
        }

        let s2 = SqliteStorage::new(&path).unwrap();
        let found = CascadeTestResultRepository::find_by_task(&s2, &Id::new("task-persist"))
            .await
            .unwrap()
            .expect("result survives restart");
        assert!(found.passed);
        assert_eq!(found.merge_commit_sha, "abc123def456");
    }
}
