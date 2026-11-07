use anyhow::Result;
use async_trait::async_trait;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Error as R2d2Error, Pool};
use diesel::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use gyre_ports::storage::StoragePort;
use std::sync::Arc;
use tracing::instrument;

pub mod agent;
pub mod agent_commit;
pub mod analytics;
pub mod attestation;
pub mod audit;
pub mod budget;
pub mod chain_attestation;
pub mod commit_signature;
pub mod compute_target;
pub mod container_audit;
pub mod conversation;
pub mod dependency;
pub mod graph;
pub mod key_binding;
pub mod kv_store;
pub mod llm_config;
pub mod merge_queue;
pub mod merge_request;
pub mod message;
pub mod meta_spec;
pub mod meta_spec_set;
pub mod network_peer;
pub mod notification;
pub mod policy;
pub mod prompt_template;
pub mod push_gate;
pub mod quality_gate;
pub mod repository;
pub mod review;
pub mod saved_view;
pub mod search;
pub mod secret;
pub mod spawn_log;
pub mod spec_approval;
pub mod spec_approval_event;
pub mod spec_assertion_results;
pub mod spec_ledger;
pub mod spec_policy;
pub mod task;
pub mod team;
pub mod tenant;
pub mod trace;
pub mod trust_anchor;
pub mod user;
pub mod user_profile;
pub mod user_workspace_state;
pub mod workspace;
pub mod workspace_membership;
pub mod worktree;

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// r2d2 connection customizer: enables WAL mode and foreign keys on every connection.
#[derive(Debug)]
struct SqliteCustomizer;

impl CustomizeConnection<SqliteConnection, R2d2Error> for SqliteCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), R2d2Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )
        .execute(conn)
        .map_err(R2d2Error::QueryError)?;
        Ok(())
    }
}

pub(crate) type DbPool = Pool<ConnectionManager<SqliteConnection>>;

/// Diesel-backed storage adapter.
/// Implements all port traits for local/development deployments.
/// All queries are scoped to `tenant_id` for multi-tenant isolation.
#[derive(Clone)]
pub struct SqliteStorage {
    /// Diesel r2d2 connection pool.
    pub(crate) pool: Arc<DbPool>,
    /// Tenant scope for all queries. Defaults to "default".
    pub(crate) tenant_id: String,
}

impl SqliteStorage {
    /// Open (or create) the SQLite database and run Diesel migrations.
    /// Uses the "default" tenant scope.
    pub fn new(db_path: impl Into<String>) -> Result<Self> {
        Self::new_for_tenant(db_path, "default")
    }

    /// Return a new handle scoped to `tenant_id`, reusing the same connection pool.
    ///
    /// This is cheap -- it only clones the `Arc` pointer to the pool. Use this
    /// in request handlers to switch tenant scope without re-opening the DB.
    pub fn with_tenant(&self, tenant_id: impl Into<String>) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            tenant_id: tenant_id.into(),
        }
    }

    /// Open (or create) the SQLite database scoped to a specific tenant.
    /// Security: "system" tenant should only be used by Admin-role callers.
    pub fn new_for_tenant(
        db_path: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Result<Self> {
        let db_path: String = db_path.into();

        // Build r2d2 pool with WAL + foreign keys customizer.
        let manager = ConnectionManager::<SqliteConnection>::new(&db_path);
        let pool = Pool::builder()
            .connection_customizer(Box::new(SqliteCustomizer))
            .build(manager)?;

        // Run Diesel migrations.
        {
            let mut conn = pool.get()?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| anyhow::anyhow!("Diesel migration failed: {e}"))?;
            // SQLite-only FTS5 search index (search.md §Index Schema). Dialect-
            // specific DDL must not go in the shared diesel migrations dir — that
            // dir is embedded and runs on PostgreSQL too, where fts5 breaks it.
            search::ensure_fts_table(&mut conn)?;
        }

        Ok(Self {
            pool: Arc::new(pool),
            tenant_id: tenant_id.into(),
        })
    }
}

