use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{
    GlobalRole, InvitationStatus, TenantInvitation, WorkspaceInvitation, WorkspaceRole,
};
use gyre_ports::{TenantInvitationRepository, WorkspaceInvitationRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{tenant_invitations, workspace_invitations};

fn ids_to_json(ids: &[Id]) -> String {
    serde_json::to_string(&ids.iter().map(|i| i.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".to_string())
}

fn json_to_ids(s: &str) -> Vec<Id> {
    let strs: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    strs.into_iter().map(Id::new).collect()
}

fn roles_to_json(roles: &[WorkspaceRole]) -> String {
    serde_json::to_string(&roles.iter().map(|r| r.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".to_string())
}

fn json_to_ws_roles(s: &str) -> Result<Vec<WorkspaceRole>> {
    let strs: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    strs.iter()
        .map(|r| {
            WorkspaceRole::parse_role(r)
                .ok_or_else(|| anyhow!("unknown workspace role in invitation: {r}"))
        })
        .collect()
}

fn parse_global_role(s: &str) -> Result<GlobalRole> {
    match s {
        "TenantAdmin" => Ok(GlobalRole::TenantAdmin),
        "Member" => Ok(GlobalRole::Member),
        other => Err(anyhow!("unknown global role in invitation: {other}")),
    }
}

fn parse_invitation_status(s: &str) -> Result<InvitationStatus> {
    InvitationStatus::parse(s).ok_or_else(|| anyhow!("unknown invitation status: {s}"))
}

// ── Tenant invitations ───────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = tenant_invitations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct TenantInvitationRow {
    id: String,
    tenant_id: String,
    email: String,
    invited_by: String,
    role: String,
    workspace_ids: String,
    workspace_roles: String,
    status: String,
    token_hash: String,
    expires_at: i64,
    created_at: i64,
    accepted_at: Option<i64>,
}

impl TenantInvitationRow {
    fn into_invitation(self) -> Result<TenantInvitation> {
        Ok(TenantInvitation {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            email: self.email,
            invited_by: Id::new(self.invited_by),
            role: parse_global_role(&self.role)?,
            workspace_ids: json_to_ids(&self.workspace_ids),
            workspace_roles: json_to_ws_roles(&self.workspace_roles)?,
            status: parse_invitation_status(&self.status)?,
            token_hash: self.token_hash,
            expires_at: self.expires_at as u64,
            created_at: self.created_at as u64,
            accepted_at: self.accepted_at.map(|v| v as u64),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = tenant_invitations)]
struct NewTenantInvitationRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    email: &'a str,
    invited_by: &'a str,
    role: &'a str,
    workspace_ids: String,
    workspace_roles: String,
    status: &'a str,
    token_hash: &'a str,
    expires_at: i64,
    created_at: i64,
    accepted_at: Option<i64>,
}

#[async_trait]
impl TenantInvitationRepository for PgStorage {
    async fn create(&self, invitation: &TenantInvitation) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let inv = invitation.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fail if a pending invitation for the same
            // (tenant, email) already exists. The unique index covers only
            // token_hash; the pending-uniqueness is enforced here so the
            // error can name the conflict.
            let existing_pending = diesel::dsl::select(diesel::dsl::exists(
                tenant_invitations::table
                    .filter(tenant_invitations::tenant_id.eq(inv.tenant_id.as_str()))
                    .filter(tenant_invitations::email.eq(inv.email.as_str()))
                    .filter(tenant_invitations::status.eq("Pending")),
            ))
            .get_result::<bool>(&mut *conn)
            .context("check pending tenant invitation")?;
            if existing_pending {
                anyhow::bail!(
                    "a pending invitation for {} in tenant {} already exists",
                    inv.email,
                    inv.tenant_id
                );
            }
            let row = NewTenantInvitationRow {
                id: inv.id.as_str(),
                tenant_id: inv.tenant_id.as_str(),
                email: &inv.email,
                invited_by: inv.invited_by.as_str(),
                role: match inv.role {
                    GlobalRole::TenantAdmin => "TenantAdmin",
                    GlobalRole::Member => "Member",
                },
                workspace_ids: ids_to_json(&inv.workspace_ids),
                workspace_roles: roles_to_json(&inv.workspace_roles),
                status: inv.status.as_str(),
                token_hash: &inv.token_hash,
                expires_at: inv.expires_at as i64,
                created_at: inv.created_at as i64,
                accepted_at: inv.accepted_at.map(|v| v as i64),
            };
            diesel::insert_into(tenant_invitations::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert tenant invitation")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<TenantInvitation>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<TenantInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            tenant_invitations::table
                .find(id.as_str())
                .first::<TenantInvitationRow>(&mut *conn)
                .optional()
                .context("find tenant invitation by id")?
                .map(TenantInvitationRow::into_invitation)
                .transpose()
        })
        .await?
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<TenantInvitation>> {
        let pool = Arc::clone(&self.pool);
        let hash = token_hash.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<TenantInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            tenant_invitations::table
                .filter(tenant_invitations::token_hash.eq(hash.as_str()))
                .first::<TenantInvitationRow>(&mut *conn)
                .optional()
                .context("find tenant invitation by token hash")?
                .map(TenantInvitationRow::into_invitation)
                .transpose()
        })
        .await?
    }

    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<TenantInvitation>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<TenantInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            tenant_invitations::table
                .filter(tenant_invitations::tenant_id.eq(tid.as_str()))
                .order(tenant_invitations::created_at.desc())
                .load::<TenantInvitationRow>(&mut *conn)
                .context("list tenant invitations")?
                .into_iter()
                .map(TenantInvitationRow::into_invitation)
                .collect()
        })
        .await?
    }

    async fn list_by_status(&self, status: InvitationStatus) -> Result<Vec<TenantInvitation>> {
        let pool = Arc::clone(&self.pool);
        let status_str = status.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<TenantInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            tenant_invitations::table
                .filter(tenant_invitations::status.eq(status_str.as_str()))
                .load::<TenantInvitationRow>(&mut *conn)
                .context("list tenant invitations by status")?
                .into_iter()
                .map(TenantInvitationRow::into_invitation)
                .collect()
        })
        .await?
    }

    async fn update_status(
        &self,
        id: &Id,
        status: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let status_str = status.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(tenant_invitations::table.find(id.as_str()))
                .set((
                    tenant_invitations::status.eq(status_str.as_str()),
                    tenant_invitations::accepted_at.eq(accepted_at.map(|v| v as i64)),
                ))
                .execute(&mut *conn)
                .context("update tenant invitation status")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(tenant_invitations::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete tenant invitation")?;
            Ok(())
        })
        .await?
    }
}

