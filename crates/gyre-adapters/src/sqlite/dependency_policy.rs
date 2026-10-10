//! SQLite adapter for the `DependencyPolicyRepository` port (task-163).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{BreakingChangeBehavior, DependencyPolicy};
use gyre_ports::DependencyPolicyRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::dependency_policies;

fn behavior_to_str(b: &BreakingChangeBehavior) -> &'static str {
    match b {
        BreakingChangeBehavior::Block => "block",
        BreakingChangeBehavior::Warn => "warn",
        BreakingChangeBehavior::Notify => "notify",
    }
}

fn str_to_behavior(s: &str) -> Result<BreakingChangeBehavior> {
    match s {
        "block" => Ok(BreakingChangeBehavior::Block),
        "warn" => Ok(BreakingChangeBehavior::Warn),
        "notify" => Ok(BreakingChangeBehavior::Notify),
        other => Err(anyhow::anyhow!("unknown breaking change behavior: {other}")),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = dependency_policies)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct DependencyPolicyRow {
    #[allow(dead_code)]
    workspace_id: String,
    breaking_change_behavior: String,
    max_version_drift: i32,
    stale_dependency_alert_days: i32,
    require_cascade_tests: i32,
    auto_create_update_tasks: i32,
    #[allow(dead_code)]
    updated_at: i64,
}

impl DependencyPolicyRow {
    fn into_policy(self) -> Result<DependencyPolicy> {
        Ok(DependencyPolicy {
            breaking_change_behavior: str_to_behavior(&self.breaking_change_behavior)?,
            max_version_drift: self.max_version_drift.max(0) as u32,
            stale_dependency_alert_days: self.stale_dependency_alert_days.max(0) as u32,
            require_cascade_tests: self.require_cascade_tests != 0,
            auto_create_update_tasks: self.auto_create_update_tasks != 0,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = dependency_policies)]
struct NewDependencyPolicyRow<'a> {
    workspace_id: &'a str,
    breaking_change_behavior: &'a str,
    max_version_drift: i32,
    stale_dependency_alert_days: i32,
    require_cascade_tests: i32,
    auto_create_update_tasks: i32,
    updated_at: i64,
}

#[async_trait]
impl DependencyPolicyRepository for SqliteStorage {
    async fn get_for_workspace(&self, workspace_id: &Id) -> Result<DependencyPolicy> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<DependencyPolicy> {
            let mut conn = pool.get().context("get db connection")?;
            let row = dependency_policies::table
                .find(wid.as_str())
                .first::<DependencyPolicyRow>(&mut *conn)
                .optional()
                .context("get dependency policy")?;
            match row {
                Some(r) => r.into_policy(),
                None => Ok(DependencyPolicy::default()),
            }
        })
        .await?
    }

    async fn set_for_workspace(&self, workspace_id: &Id, policy: &DependencyPolicy) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        let p = policy.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewDependencyPolicyRow {
                workspace_id: wid.as_str(),
                breaking_change_behavior: behavior_to_str(&p.breaking_change_behavior),
                max_version_drift: p.max_version_drift as i32,
                stale_dependency_alert_days: p.stale_dependency_alert_days as i32,
                require_cascade_tests: if p.require_cascade_tests { 1 } else { 0 },
                auto_create_update_tasks: if p.auto_create_update_tasks { 1 } else { 0 },
                updated_at: now_secs(),
            };
            diesel::insert_into(dependency_policies::table)
                .values(&row)
                .on_conflict(dependency_policies::workspace_id)
                .do_update()
                .set((
                    dependency_policies::breaking_change_behavior.eq(row.breaking_change_behavior),
                    dependency_policies::max_version_drift.eq(row.max_version_drift),
                    dependency_policies::stale_dependency_alert_days
                        .eq(row.stale_dependency_alert_days),
                    dependency_policies::require_cascade_tests.eq(row.require_cascade_tests),
                    dependency_policies::auto_create_update_tasks.eq(row.auto_create_update_tasks),
                    dependency_policies::updated_at.eq(row.updated_at),
                ))
                .execute(&mut *conn)
                .context("set dependency policy")?;
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

    #[tokio::test]
    async fn unset_workspace_returns_default_policy() {
        let (_tmp, s) = setup();
        let policy = DependencyPolicyRepository::get_for_workspace(&s, &Id::new("ws-none"))
            .await
            .unwrap();
        assert_eq!(
            policy.breaking_change_behavior,
            BreakingChangeBehavior::Warn
        );
        assert_eq!(policy.max_version_drift, 3);
        assert_eq!(policy.stale_dependency_alert_days, 30);
        assert!(policy.require_cascade_tests);
        assert!(policy.auto_create_update_tasks);
    }

    #[tokio::test]
    async fn set_and_get_roundtrip_all_fields() {
        let (_tmp, s) = setup();
        let policy = DependencyPolicy {
            breaking_change_behavior: BreakingChangeBehavior::Block,
            max_version_drift: 7,
            stale_dependency_alert_days: 90,
            require_cascade_tests: false,
            auto_create_update_tasks: false,
        };
        DependencyPolicyRepository::set_for_workspace(&s, &Id::new("ws-1"), &policy)
            .await
            .unwrap();

        let found = DependencyPolicyRepository::get_for_workspace(&s, &Id::new("ws-1"))
            .await
            .unwrap();
        assert_eq!(
            found.breaking_change_behavior,
            BreakingChangeBehavior::Block
        );
        assert_eq!(found.max_version_drift, 7);
        assert_eq!(found.stale_dependency_alert_days, 90);
        assert!(!found.require_cascade_tests);
        assert!(!found.auto_create_update_tasks);

        // Other workspaces are unaffected.
        let other = DependencyPolicyRepository::get_for_workspace(&s, &Id::new("ws-2"))
            .await
            .unwrap();
        assert_eq!(other.breaking_change_behavior, BreakingChangeBehavior::Warn);
    }

    #[tokio::test]
    async fn set_overwrites_previous_policy() {
        let (_tmp, s) = setup();
        let ws = Id::new("ws-overwrite");
        DependencyPolicyRepository::set_for_workspace(
            &s,
            &ws,
            &DependencyPolicy {
                breaking_change_behavior: BreakingChangeBehavior::Notify,
                max_version_drift: 1,
                stale_dependency_alert_days: 2,
                require_cascade_tests: false,
                auto_create_update_tasks: false,
            },
        )
        .await
        .unwrap();

        DependencyPolicyRepository::set_for_workspace(
            &s,
            &ws,
            &DependencyPolicy {
                breaking_change_behavior: BreakingChangeBehavior::Block,
                max_version_drift: 5,
                stale_dependency_alert_days: 60,
                require_cascade_tests: true,
                auto_create_update_tasks: true,
            },
        )
        .await
        .unwrap();

        let found = DependencyPolicyRepository::get_for_workspace(&s, &ws)
            .await
            .unwrap();
        assert_eq!(
            found.breaking_change_behavior,
            BreakingChangeBehavior::Block
        );
        assert_eq!(found.max_version_drift, 5);
        assert_eq!(found.stale_dependency_alert_days, 60);
        assert!(found.require_cascade_tests);
        assert!(found.auto_create_update_tasks);
    }

    /// Persistence proof: a fresh storage instance over the same DB file sees
    /// the policy written by the first instance.
    #[tokio::test]
    async fn policy_survives_fresh_storage_instance() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let s = SqliteStorage::new(&path).unwrap();
            DependencyPolicyRepository::set_for_workspace(
                &s,
                &Id::new("ws-persist"),
                &DependencyPolicy {
                    breaking_change_behavior: BreakingChangeBehavior::Block,
                    max_version_drift: 9,
                    stale_dependency_alert_days: 15,
                    require_cascade_tests: true,
                    auto_create_update_tasks: false,
                },
            )
            .await
            .unwrap();
        }

        let s2 = SqliteStorage::new(&path).unwrap();
        let found = DependencyPolicyRepository::get_for_workspace(&s2, &Id::new("ws-persist"))
            .await
            .unwrap();
        assert_eq!(
            found.breaking_change_behavior,
            BreakingChangeBehavior::Block
        );
        assert_eq!(found.max_version_drift, 9);
        assert_eq!(found.stale_dependency_alert_days, 15);
        assert!(found.require_cascade_tests);
        assert!(!found.auto_create_update_tasks);
    }
}
