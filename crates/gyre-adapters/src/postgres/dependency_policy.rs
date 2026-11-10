//! PostgreSQL adapter for the `DependencyPolicyRepository` port (task-163).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{BreakingChangeBehavior, DependencyPolicy};
use gyre_ports::DependencyPolicyRepository;
use std::sync::Arc;

use super::PgStorage;
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
impl DependencyPolicyRepository for PgStorage {
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
