use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::spec_links::{SpecLinkEntry, SpecLinkType};
use gyre_ports::SpecLinkRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_links;

/// Row mapping notes (spec-links.md §Forge-Maintained Spec Graph vs
/// `SpecLinkEntry`):
/// - `source_repo_id` is `NOT NULL DEFAULT ''` in SQL but `Option<String>` in
///   the entry: legacy same-repo links have no repo scope. `None` ↔ `''`.
/// - `target_sha` is `NOT NULL DEFAULT ''` in SQL but `Option<String>` in the
///   entry: unresolved cross-workspace links have no pinned SHA. `None` ↔ `''`.
#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_links)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecLinkRow {
    id: String,
    source_repo_id: String,
    source_path: String,
    source_sha: String,
    link_type: String,
    target_repo_id: Option<String>,
    target_path: String,
    target_sha: String,
    target_display: Option<String>,
    reason: Option<String>,
    status: String,
    created_at: i64,
    stale_since: Option<i64>,
}

impl SpecLinkRow {
    fn into_entry(self) -> Result<SpecLinkEntry> {
        let link_type_str = self.link_type.clone();
        let link_type: SpecLinkType = link_type_str
            .parse()
            .map_err(|e| anyhow::anyhow!("spec_links.link_type '{link_type_str}': {e}"))?;
        Ok(SpecLinkEntry {
            id: self.id,
            source_repo_id: opt_from_empty(self.source_repo_id),
            source_path: self.source_path,
            source_sha: self.source_sha,
            link_type,
            target_repo_id: self.target_repo_id,
            target_path: self.target_path,
            target_sha: opt_from_empty(self.target_sha),
            target_display: self.target_display,
            reason: self.reason,
            status: self.status,
            created_at: self.created_at as u64,
            stale_since: self.stale_since.map(|v| v as u64),
        })
    }
}

/// `''` in a NOT NULL column ↔ `None` in `SpecLinkEntry`.
fn opt_from_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn empty_from_opt(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("")
}

#[derive(Insertable)]
#[diesel(table_name = spec_links)]
struct NewSpecLinkRow {
    id: String,
    source_repo_id: String,
    source_path: String,
    source_sha: String,
    link_type: String,
    target_repo_id: Option<String>,
    target_path: String,
    target_sha: String,
    target_display: Option<String>,
    reason: Option<String>,
    status: String,
    created_at: i64,
    stale_since: Option<i64>,
}

fn insertable_row(e: &SpecLinkEntry) -> NewSpecLinkRow {
    NewSpecLinkRow {
        id: e.id.clone(),
        source_repo_id: e.source_repo_id.clone().unwrap_or_default(),
        source_path: e.source_path.clone(),
        source_sha: e.source_sha.clone(),
        link_type: e.link_type.to_string(),
        target_repo_id: e.target_repo_id.clone(),
        target_path: e.target_path.clone(),
        target_sha: e.target_sha.clone().unwrap_or_default(),
        target_display: e.target_display.clone(),
        reason: e.reason.clone(),
        status: e.status.clone(),
        created_at: e.created_at as i64,
        stale_since: e.stale_since.map(|v| v as i64),
    }
}


#[async_trait]
impl SpecLinkRepository for SqliteStorage {
    async fn list_all(&self) -> Result<Vec<SpecLinkEntry>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecLinkEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_links::table
                .order(spec_links::id.asc())
                .load::<SpecLinkRow>(&mut *conn)
                .context("list all spec links")?;
            rows.into_iter().map(SpecLinkRow::into_entry).collect()
        })
        .await?
    }

    async fn replace_for_source(
        &self,
        source_repo_id: &str,
        source_path: &str,
        links: &[SpecLinkEntry],
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = source_repo_id.to_string();
        let path = source_path.to_string();
        let rows: Vec<NewSpecLinkRow> = links.iter().map(insertable_row).collect();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            conn.transaction(|conn| -> Result<()> {
                diesel::delete(
                    spec_links::table.filter(
                        spec_links::source_repo_id
                            .eq(&repo_id)
                            .and(spec_links::source_path.eq(&path)),
                    ),
                )
                .execute(conn)
                .context("delete spec links for source")?;
                if !rows.is_empty() {
                    diesel::insert_into(spec_links::table)
                        .values(&rows)
                        .execute(conn)
                        .context("insert spec links for source")?;
                }
                Ok(())
            })
        })
        .await??;
        Ok(())
    }

    async fn save(&self, entry: &SpecLinkEntry) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = entry.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = insertable_row(&e);
            let update = insertable_row(&e);
            diesel::insert_into(spec_links::table)
                .values(&row)
                .on_conflict(spec_links::id)
                .do_update()
                .set((
                    spec_links::source_repo_id.eq(update.source_repo_id),
                    spec_links::source_path.eq(update.source_path),
                    spec_links::source_sha.eq(update.source_sha),
                    spec_links::link_type.eq(update.link_type),
                    spec_links::target_repo_id.eq(update.target_repo_id),
                    spec_links::target_path.eq(update.target_path),
                    spec_links::target_sha.eq(update.target_sha),
                    spec_links::target_display.eq(update.target_display),
                    spec_links::reason.eq(update.reason),
                    spec_links::status.eq(update.status),
                    spec_links::created_at.eq(update.created_at),
                    spec_links::stale_since.eq(update.stale_since),
                ))
                .execute(&mut *conn)
                .context("upsert spec link")?;
            Ok(())
        })
        .await?
    }

    async fn delete_by_source_repo(&self, source_repo_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = source_repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(spec_links::table.filter(spec_links::source_repo_id.eq(&repo_id)))
                .execute(&mut *conn)
                .context("delete spec links by source repo")?;
            Ok(())
        })
        .await?
    }
}
