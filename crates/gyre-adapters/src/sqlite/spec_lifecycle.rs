use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecLifecycleConfig;
use gyre_ports::SpecLifecycleRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_lifecycle_configs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_lifecycle_configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecLifecycleRow {
    #[allow(dead_code)]
    repo_id: String,
    config: String,
}

#[derive(Insertable)]
#[diesel(table_name = spec_lifecycle_configs)]
struct NewSpecLifecycleRow<'a> {
    repo_id: &'a str,
    config: &'a str,
}

#[async_trait]
impl SpecLifecycleRepository for SqliteStorage {
    async fn get_for_repo(&self, repo_id: &str) -> Result<SpecLifecycleConfig> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<SpecLifecycleConfig> {
            let mut conn = pool.get().context("get db connection")?;
            let row = spec_lifecycle_configs::table
                .find(&repo_id)
                .first::<SpecLifecycleRow>(&mut *conn)
                .optional()
                .context("find spec lifecycle config for repo")?;
            match row {
                None => Ok(SpecLifecycleConfig::default()),
                Some(row) => Ok(serde_json::from_str(&row.config)
                    .context("deserialize spec lifecycle config")?),
            }
        })
        .await?
    }

    async fn set_for_repo(&self, repo_id: &str, config: SpecLifecycleConfig) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        let config = serde_json::to_string(&config).context("serialize spec lifecycle config")?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecLifecycleRow {
                repo_id: &repo_id,
                config: &config,
            };
            diesel::insert_into(spec_lifecycle_configs::table)
                .values(&row)
                .on_conflict(spec_lifecycle_configs::repo_id)
                .do_update()
                .set(spec_lifecycle_configs::config.eq(&config))
                .execute(&mut *conn)
                .context("upsert spec lifecycle config")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    #[tokio::test]
    async fn get_for_repo_unconfigured_returns_defaults() {
        // Absent rows mean "defaults" — the port contract: the push hook
        // degrades to default behavior, never an error.
        let (_tmp, s) = setup();
        let cfg = s.get_for_repo("repo-none").await.unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.watched_paths, vec!["specs/system/", "specs/development/"]);
        assert_eq!(
            cfg.ignored_paths,
            vec![
                "specs/milestones/",
                "specs/prior-art/",
                "specs/personas/",
                "specs/prompts/"
            ]
        );
        assert_eq!(cfg.default_priority_new, gyre_domain::TaskPriority::Medium);
        assert_eq!(cfg.default_priority_modified, gyre_domain::TaskPriority::High);
        assert_eq!(cfg.default_priority_deleted, gyre_domain::TaskPriority::High);
    }

    #[tokio::test]
    async fn set_then_get_round_trips_custom_config() {
        let (_tmp, s) = setup();
        let cfg = SpecLifecycleConfig {
            enabled: false,
            watched_paths: vec!["docs/specs/".to_string()],
            ignored_paths: vec!["docs/specs/archive/".to_string()],
            auto_invalidate_approvals: false,
            dedup_open_tasks: false,
            default_priority_new: gyre_domain::TaskPriority::Critical,
            default_priority_modified: gyre_domain::TaskPriority::Low,
            default_priority_deleted: gyre_domain::TaskPriority::Medium,
        };
        s.set_for_repo("repo-1", cfg.clone()).await.unwrap();
        let got = s.get_for_repo("repo-1").await.unwrap();
        assert!(!got.enabled);
        assert_eq!(got.watched_paths, vec!["docs/specs/"]);
        assert_eq!(got.ignored_paths, vec!["docs/specs/archive/"]);
        assert!(!got.auto_invalidate_approvals);
        assert!(!got.dedup_open_tasks);
        assert_eq!(got.default_priority_new, gyre_domain::TaskPriority::Critical);
        assert_eq!(got.default_priority_modified, gyre_domain::TaskPriority::Low);
        assert_eq!(got.default_priority_deleted, gyre_domain::TaskPriority::Medium);
    }

    #[tokio::test]
    async fn set_twice_overwrites_not_duplicates() {
        // Upsert path: second write must replace the first row, not fail
        // (repo_id is PRIMARY KEY) and not leave a stale config behind.
        let (_tmp, s) = setup();
        s.set_for_repo(
            "repo-1",
            SpecLifecycleConfig {
                watched_paths: vec!["old/".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        s.set_for_repo(
            "repo-1",
            SpecLifecycleConfig {
                watched_paths: vec!["new/".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let got = s.get_for_repo("repo-1").await.unwrap();
        assert_eq!(got.watched_paths, vec!["new/"]);
    }

    #[tokio::test]
    async fn configs_are_per_repo() {
        // Distinct repos must not see each other's config.
        let (_tmp, s) = setup();
        s.set_for_repo(
            "repo-a",
            SpecLifecycleConfig {
                enabled: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let b = s.get_for_repo("repo-b").await.unwrap();
        assert!(b.enabled, "repo-b must see defaults, not repo-a's config");
    }
}
