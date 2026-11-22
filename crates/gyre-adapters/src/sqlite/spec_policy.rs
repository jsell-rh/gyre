use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecPolicy;
use gyre_ports::SpecPolicyRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_policies;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_policies)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecPolicyRow {
    #[allow(dead_code)]
    repo_id: String,
    require_spec_ref: i32,
    require_approved_spec: i32,
    warn_stale_spec: i32,
    require_current_spec: i32,
    require_signed_approval: i32,
    enforce_manifest: i32,
}

impl SpecPolicyRow {
    fn into_policy(self) -> SpecPolicy {
        SpecPolicy {
            require_spec_ref: self.require_spec_ref != 0,
            require_approved_spec: self.require_approved_spec != 0,
            warn_stale_spec: self.warn_stale_spec != 0,
            require_current_spec: self.require_current_spec != 0,
            require_signed_approval: self.require_signed_approval != 0,
            enforce_manifest: self.enforce_manifest != 0,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_policies)]
struct NewSpecPolicyRow<'a> {
    repo_id: &'a str,
    require_spec_ref: i32,
    require_approved_spec: i32,
    warn_stale_spec: i32,
    require_current_spec: i32,
    require_signed_approval: i32,
    enforce_manifest: i32,
}

#[async_trait]
impl SpecPolicyRepository for SqliteStorage {
    async fn get_for_repo(&self, repo_id: &str) -> Result<SpecPolicy> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<SpecPolicy> {
            let mut conn = pool.get().context("get db connection")?;
            let row = spec_policies::table
                .find(&repo_id)
                .first::<SpecPolicyRow>(&mut *conn)
                .optional()
                .context("find spec policy for repo")?;
            Ok(row.map(SpecPolicyRow::into_policy).unwrap_or_default())
        })
        .await?
    }

    async fn set_for_repo(&self, repo_id: &str, policy: SpecPolicy) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecPolicyRow {
                repo_id: &repo_id,
                require_spec_ref: if policy.require_spec_ref { 1 } else { 0 },
                require_approved_spec: if policy.require_approved_spec { 1 } else { 0 },
                warn_stale_spec: if policy.warn_stale_spec { 1 } else { 0 },
                require_current_spec: if policy.require_current_spec { 1 } else { 0 },
                require_signed_approval: if policy.require_signed_approval { 1 } else { 0 },
                enforce_manifest: if policy.enforce_manifest { 1 } else { 0 },
            };
            diesel::insert_into(spec_policies::table)
                .values(&row)
                .on_conflict(spec_policies::repo_id)
                .do_update()
                .set((
                    spec_policies::require_spec_ref.eq(row.require_spec_ref),
                    spec_policies::require_approved_spec.eq(row.require_approved_spec),
                    spec_policies::warn_stale_spec.eq(row.warn_stale_spec),
                    spec_policies::require_current_spec.eq(row.require_current_spec),
                    spec_policies::require_signed_approval.eq(row.require_signed_approval),
                    spec_policies::enforce_manifest.eq(row.enforce_manifest),
                ))
                .execute(&mut *conn)
                .context("upsert spec policy")?;
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

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    #[tokio::test]
    async fn set_and_get_round_trips_all_five_policies() {
        // task-139: all five forge enforcement policies (agent-gates.md
        // §Forge Enforcement Policies) must persist — the
        // require_signed_approval column arrives via migration 000059, so
        // this also proves the migration ran on this fresh database.
        let (_tmp, storage) = tmp_storage();

        // Default for an unknown repo: everything opt-in false.
        let default = storage.get_for_repo("repo-x").await.unwrap();
        assert!(!default.require_spec_ref);
        assert!(!default.require_approved_spec);
        assert!(!default.warn_stale_spec);
        assert!(!default.require_current_spec);
        assert!(!default.require_signed_approval);
        assert!(!default.enforce_manifest);

        let policy = SpecPolicy {
            require_spec_ref: true,
            require_approved_spec: true,
            warn_stale_spec: true,
            require_current_spec: true,
            require_signed_approval: true,
            enforce_manifest: false,
        };
        storage.set_for_repo("repo-x", policy).await.unwrap();

        let loaded = storage.get_for_repo("repo-x").await.unwrap();
        assert!(loaded.require_spec_ref);
        assert!(loaded.require_approved_spec);
        assert!(loaded.warn_stale_spec);
        assert!(loaded.require_current_spec);
        assert!(
            loaded.require_signed_approval,
            "require_signed_approval must persist"
        );
        assert!(!loaded.enforce_manifest);

        // Update path: flipping a field persists (upsert, not insert-only).
        let updated = SpecPolicy {
            require_signed_approval: false,
            ..loaded
        };
        storage.set_for_repo("repo-x", updated).await.unwrap();
        let reloaded = storage.get_for_repo("repo-x").await.unwrap();
        assert!(!reloaded.require_signed_approval);
        assert!(reloaded.require_current_spec);
    }
}
