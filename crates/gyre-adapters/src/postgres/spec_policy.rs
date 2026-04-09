use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecPolicy;
use gyre_ports::SpecPolicyRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::spec_policies;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_policies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SpecPolicyRow {
    #[allow(dead_code)]
    repo_id: String,
    require_spec_ref: i32,
    require_approved_spec: i32,
    warn_stale_spec: i32,
    require_current_spec: i32,
    enforce_manifest: i32,
}

impl SpecPolicyRow {
    fn into_policy(self) -> SpecPolicy {
        SpecPolicy {
            require_spec_ref: self.require_spec_ref != 0,
            require_approved_spec: self.require_approved_spec != 0,
            warn_stale_spec: self.warn_stale_spec != 0,
            require_current_spec: self.require_current_spec != 0,
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
    enforce_manifest: i32,
}

#[async_trait]
impl SpecPolicyRepository for PgStorage {
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
                    spec_policies::enforce_manifest.eq(row.enforce_manifest),
                ))
                .execute(&mut *conn)
                .context("upsert spec policy")?;
            Ok(())
        })
        .await?
    }
}