#[async_trait]
impl StoragePort for SqliteStorage {
    #[instrument(skip(self), err)]
    async fn health_check(&self) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<()> {
            use diesel::RunQueryDsl;
            let mut conn = pool.get()?;
            diesel::sql_query("SELECT 1")
                .execute(&mut *conn)
                .map_err(anyhow::Error::from)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    #[instrument(skip(self), err)]
    async fn migrations_pending(&self) -> Result<usize> {
        let pool = Arc::clone(&self.pool);
        let pending = tokio::task::spawn_blocking(move || -> Result<usize> {
            let mut conn = pool.get()?;
            let pending = conn
                .pending_migrations(MIGRATIONS)
                .map_err(|e| anyhow::anyhow!("migration state check failed: {e}"))?
                .len();
            Ok(pending)
        })
        .await??;
        Ok(pending)
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

    #[tokio::test]
    async fn health_check_ok() {
        let (_tmp, storage) = tmp_storage();
        storage.health_check().await.unwrap();
    }

    #[tokio::test]
    async fn migrations_pending_zero_after_startup() {
        // Constructor runs all pending migrations, so a fresh DB must report 0.
        let (_tmp, storage) = tmp_storage();
        assert_eq!(storage.migrations_pending().await.unwrap(), 0);
    }

    #[test]
    fn migrations_create_tables() {
        let (_tmp, storage) = tmp_storage();
        let mut conn = storage.pool.get().unwrap();
        let tables = [
            // Core schema (from schema.rs — Diesel-managed tables)
            "repositories",
            "agents",
            "tasks",
            "merge_requests",
            "activity_events",
            "review_comments",
            "reviews",
            "merge_queue",
            "agent_commits",
            "agent_worktrees",
            "users",
            "api_keys",
            "analytics_events",
            "cost_entries",
            "audit_events",
            "spawn_log",
            "revoked_tokens",
            "network_peers",
            "workspaces",
            "personas",
            "teams",
            "workspace_memberships",
            "notifications",
            "policies",
            "policy_decisions",
            "spec_approvals",
            "dependency_edges",
            "budget_configs",
            "quality_gates",
            "gate_results",
            "repo_push_gates",
            "spec_policies",
            "attestation_bundles",
            "container_audit_records",
            "spec_ledger_entries",
            "spec_approval_events",
            "tenants",
            "messages",
            "meta_spec_sets",
            "gate_traces",
            "trace_spans",
            "conversations",
            "turn_commit_links",
            "budget_call_records",
            "user_workspace_state",
            // Previously shadowed by duplicate version prefixes — regression guard
            // (these tables were silently skipped on fresh installs before the fix)
            "kv_store",
            "budget_usages",
            "graph_nodes",
            "graph_edges",
            "graph_deltas",
            // LLM function config (migration 000031)
            "llm_function_configs",
            // User profile tables (migration 000036)
            "user_notification_preferences",
            "user_tokens",
            // Meta-spec registry (migration 000032)
            "meta_specs",
            "meta_spec_versions",
            "meta_spec_bindings",
            // Authorization provenance (migration 000047)
            "trust_anchors",
            "key_bindings",
            "chain_attestations",
            // Raw-SQL tables (not in schema.rs but created by migrations)
            "explorer_views",
            "prompt_templates",
        ];
        for table in &tables {
            use diesel::RunQueryDsl;
            // A table exists if we can SELECT from it without error.
            diesel::sql_query(format!("SELECT 1 FROM {table} LIMIT 0"))
                .execute(&mut *conn)
                .unwrap_or_else(|e| panic!("table '{table}' missing: {e}"));
        }
    }

    #[test]
    fn migrations_create_previously_shadowed_columns() {
        // 000051 collision (task-093 vs task-095, same-day merges): Diesel
        // ran only post_merge_gates, silently dropping the orchestrator
        // columns. Guard asserts they exist after the renumbered 000052.
        let (_tmp, storage) = tmp_storage();
        let mut conn = storage.pool.get().unwrap();
        for (table, column) in [
            ("agents", "orchestrator_type"),
            ("agents", "repo_id"),
            ("agents", "restart_on_failure"),
            ("quality_gates", "gate_phase"),
            ("quality_gates", "timeout_secs"),
        ] {
            use diesel::RunQueryDsl;
            let ok = diesel::sql_query(format!("SELECT {column} FROM {table} LIMIT 0"))
                .execute(&mut *conn)
                .is_ok();
            assert!(ok, "column '{table}.{column}' missing after migrations");
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        // Run twice — should not fail.
        SqliteStorage::new(path).unwrap();
        SqliteStorage::new(path).unwrap();
    }

    /// Task-102 F1 regression: migration 000054 folds legacy path/pid
    /// rows into the envelope `detail` payload. The fold must be plain
    /// portable SQL (CASE WHEN + string concatenation): this migrations
    /// directory is embedded once and executed by BOTH storage backends,
    /// and the previous json_patch/json_object form aborted PostgreSQL
    /// deployments at startup. This test drives the REAL embedded up.sql
    /// against seeded legacy rows — including JSON-hostile path values
    /// (quote, backslash, newline, tab) and non-object details — and
    /// asserts the folded payload parses back to exactly the merged
    /// data (merge-patch semantics: NULL source values add no key, a
    /// pre-existing key is never overwritten, a non-object target is
    /// replaced by the folded object).
    #[test]
    fn migration_000054_folds_legacy_path_pid_into_detail() {
        use diesel::sql_types::{Integer, Nullable, Text};
        use diesel::RunQueryDsl;

        // `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
        // `Self { field: field, … }` with spans mapped onto the field
        // declarations below, so clippy::redundant_field_names fires on
        // derive-generated code. The generated construction sits in a dummy
        // module that inherits neither struct- nor field-level `#[allow]`
        // (verified in sqlite/search.rs), so the lint is scoped off for this
        // small row-carrier module instead. Nothing here writes
        // `field: field` initializations by hand.
        mod rows {
            #![allow(clippy::redundant_field_names)]
            use diesel::sql_types::{Nullable, Text};
            use diesel::QueryableByName;

            #[derive(QueryableByName)]
            pub(super) struct FoldedRow {
                #[diesel(sql_type = Text)]
                pub(super) id: String,
                #[diesel(sql_type = Text)]
                pub(super) detail: String,
                #[diesel(sql_type = Nullable<Text>)]
                pub(super) agent_id: Option<String>,
                #[diesel(sql_type = Text)]
                pub(super) resource_type: String,
                #[diesel(sql_type = Nullable<Text>)]
                pub(super) resource_id: Option<String>,
                #[diesel(sql_type = Text)]
                pub(super) outcome: String,
            }
            #[derive(QueryableByName)]
            pub(super) struct VersionRow {
                #[diesel(sql_type = Text)]
                pub(super) version: String,
            }
        }
        use rows::{FoldedRow, VersionRow};

        let (_tmp, storage) = tmp_storage();
        let mut conn = storage.pool.get().unwrap();

        // Re-create the pre-000054 legacy shape and seed representative rows.
        diesel::sql_query("DROP TABLE audit_events")
            .execute(&mut conn)
            .unwrap();
        diesel::sql_query(
            "CREATE TABLE audit_events (\
             id TEXT PRIMARY KEY NOT NULL, \
             agent_id TEXT NOT NULL, \
             event_type TEXT NOT NULL, \
             path TEXT, \
             details TEXT NOT NULL DEFAULT '{}', \
             pid INTEGER, \
             timestamp BIGINT NOT NULL)",
        )
        .execute(&mut conn)
        .unwrap();
        let insert = "INSERT INTO audit_events \
             (id, agent_id, event_type, path, details, pid, timestamp) \
             VALUES (?, 'ag-1', 'file_access', ?, ?, ?, 1)";
        let seed: &[(&str, Option<&str>, &str, Option<i32>)] = &[
            ("both", Some("/etc/hosts"), r#"{"mode":"read"}"#, Some(1234)),
            ("nulls", None, "{}", None),
            ("pid_only", None, r#"{"k":1}"#, Some(77)),
            ("hostile", Some("/tmp/we\"ird\\pa\nth\t"), "{}", None),
            ("scalar", Some("/x"), "42", None),
            ("existing_key", None, r#"{"path":"already","k":2}"#, None),
        ];
        for (id, path, details, pid) in seed {
            diesel::sql_query(insert)
                .bind::<Text, _>(id)
                .bind::<Nullable<Text>, _>(path)
                .bind::<Text, _>(details)
                .bind::<Nullable<Integer>, _>(pid)
                .execute(&mut conn)
                .unwrap();
        }

        // Un-mark 000054 so the runner re-applies the real up.sql to the
        // legacy rows (the same transition a deployed database makes).
        let versions: Vec<VersionRow> = diesel::sql_query(
            "SELECT version FROM __diesel_schema_migrations WHERE version LIKE '%000054%'",
        )
        .load(&mut conn)
        .unwrap();
        assert_eq!(versions.len(), 1, "expected exactly one 000054 version row");
        let deleted = diesel::sql_query("DELETE FROM __diesel_schema_migrations WHERE version = ?")
            .bind::<Text, _>(&versions[0].version)
            .execute(&mut conn)
            .unwrap();
        assert_eq!(deleted, 1, "failed to un-mark migration 000054");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("portable 000054 up.sql must run (task-102 F1)");

        let rows: Vec<FoldedRow> = diesel::sql_query(
            "SELECT id, detail, agent_id, resource_type, resource_id, outcome FROM audit_events",
        )
        .load(&mut conn)
        .unwrap();
        let folded: std::collections::HashMap<String, serde_json::Value> = rows
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    serde_json::from_str(&r.detail)
                        .unwrap_or_else(|e| panic!("invalid folded JSON for {}: {e}", r.id)),
                )
            })
            .collect();
        assert_eq!(
            folded["both"],
            serde_json::json!({"mode": "read", "path": "/etc/hosts", "pid": 1234})
        );
        assert_eq!(folded["nulls"], serde_json::json!({}));
        assert_eq!(folded["pid_only"], serde_json::json!({"k": 1, "pid": 77}));
        assert_eq!(
            folded["hostile"],
            serde_json::json!({"path": "/tmp/we\"ird\\pa\nth\t"})
        );
        // Merge-patch replaces a non-object target with the folded object.
        assert_eq!(folded["scalar"], serde_json::json!({"path": "/x"}));
        // A pre-existing key is never overwritten; nothing to fold keeps
        // the legacy payload verbatim.
        assert_eq!(
            folded["existing_key"],
            serde_json::json!({"path": "already", "k": 2})
        );

        // Envelope backfill columns for legacy rows.
        for r in &rows {
            assert_eq!(r.agent_id.as_deref(), Some("ag-1"));
            assert_eq!(r.resource_type, "agent");
            assert_eq!(r.resource_id.as_deref(), Some("ag-1"));
            assert_eq!(r.outcome, "success");
        }
    }

    /// Guard: detect duplicate migration version strings before they shadow each other.
    ///
    /// Diesel's `migrations_internals::version_from_string` extracts the version from a
    /// migration directory name by taking everything before the first `_` and stripping
    /// dashes: e.g. `2026-03-23-000009_kv_store_and_budget_usage` → `"20260323000009"`.
    /// Two directories that produce the same version string will collide — Diesel runs
    /// only the last one alphabetically, silently dropping the other's schema changes.
    ///
    /// This test replicates that extraction logic and fails if any two migration
    /// directories produce the same version, so the defect is caught before it reaches
    /// a deployed database.
    #[test]
    fn no_duplicate_migration_version_prefixes() {
        let migrations_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
        let mut versions = std::collections::HashMap::<String, Vec<String>>::new();

        let entries = std::fs::read_dir(migrations_dir).expect("migrations directory should exist");

        for entry in entries {
            let entry = entry.expect("valid dir entry");
            let name = entry.file_name().into_string().expect("valid UTF-8");
            // Replicate Diesel's version_from_string:
            //   take everything before the first '_', then remove all '-'
            let version = name.split('_').next().unwrap_or("").replace('-', "");
            if !version.is_empty() {
                versions.entry(version).or_default().push(name);
            }
        }

        let duplicates: Vec<_> = versions
            .into_iter()
            .filter(|(_, names)| names.len() > 1)
            .collect();

        assert!(
            duplicates.is_empty(),
            "Duplicate Diesel migration versions detected — only one will run per version:\n{}",
            duplicates
                .iter()
                .map(|(version, names)| format!("  version {}: {:?}", version, names))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
