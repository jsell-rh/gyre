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

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_link(
        id: &str,
        source_repo_id: Option<&str>,
        link_type: SpecLinkType,
        target_repo_id: Option<&str>,
        target_sha: Option<&str>,
        target_display: Option<&str>,
        reason: Option<&str>,
        stale_since: Option<u64>,
    ) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: format!("system/{id}.md"),
            source_repo_id: source_repo_id.map(|s| s.to_string()),
            source_sha: format!("sha-{id}"),
            link_type,
            target_path: "system/target.md".to_string(),
            target_repo_id: target_repo_id.map(|s| s.to_string()),
            target_display: target_display.map(|s| s.to_string()),
            target_sha: target_sha.map(|s| s.to_string()),
            reason: reason.map(|s| s.to_string()),
            status: "active".to_string(),
            created_at: 1_000_000,
            stale_since,
        }
    }

    /// Every SpecLinkType variant and Option column must round-trip losslessly
    /// (task-198 acceptance: adapter round-trip incl. Option columns).
    #[tokio::test]
    async fn round_trips_all_link_types_and_option_columns() {
        let (_tmp, s) = setup();
        let variants = [
            SpecLinkType::Implements,
            SpecLinkType::Supersedes,
            SpecLinkType::DependsOn,
            SpecLinkType::ConflictsWith,
            SpecLinkType::Extends,
            SpecLinkType::References,
        ];
        let mut expected = Vec::new();
        for (i, lt) in variants.iter().enumerate() {
            // Alternate Option-column shapes: Some/None on every optional field.
            let e = make_link(
                &format!("link-{i}"),
                if i % 2 == 0 { Some("repo-1") } else { None },
                lt.clone(),
                if i % 2 == 0 { Some("repo-2") } else { None },
                if i % 3 == 0 { Some("pinned-sha") } else { None },
                if i % 2 == 0 {
                    Some("@platform-core/api-svc/system/auth.md")
                } else {
                    None
                },
                if i % 2 == 0 { Some("why not") } else { None },
                if i % 2 == 0 { Some(1_234_567) } else { None },
            );
            s.save(&e).await.unwrap();
            expected.push(e);
        }

        let loaded = s.list_all().await.unwrap();
        assert_eq!(loaded.len(), expected.len());
        for e in &expected {
            let got = loaded.iter().find(|l| l.id == e.id).unwrap();
            assert_eq!(got.id, e.id);
            assert_eq!(got.source_path, e.source_path);
            assert_eq!(got.source_repo_id, e.source_repo_id);
            assert_eq!(got.source_sha, e.source_sha, "source_sha for {}", e.id);
            assert_eq!(got.link_type, e.link_type, "link_type for {}", e.id);
            assert_eq!(got.target_path, e.target_path);
            assert_eq!(got.target_repo_id, e.target_repo_id);
            assert_eq!(got.target_display, e.target_display);
            assert_eq!(got.target_sha, e.target_sha, "target_sha for {}", e.id);
            assert_eq!(got.reason, e.reason);
            assert_eq!(got.status, e.status);
            assert_eq!(got.created_at, e.created_at);
            assert_eq!(got.stale_since, e.stale_since);
        }
    }

    /// `replace_for_source` must delete exactly the (repo, source_path) rows
    /// and insert the new set — other sources untouched.
    #[tokio::test]
    async fn replace_for_source_scopes_to_repo_and_path() {
        let (_tmp, s) = setup();

        let mut a1 = make_link("a-1", Some("repo-a"), SpecLinkType::DependsOn, None, Some("t1"), None, None, None);
        a1.source_path = "system/a.md".to_string();
        let mut a2 = make_link("a-2", Some("repo-a"), SpecLinkType::Extends, None, None, None, None, None);
        a2.source_path = "system/a.md".to_string();
        let mut b1 = make_link("b-1", Some("repo-b"), SpecLinkType::Implements, None, None, None, None, None);
        b1.source_path = "system/a.md".to_string(); // same path, other repo
        let mut other_path = make_link("a-other", Some("repo-a"), SpecLinkType::References, None, None, None, None, None);
        other_path.source_path = "system/other.md".to_string(); // same repo, other path

        for e in [&a1, &a2, &b1, &other_path] {
            s.save(e).await.unwrap();
        }

        let mut replacement = make_link("a-new", Some("repo-a"), SpecLinkType::ConflictsWith, None, None, None, None, None);
        replacement.source_path = "system/a.md".to_string();

        s.replace_for_source("repo-a", "system/a.md", &[replacement])
            .await
            .unwrap();

        let loaded = s.list_all().await.unwrap();
        let ids: Vec<&str> = loaded.iter().map(|l| l.id.as_str()).collect();
        assert!(ids.contains(&"a-new"), "replacement inserted: {ids:?}");
        assert!(!ids.contains(&"a-1"), "old a-1 removed: {ids:?}");
        assert!(!ids.contains(&"a-2"), "old a-2 removed: {ids:?}");
        assert!(ids.contains(&"b-1"), "other repo untouched: {ids:?}");
        assert!(ids.contains(&"a-other"), "other path untouched: {ids:?}");
    }

    /// `save` upserts by id — a staleness transition must overwrite the row.
    #[tokio::test]
    async fn save_upserts_by_id() {
        let (_tmp, s) = setup();
        let mut e = make_link("up-1", Some("repo-1"), SpecLinkType::Extends, None, Some("old"), None, None, None);
        s.save(&e).await.unwrap();

        // Staleness transition: same id, new status/stale_since/target_sha.
        e.status = "stale".to_string();
        e.stale_since = Some(9_999_999);
        e.target_sha = Some("new".to_string());
        s.save(&e).await.unwrap();

        let loaded = s.list_all().await.unwrap();
        assert_eq!(loaded.len(), 1, "upsert must not duplicate: {loaded:?}");
        assert_eq!(loaded[0].status, "stale");
        assert_eq!(loaded[0].stale_since, Some(9_999_999));
        assert_eq!(loaded[0].target_sha.as_deref(), Some("new"));
    }

    /// `delete_by_source_repo` removes only that repo's links.
    #[tokio::test]
    async fn delete_by_source_repo_scopes_to_repo() {
        let (_tmp, s) = setup();
        let a = make_link("del-a", Some("repo-a"), SpecLinkType::Implements, None, None, None, None, None);
        let b = make_link("del-b", Some("repo-b"), SpecLinkType::Implements, None, None, None, None, None);
        let unscoped = make_link("del-u", None, SpecLinkType::Implements, None, None, None, None, None);
        for e in [&a, &b, &unscoped] {
            s.save(e).await.unwrap();
        }

        s.delete_by_source_repo("repo-a").await.unwrap();

        let ids: Vec<String> = s.list_all().await.unwrap().into_iter().map(|l| l.id).collect();
        assert!(!ids.contains(&"del-a".to_string()));
        assert!(ids.contains(&"del-b".to_string()));
        assert!(ids.contains(&"del-u".to_string()));
    }
}