// ── Workspace invitations ────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = workspace_invitations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct WorkspaceInvitationRow {
    id: String,
    tenant_id: String,
    workspace_id: String,
    user_id: String,
    invited_by: String,
    role: String,
    status: String,
    token_hash: String,
    expires_at: i64,
    created_at: i64,
    accepted_at: Option<i64>,
}

impl WorkspaceInvitationRow {
    fn into_invitation(self) -> Result<WorkspaceInvitation> {
        Ok(WorkspaceInvitation {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            workspace_id: Id::new(self.workspace_id),
            user_id: Id::new(self.user_id),
            invited_by: Id::new(self.invited_by),
            role: WorkspaceRole::parse_role(&self.role)
                .ok_or_else(|| anyhow!("unknown workspace role in invitation: {}", self.role))?,
            status: parse_invitation_status(&self.status)?,
            token_hash: self.token_hash,
            expires_at: self.expires_at as u64,
            created_at: self.created_at as u64,
            accepted_at: self.accepted_at.map(|v| v as u64),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = workspace_invitations)]
struct NewWorkspaceInvitationRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    workspace_id: &'a str,
    user_id: &'a str,
    invited_by: &'a str,
    role: &'a str,
    status: &'a str,
    token_hash: &'a str,
    expires_at: i64,
    created_at: i64,
    accepted_at: Option<i64>,
}

#[async_trait]
impl WorkspaceInvitationRepository for PgStorage {
    async fn create(&self, invitation: &WorkspaceInvitation) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let inv = invitation.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Port contract: fail if a pending invitation for the same
            // (workspace, user) already exists.
            let existing_pending = diesel::dsl::select(diesel::dsl::exists(
                workspace_invitations::table
                    .filter(
                        workspace_invitations::workspace_id.eq(inv.workspace_id.as_str()),
                    )
                    .filter(workspace_invitations::user_id.eq(inv.user_id.as_str()))
                    .filter(workspace_invitations::status.eq("Pending")),
            ))
            .get_result::<bool>(&mut *conn)
            .context("check pending workspace invitation")?;
            if existing_pending {
                anyhow::bail!(
                    "a pending invitation for user {} in workspace {} already exists",
                    inv.user_id,
                    inv.workspace_id
                );
            }
            let row = NewWorkspaceInvitationRow {
                id: inv.id.as_str(),
                tenant_id: inv.tenant_id.as_str(),
                workspace_id: inv.workspace_id.as_str(),
                user_id: inv.user_id.as_str(),
                invited_by: inv.invited_by.as_str(),
                role: inv.role.as_str(),
                status: inv.status.as_str(),
                token_hash: &inv.token_hash,
                expires_at: inv.expires_at as i64,
                created_at: inv.created_at as i64,
                accepted_at: inv.accepted_at.map(|v| v as i64),
            };
            diesel::insert_into(workspace_invitations::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert workspace invitation")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<WorkspaceInvitation>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<WorkspaceInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            workspace_invitations::table
                .find(id.as_str())
                .first::<WorkspaceInvitationRow>(&mut *conn)
                .optional()
                .context("find workspace invitation by id")?
                .map(WorkspaceInvitationRow::into_invitation)
                .transpose()
        })
        .await?
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<WorkspaceInvitation>> {
        let pool = Arc::clone(&self.pool);
        let hash = token_hash.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<WorkspaceInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            workspace_invitations::table
                .filter(workspace_invitations::token_hash.eq(hash.as_str()))
                .first::<WorkspaceInvitationRow>(&mut *conn)
                .optional()
                .context("find workspace invitation by token hash")?
                .map(WorkspaceInvitationRow::into_invitation)
                .transpose()
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<WorkspaceInvitation>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            workspace_invitations::table
                .filter(workspace_invitations::workspace_id.eq(wid.as_str()))
                .order(workspace_invitations::created_at.desc())
                .load::<WorkspaceInvitationRow>(&mut *conn)
                .context("list workspace invitations")?
                .into_iter()
                .map(WorkspaceInvitationRow::into_invitation)
                .collect()
        })
        .await?
    }

    async fn list_by_user(&self, user_id: &Id) -> Result<Vec<WorkspaceInvitation>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            workspace_invitations::table
                .filter(workspace_invitations::user_id.eq(uid.as_str()))
                .order(workspace_invitations::created_at.desc())
                .load::<WorkspaceInvitationRow>(&mut *conn)
                .context("list workspace invitations by user")?
                .into_iter()
                .map(WorkspaceInvitationRow::into_invitation)
                .collect()
        })
        .await?
    }

    async fn list_by_status(&self, status: InvitationStatus) -> Result<Vec<WorkspaceInvitation>> {
        let pool = Arc::clone(&self.pool);
        let status_str = status.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceInvitation>> {
            let mut conn = pool.get().context("get db connection")?;
            workspace_invitations::table
                .filter(workspace_invitations::status.eq(status_str.as_str()))
                .load::<WorkspaceInvitationRow>(&mut *conn)
                .context("list workspace invitations by status")?
                .into_iter()
                .map(WorkspaceInvitationRow::into_invitation)
                .collect()
        })
        .await?
    }

    async fn update_status(
        &self,
        id: &Id,
        status: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let status_str = status.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(workspace_invitations::table.find(id.as_str()))
                .set((
                    workspace_invitations::status.eq(status_str.as_str()),
                    workspace_invitations::accepted_at.eq(accepted_at.map(|v| v as i64)),
                ))
                .execute(&mut *conn)
                .context("update workspace invitation status")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(workspace_invitations::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete workspace invitation")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a tenant invitation through SQLite and verify every field
    /// survives persistence, lookup by token hash, and status transitions.
    #[tokio::test]
    async fn tenant_invitation_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("test.db").to_str().unwrap()).unwrap();
        let inv = TenantInvitation {
            id: Id::new("ti1"),
            tenant_id: Id::new("t1"),
            email: "alice@example.com".to_string(),
            invited_by: Id::new("u1"),
            role: GlobalRole::Member,
            workspace_ids: vec![Id::new("ws1"), Id::new("ws2")],
            workspace_roles: vec![WorkspaceRole::Developer, WorkspaceRole::Admin],
            status: InvitationStatus::Pending,
            token_hash: "abc123".to_string(),
            expires_at: 2000,
            created_at: 1000,
            accepted_at: None,
        };
        db.create(&inv).await.unwrap();

        // Duplicate pending invitation for the same (tenant, email) is rejected.
        let dup = TenantInvitation { ..inv.clone() };
        assert!(db.create(&dup).await.is_err());

        let found = db.find_by_token_hash("abc123").await.unwrap().unwrap();
        assert_eq!(found.id.as_str(), "ti1");
        assert_eq!(found.email, "alice@example.com");
        assert_eq!(found.workspace_ids.len(), 2);
        assert_eq!(found.workspace_roles[0], WorkspaceRole::Developer);
        assert_eq!(found.workspace_roles[1], WorkspaceRole::Admin);
        assert_eq!(found.status, InvitationStatus::Pending);

        let listed = db.list_by_tenant(&Id::new("t1")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(db
            .list_by_tenant(&Id::new("other"))
            .await
            .unwrap()
            .is_empty());

        db.update_status(&Id::new("ti1"), InvitationStatus::Accepted, Some(1500))
            .await
            .unwrap();
        let accepted = db.find_by_id(&Id::new("ti1")).await.unwrap().unwrap();
        assert_eq!(accepted.status, InvitationStatus::Accepted);
        assert_eq!(accepted.accepted_at, Some(1500));

        // After a terminal status, a new invitation for the same email is allowed.
        let reinvite = TenantInvitation {
            id: Id::new("ti2"),
            token_hash: "def456".to_string(),
            ..inv
        };
        db.create(&reinvite).await.unwrap();
        let pending = db.list_by_status(InvitationStatus::Pending).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id.as_str(), "ti2");
    }

    /// Round-trip a workspace invitation and verify the pending-duplicate
    /// contract is scoped per (workspace, user).
    #[tokio::test]
    async fn workspace_invitation_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("test.db").to_str().unwrap()).unwrap();
        let inv = WorkspaceInvitation {
            id: Id::new("wi1"),
            tenant_id: Id::new("t1"),
            workspace_id: Id::new("ws1"),
            user_id: Id::new("u2"),
            invited_by: Id::new("u1"),
            role: WorkspaceRole::Viewer,
            status: InvitationStatus::Pending,
            token_hash: "tok1".to_string(),
            expires_at: 2000,
            created_at: 1000,
            accepted_at: None,
        };
        db.create(&inv).await.unwrap();

        // Same (workspace, user) pending → rejected.
        assert!(db.create(&inv).await.is_err());

        // Same user, different workspace → allowed.
        let other_ws = WorkspaceInvitation {
            id: Id::new("wi2"),
            workspace_id: Id::new("ws2"),
            token_hash: "tok2".to_string(),
            ..inv.clone()
        };
        db.create(&other_ws).await.unwrap();

        let by_user = db.list_by_user(&Id::new("u2")).await.unwrap();
        assert_eq!(by_user.len(), 2);
        let by_ws = db.list_by_workspace(&Id::new("ws1")).await.unwrap();
        assert_eq!(by_ws.len(), 1);

        db.update_status(&Id::new("wi1"), InvitationStatus::Declined, None)
            .await
            .unwrap();
        assert_eq!(
            db.find_by_id(&Id::new("wi1"))
                .await
                .unwrap()
                .unwrap()
                .status,
            InvitationStatus::Declined
        );
    }
}
