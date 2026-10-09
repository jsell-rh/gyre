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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    fn entry(id: &str, link_type: SpecLinkType) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: format!("system/source-{id}.md"),
            source_repo_id: Some("repo-1".to_string()),
            source_sha: format!("sha-{id}"),
            link_type,
            target_path: "system/target.md".to_string(),
            target_repo_id: Some("repo-2".to_string()),
            target_display: Some("@ws/repo-2/system/target.md".to_string()),
            target_sha: Some("target-sha".to_string()),
            reason: Some("why".to_string()),
            status: "active".to_string(),
            created_at: 1_700_000_000,
            stale_since: None,
        }
    }

    /// Every SpecLinkType variant and every Option column must round-trip
    /// losslessly through the SQLite table (task-198 AC).
    #[tokio::test]
    async fn round_trips_every_link_type_and_option_columns() {
        let (_tmp, storage) = tmp_storage();
        let variants = [
            SpecLinkType::Implements,
            SpecLinkType::Supersedes,
            SpecLinkType::DependsOn,
            SpecLinkType::ConflictsWith,
            SpecLinkType::Extends,
            SpecLinkType::References,
        ];
        let mut expected: Vec<SpecLinkEntry> = Vec::new();
        for (i, lt) in variants.iter().enumerate() {
            let e = entry(&format!("link-{i}"), lt.clone());
            storage.save(&e).await.unwrap();
            expected.push(e);
        }
        // Option columns exercised in their None forms too.
        let mut none_opts = entry("link-none", SpecLinkType::DependsOn);
        none_opts.source_repo_id = None;
        none_opts.target_repo_id = None;
        none_opts.target_display = None;
        none_opts.target_sha = None;
        none_opts.reason = None;
        none_opts.stale_since = Some(1_700_000_500);
        storage.save(&none_opts).await.unwrap();
        expected.push(none_opts);

        let all = storage.list_all().await.unwrap();
        assert_eq!(all.len(), expected.len());
        for exp in &expected {
            let got = all.iter().find(|l| l.id == exp.id).expect("link present");
            assert_eq!(got.source_path, exp.source_path);
            assert_eq!(got.source_repo_id, exp.source_repo_id);
            assert_eq!(got.source_sha, exp.source_sha, "source_sha must round-trip");
            assert_eq!(got.link_type, exp.link_type);
            assert_eq!(got.target_path, exp.target_path);
            assert_eq!(got.target_repo_id, exp.target_repo_id);
            assert_eq!(got.target_display, exp.target_display);
            assert_eq!(got.target_sha, exp.target_sha);
            assert_eq!(got.reason, exp.reason);
            assert_eq!(got.status, exp.status);
            assert_eq!(got.created_at, exp.created_at);
            assert_eq!(got.stale_since, exp.stale_since);
        }
    }

    /// Rows must survive a fresh `SqliteStorage` handle on the same file —
    /// the restart-durability contract (task-198 AC). Fails against an
    /// empty-init in-memory graph.
    #[tokio::test]
    async fn links_survive_reopened_database() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let e = entry("durable-1", SpecLinkType::Implements);
        {
            let storage = SqliteStorage::new(&path).unwrap();
            storage.save(&e).await.unwrap();
        }
        // Simulated restart: brand-new storage handle over the same file.
        let reopened = SqliteStorage::new(&path).unwrap();
        let all = reopened.list_all().await.unwrap();
        assert_eq!(all.len(), 1, "graph must not be empty after reopen");
        let got = &all[0];
        assert_eq!(got.id, e.id);
        assert_eq!(got.source_sha, e.source_sha);
        assert_eq!(got.link_type, e.link_type);
    }

    /// replace_for_source must atomically swap a source spec's link set.
    #[tokio::test]
    async fn replace_for_source_swaps_link_set() {
        let (_tmp, storage) = tmp_storage();
        let old = entry("old-1", SpecLinkType::Implements);
        storage.save(&old).await.unwrap();
        // An unrelated link that must NOT be touched.
        let other = entry("other-1", SpecLinkType::Extends);
        storage.save(&other).await.unwrap();

        let replacement = entry("new-1", SpecLinkType::DependsOn);
        storage
            .replace_for_source(
                // Keyed on (source_repo_id, source_path): the replacement
                // must claim the same source spec as the row it swaps out.
                "repo-1",
                &old.source_path,
                std::slice::from_ref(&replacement),
            )
            .await
            .unwrap();

        let all = storage.list_all().await.unwrap();
        assert_eq!(all.len(), 2, "old row replaced, unrelated row kept");
        assert!(all.iter().any(|l| l.id == "new-1"));
        assert!(all.iter().any(|l| l.id == "other-1"));
        assert!(!all.iter().any(|l| l.id == "old-1"));
    }

    /// delete_by_source_repo removes only the given repo's rows.
    #[tokio::test]
    async fn delete_by_source_repo_scopes_to_repo() {
        let (_tmp, storage) = tmp_storage();
        let a = entry("a-1", SpecLinkType::Implements);
        storage.save(&a).await.unwrap();
        let mut b = entry("b-1", SpecLinkType::Implements);
        b.source_repo_id = Some("repo-other".to_string());
        storage.save(&b).await.unwrap();

        storage.delete_by_source_repo("repo-1").await.unwrap();
        let all = storage.list_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b-1");
    }
}
