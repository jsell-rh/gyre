use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{RepoAccessOverride, WorkspaceRole};
use gyre_ports::RepoAccessOverrideRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::repo_access_overrides;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repo_access_overrides)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct OverrideRow {
    id: String,
    repo_id: String,
    user_id: String,
    role: String,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

impl OverrideRow {
    fn into_override(self) -> Result<RepoAccessOverride> {
        let role = WorkspaceRole::parse_role(&self.role)
            .ok_or_else(|| anyhow!("unknown workspace role: {}", self.role))?;
        Ok(RepoAccessOverride {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            user_id: Id::new(self.user_id),
            role,
            created_by: Id::new(self.created_by),
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = repo_access_overrides)]
struct NewOverrideRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    user_id: &'a str,
    role: &'a str,
    created_by: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[async_trait]
impl RepoAccessOverrideRepository for SqliteStorage {
    async fn create(&self, override_: &RepoAccessOverride) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let o = override_.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewOverrideRow {
                id: o.id.as_str(),
                repo_id: o.repo_id.as_str(),
                user_id: o.user_id.as_str(),
                role: o.role.as_str(),
                created_by: o.created_by.as_str(),
                created_at: o.created_at as i64,
                updated_at: o.updated_at as i64,
            };
            // The UNIQUE(repo_id, user_id) constraint rejects duplicates; map
            // the Diesel error to `anyhow!` text the caller can branch on.
            diesel::insert_into(repo_access_overrides::table)
                .values(&row)
                .execute(&mut *conn)
                .map_err(|e| {
                    if e.to_string().contains("UNIQUE") {
                        anyhow!(
                            "repo access override for repo {} user {} already exists",
                            o.repo_id,
                            o.user_id
                        )
                    } else {
                        anyhow!("insert repo access override: {e}")
                    }
                })?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repo_access_overrides::table
                .find(id.as_str())
                .first::<OverrideRow>(&mut *conn)
                .optional()
                .context("find repo access override by id")?;
            result.map(OverrideRow::into_override).transpose()
        })
        .await?
    }

    async fn find_by_repo_and_user(
        &self,
        repo_id: &Id,
        user_id: &Id,
    ) -> Result<Option<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repo_access_overrides::table
                .filter(repo_access_overrides::repo_id.eq(rid.as_str()))
                .filter(repo_access_overrides::user_id.eq(uid.as_str()))
                .first::<OverrideRow>(&mut *conn)
                .optional()
                .context("find repo access override by repo and user")?;
            result.map(OverrideRow::into_override).transpose()
        })
        .await?
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repo_access_overrides::table
                .filter(repo_access_overrides::repo_id.eq(rid.as_str()))
                .order(repo_access_overrides::created_at.asc())
                .load::<OverrideRow>(&mut *conn)
                .context("list repo access overrides by repo")?;
            rows.into_iter().map(OverrideRow::into_override).collect()
        })
        .await?
    }

    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(repo_access_overrides::table.find(id.as_str()))
                .set((
                    repo_access_overrides::role.eq(role.as_str()),
                    repo_access_overrides::updated_at.eq(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64),
                ))
                .execute(&mut *conn)
                .context("update repo access override role")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(repo_access_overrides::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete repo access override")?;
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

    fn make_override(id: &str, repo: &str, user: &str, role: WorkspaceRole) -> RepoAccessOverride {
        RepoAccessOverride::new(
            Id::new(id),
            Id::new(repo),
            Id::new(user),
            role,
            Id::new("admin-1"),
            1000,
        )
    }

    #[tokio::test]
    async fn override_roundtrip_and_lookup() {
        let (_tmp, s) = setup();
        RepoAccessOverrideRepository::create(
            &s,
            &make_override("o1", "repo-1", "dev-1", WorkspaceRole::Viewer),
        )
        .await
        .unwrap();

        let found = RepoAccessOverrideRepository::find_by_repo_and_user(
            &s,
            &Id::new("repo-1"),
            &Id::new("dev-1"),
        )
        .await
        .unwrap()
        .expect("override must be findable by (repo, user)");
        assert_eq!(found.role, WorkspaceRole::Viewer);
        assert_eq!(found.created_at, 1000);

        let listed = RepoAccessOverrideRepository::list_by_repo(&s, &Id::new("repo-1"))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);

        RepoAccessOverrideRepository::update_role(&s, &found.id, WorkspaceRole::Developer)
            .await
            .unwrap();
        let updated = RepoAccessOverrideRepository::find_by_id(&s, &found.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.role, WorkspaceRole::Developer);

        RepoAccessOverrideRepository::delete(&s, &found.id)
            .await
            .unwrap();
        assert!(RepoAccessOverrideRepository::find_by_id(&s, &found.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn duplicate_repo_user_pair_is_rejected() {
        // Port contract: "Fails if an override for the same (repo_id, user_id)
        // pair already exists" — enforced by the UNIQUE constraint.
        let (_tmp, s) = setup();
        RepoAccessOverrideRepository::create(
            &s,
            &make_override("o1", "repo-1", "dev-1", WorkspaceRole::Viewer),
        )
        .await
        .unwrap();
        let err = RepoAccessOverrideRepository::create(
            &s,
            &make_override("o2", "repo-1", "dev-1", WorkspaceRole::Developer),
        )
        .await;
        assert!(err.is_err(), "duplicate (repo, user) must fail");
        // A different user on the same repo is fine.
        RepoAccessOverrideRepository::create(
            &s,
            &make_override("o3", "repo-1", "dev-2", WorkspaceRole::Viewer),
        )
        .await
        .unwrap();
    }
}
