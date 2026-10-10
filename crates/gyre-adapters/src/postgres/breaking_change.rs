//! PostgreSQL adapter for the `BreakingChangeRepository` port (task-163).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::BreakingChange;
use gyre_ports::BreakingChangeRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::breaking_changes;

#[derive(Queryable, Selectable)]
#[diesel(table_name = breaking_changes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl BreakingChangeRepository for PgStorage {
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
                acknowledged: if bc.acknowledged { 1 } else { 0 },
                acknowledged_by: bc.acknowledged_by.as_deref(),
                acknowledged_at: bc.acknowledged_at.map(|v| v as i64),
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
