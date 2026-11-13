use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{
    GlobalRole, InvitationStatus, TenantInvitation, WorkspaceInvitation, WorkspaceRole,
};
use gyre_ports::{TenantInvitationRepository, WorkspaceInvitationRepository};
use std::sync::Arc;

use super::SqliteStorage;
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

/// True when `err` is the partial unique index on
/// `(tenant_id, email) WHERE status = 'Pending'` — the database-level
/// enforcement of the "one pending invitation per (tenant, email)" port
/// contract (task-110 review F2). SQLite reports the violation either by
/// index name (newer builds) or by the column list; the token_hash unique
/// index must NOT match (it is a different constraint and names its own
/// column).
fn is_tenant_pending_scope_violation(err: &diesel::result::Error) -> bool {
    use diesel::result::{DatabaseErrorKind, Error};
    match err {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, e) => {
            let msg = e.message();
            msg.contains("idx_tenant_invitations_pending_scope")
                || msg.contains(
                    "tenant_invitations.tenant_id, tenant_invitations.email",
                )
        }
        _ => false,
    }
}

/// Workspace counterpart of [`is_tenant_pending_scope_violation`] for the
/// `(workspace_id, user_id) WHERE status = 'Pending'` partial index.
fn is_workspace_pending_scope_violation(err: &diesel::result::Error) -> bool {
    use diesel::result::{DatabaseErrorKind, Error};
    match err {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, e) => {
            let msg = e.message();
            msg.contains("idx_workspace_invitations_pending_scope")
                || msg.contains(
                    "workspace_invitations.workspace_id, workspace_invitations.user_id",
                )
        }
        _ => false,
    }
}

// ── Tenant invitations ───────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = tenant_invitations)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl TenantInvitationRepository for SqliteStorage {
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
            // The partial unique index (migration 000061) is the race
            // backstop: a concurrent insert that also passed the check
            // above is rejected by the database, and the violation is
            // surfaced as the same port-contract error (task-110 F2).
            if let Err(e) = diesel::insert_into(tenant_invitations::table)
                .values(&row)
                .execute(&mut *conn)
            {
                if is_tenant_pending_scope_violation(&e) {
                    anyhow::bail!(
                        "a pending invitation for {} in tenant {} already exists",
                        inv.email,
                        inv.tenant_id
                    );
                }
                return Err(anyhow::Error::new(e).context("insert tenant invitation"));
            }
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

    async fn transition_status(
        &self,
        id: &Id,
        from: InvitationStatus,
        to: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let from_str = from.as_str().to_string();
        let to_str = to.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            // The `from` precondition lives in the UPDATE's WHERE clause —
            // the database rejects the transition when another writer got
            // there first, so concurrent accepts cannot both win.
            let rows = diesel::update(
                tenant_invitations::table
                    .find(id.as_str())
                    .filter(tenant_invitations::status.eq(from_str.as_str())),
            )
            .set((
                tenant_invitations::status.eq(to_str.as_str()),
                tenant_invitations::accepted_at.eq(accepted_at.map(|v| v as i64)),
            ))
            .execute(&mut *conn)
            .context("transition tenant invitation status")?;
            Ok(rows > 0)
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
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl WorkspaceInvitationRepository for SqliteStorage {
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
            // Same race backstop as the tenant repository: the partial
            // unique index (migration 000061) arbitrates concurrent
            // check-then-insert races and the violation surfaces as the
            // port-contract error (task-110 F2).
            if let Err(e) = diesel::insert_into(workspace_invitations::table)
                .values(&row)
                .execute(&mut *conn)
            {
                if is_workspace_pending_scope_violation(&e) {
                    anyhow::bail!(
                        "a pending invitation for user {} in workspace {} already exists",
                        inv.user_id,
                        inv.workspace_id
                    );
                }
                return Err(anyhow::Error::new(e).context("insert workspace invitation"));
            }
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

    async fn transition_status(
        &self,
        id: &Id,
        from: InvitationStatus,
        to: InvitationStatus,
        accepted_at: Option<u64>,
    ) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let from_str = from.as_str().to_string();
        let to_str = to.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            // Conditional UPDATE — see the tenant variant for the rationale.
            let rows = diesel::update(
                workspace_invitations::table
                    .find(id.as_str())
                    .filter(workspace_invitations::status.eq(from_str.as_str())),
            )
            .set((
                workspace_invitations::status.eq(to_str.as_str()),
                workspace_invitations::accepted_at.eq(accepted_at.map(|v| v as i64)),
            ))
            .execute(&mut *conn)
            .context("transition workspace invitation status")?;
            Ok(rows > 0)
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
    // Both repository traits define create/find_by_id/find_by_token_hash/
    // transition_status/delete; `use super::*` brings both into scope, so the
    // shared method names are ambiguous at call syntax. Disambiguate with
    // trait-qualified calls.
    use gyre_ports::TenantInvitationRepository as TiRepo;
    use gyre_ports::WorkspaceInvitationRepository as WiRepo;

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
        TiRepo::create(&db, &inv).await.unwrap();

        // Duplicate pending invitation for the same (tenant, email) is rejected.
        let dup = TenantInvitation { ..inv.clone() };
        assert!(TiRepo::create(&db, &dup).await.is_err());

        let found = TiRepo::find_by_token_hash(&db, "abc123").await.unwrap().unwrap();
        assert_eq!(found.id.as_str(), "ti1");
        assert_eq!(found.email, "alice@example.com");
        assert_eq!(found.workspace_ids.len(), 2);
        assert_eq!(found.workspace_roles[0], WorkspaceRole::Developer);
        assert_eq!(found.workspace_roles[1], WorkspaceRole::Admin);
        assert_eq!(found.status, InvitationStatus::Pending);

        let listed = TiRepo::list_by_tenant(&db, &Id::new("t1")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(db
            .list_by_tenant(&Id::new("other"))
            .await
            .unwrap()
            .is_empty());

        let claimed = TiRepo::transition_status(
            &db,
            &Id::new("ti1"),
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(1500),
        )
        .await
        .unwrap();
        assert!(claimed, "pending → accepted must claim");
        let accepted = TiRepo::find_by_id(&db, &Id::new("ti1")).await.unwrap().unwrap();
        assert_eq!(accepted.status, InvitationStatus::Accepted);
        assert_eq!(accepted.accepted_at, Some(1500));

        // Single-use guard: the same transition cannot happen twice — the
        // second claim loses because the row is no longer Pending.
        let reclaimed = TiRepo::transition_status(
            &db,
            &Id::new("ti1"),
            InvitationStatus::Pending,
            InvitationStatus::Accepted,
            Some(1600),
        )
        .await
        .unwrap();
        assert!(!reclaimed, "a claimed invitation cannot be re-claimed");
        let still = TiRepo::find_by_id(&db, &Id::new("ti1")).await.unwrap().unwrap();
        assert_eq!(still.status, InvitationStatus::Accepted);
        assert_eq!(still.accepted_at, Some(1500), "loser does not overwrite");

        // After a terminal status, a new invitation for the same email is allowed.
        let reinvite = TenantInvitation {
            id: Id::new("ti2"),
            token_hash: "def456".to_string(),
            ..inv
        };
        TiRepo::create(&db, &reinvite).await.unwrap();
        let pending = TiRepo::list_by_status(&db, InvitationStatus::Pending).await.unwrap();
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
        WiRepo::create(&db, &inv).await.unwrap();

        // Same (workspace, user) pending → rejected.
        assert!(WiRepo::create(&db, &inv).await.is_err());

        // Same user, different workspace → allowed.
        let other_ws = WorkspaceInvitation {
            id: Id::new("wi2"),
            workspace_id: Id::new("ws2"),
            token_hash: "tok2".to_string(),
            ..inv.clone()
        };
        WiRepo::create(&db, &other_ws).await.unwrap();

        let by_user = WiRepo::list_by_user(&db, &Id::new("u2")).await.unwrap();
        assert_eq!(by_user.len(), 2);
        let by_ws = WiRepo::list_by_workspace(&db, &Id::new("ws1")).await.unwrap();
        assert_eq!(by_ws.len(), 1);

        let claimed = WiRepo::transition_status(
            &db,
            &Id::new("wi1"),
            InvitationStatus::Pending,
            InvitationStatus::Declined,
            None,
        )
        .await
        .unwrap();
        assert!(claimed);
        // Non-pending rows cannot be transitioned (single-use guard).
        let reclaimed = WiRepo::transition_status(
            &db,
            &Id::new("wi1"),
            InvitationStatus::Pending,
            InvitationStatus::Declined,
            None,
        )
        .await
        .unwrap();
        assert!(!reclaimed);
        assert_eq!(
            WiRepo::find_by_id(&db, &Id::new("wi1"))
                .await
                .unwrap()
                .unwrap()
                .status,
            InvitationStatus::Declined
        );
    }

    /// The transition precondition is enforced by the UPDATE's WHERE
    /// clause, not by a read-then-write: a row that is Revoked cannot be
    /// flipped to Expired by an expiry-job-style transition, and a missing
    /// id reports `false` rather than an error.
    #[tokio::test]
    async fn transition_status_rejects_wrong_source_status() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("test.db").to_str().unwrap()).unwrap();
        let inv = WorkspaceInvitation {
            id: Id::new("wi-r"),
            tenant_id: Id::new("t1"),
            workspace_id: Id::new("ws1"),
            user_id: Id::new("u2"),
            invited_by: Id::new("u1"),
            role: WorkspaceRole::Viewer,
            status: InvitationStatus::Pending,
            token_hash: "tok-r".to_string(),
            expires_at: 2000,
            created_at: 1000,
            accepted_at: None,
        };
        WiRepo::create(&db, &inv).await.unwrap();
        assert!(
            WiRepo::transition_status(
                &db,
                &Id::new("wi-r"),
                InvitationStatus::Pending,
                InvitationStatus::Revoked,
                None
            )
            .await
            .unwrap()
        );
        // Expiry job fires late: the row is already Revoked — the
        // Pending→Expired transition must not overwrite it.
        assert!(!
            WiRepo::transition_status(
                &db,
                &Id::new("wi-r"),
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None
            )
            .await
            .unwrap()
        );
        assert_eq!(
            WiRepo::find_by_id(&db, &Id::new("wi-r"))
                .await
                .unwrap()
                .unwrap()
                .status,
            InvitationStatus::Revoked
        );
        // Unknown id → false, not an error.
        assert!(!
            WiRepo::transition_status(
                &db,
                &Id::new("nope"),
                InvitationStatus::Pending,
                InvitationStatus::Expired,
                None
            )
            .await
            .unwrap()
        );
    }

    /// Task-110 review F2: the "one pending invitation per (tenant, email)"
    /// port contract must be enforced by the DATABASE (partial unique
    /// index, migration 000061), not only by the adapter's check-then-insert
    /// — two concurrent creates that both pass the check must not both
    /// insert. This test bypasses the repository entirely (raw SQL) so it
    /// fails if the index is missing, and feeds the REAL database errors
    /// to the violation discriminators so a matcher regression (e.g. one
    /// that swallows the token_hash constraint) also fails.
    #[test]
    fn pending_scope_unique_index_enforced_by_database() {
        use diesel::sql_types::Text;
        use diesel::RunQueryDsl;

        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("test.db").to_str().unwrap()).unwrap();
        let mut conn = db.pool.get().unwrap();

        let tenant_insert = "INSERT INTO tenant_invitations \
             (id, tenant_id, email, invited_by, role, workspace_ids, workspace_roles, \
              status, token_hash, expires_at, created_at, accepted_at) \
             VALUES (?, ?, ?, 'u1', 'Member', '[]', '[]', ?, ?, 9999999999, 1, NULL)";
        let ws_insert = "INSERT INTO workspace_invitations \
             (id, tenant_id, workspace_id, user_id, invited_by, role, \
              status, token_hash, expires_at, created_at, accepted_at) \
             VALUES (?, 't1', ?, ?, 'u1', 'Viewer', ?, ?, 9999999999, 1, NULL)";

        // Baseline pending invitation for (t1, a@x.com).
        diesel::sql_query(tenant_insert)
            .bind::<Text, _>("inv-1")
            .bind::<Text, _>("t1")
            .bind::<Text, _>("a@x.com")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("hash-1")
            .execute(&mut conn)
            .unwrap();

        // Second Pending row for the SAME (tenant, email) → the index
        // rejects it at the database level and the discriminator
        // recognizes the violation as the pending-scope contract conflict.
        let err = diesel::sql_query(tenant_insert)
            .bind::<Text, _>("inv-2")
            .bind::<Text, _>("t1")
            .bind::<Text, _>("a@x.com")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("hash-2")
            .execute(&mut conn)
            .unwrap_err();
        assert!(
            is_tenant_pending_scope_violation(&err),
            "pending-scope index did not fire or was not recognized: {err}"
        );

        // A different email for the same tenant is fine — the index is
        // scoped to (tenant, email).
        diesel::sql_query(tenant_insert)
            .bind::<Text, _>("inv-3")
            .bind::<Text, _>("t1")
            .bind::<Text, _>("b@x.com")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("hash-3")
            .execute(&mut conn)
            .unwrap();

        // A TERMINAL row for the same (tenant, email) does not conflict:
        // re-invite after Revoked/Expired/Declined must keep working.
        diesel::sql_query(tenant_insert)
            .bind::<Text, _>("inv-4")
            .bind::<Text, _>("t1")
            .bind::<Text, _>("a@x.com")
            .bind::<Text, _>("Revoked")
            .bind::<Text, _>("hash-4")
            .execute(&mut conn)
            .unwrap();

        // The token_hash unique index is a DIFFERENT constraint: a
        // violation of it must NOT be mistaken for the pending-scope
        // contract (different tenant and email, colliding hash).
        let err = diesel::sql_query(tenant_insert)
            .bind::<Text, _>("inv-5")
            .bind::<Text, _>("t2")
            .bind::<Text, _>("c@x.com")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("hash-1")
            .execute(&mut conn)
            .unwrap_err();
        assert!(
            !is_tenant_pending_scope_violation(&err),
            "token_hash violation misclassified as pending-scope: {err}"
        );

        // Workspace variant: same contract on (workspace_id, user_id).
        diesel::sql_query(ws_insert)
            .bind::<Text, _>("wi-1")
            .bind::<Text, _>("ws1")
            .bind::<Text, _>("u9")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("whash-1")
            .execute(&mut conn)
            .unwrap();
        let err = diesel::sql_query(ws_insert)
            .bind::<Text, _>("wi-2")
            .bind::<Text, _>("ws1")
            .bind::<Text, _>("u9")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("whash-2")
            .execute(&mut conn)
            .unwrap_err();
        assert!(
            is_workspace_pending_scope_violation(&err),
            "workspace pending-scope index did not fire or was not recognized: {err}"
        );
        // Different user, same workspace → fine.
        diesel::sql_query(ws_insert)
            .bind::<Text, _>("wi-3")
            .bind::<Text, _>("ws1")
            .bind::<Text, _>("u8")
            .bind::<Text, _>("Pending")
            .bind::<Text, _>("whash-3")
            .execute(&mut conn)
            .unwrap();
        // Terminal row for the same (workspace, user) → fine.
        diesel::sql_query(ws_insert)
            .bind::<Text, _>("wi-4")
            .bind::<Text, _>("ws1")
            .bind::<Text, _>("u9")
            .bind::<Text, _>("Declined")
            .bind::<Text, _>("whash-4")
            .execute(&mut conn)
            .unwrap();
    }

    /// Task-110 review F2 safety net: migration 000064 (renumbered from
    /// 000061 at the 2026-11-13 rebase onto main head f764bc00) must apply
    /// to a database that ALREADY contains raced duplicate Pending rows
    /// (the exact state the check-then-insert gap allowed) without failing
    /// at CREATE UNIQUE INDEX — the superseded duplicates are marked
    /// Expired (audit-preserving terminal state) and only the newest per
    /// scope survives as Pending.
    #[test]
    fn migration_000064_dedupes_raced_pending_invitations_before_index() {
        use crate::sqlite::MIGRATIONS;
        use diesel::migration::{MigrationConnection, MigrationSource};
        use diesel::sql_types::{Integer, Text};
        use diesel::{Connection, RunQueryDsl};
        use diesel_migrations::MigrationHarness;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut conn = diesel::SqliteConnection::establish(path.to_str().unwrap()).unwrap();

        // Pre-000064 state: every earlier migration applied.
        let all = MIGRATIONS.migrations().unwrap();
        let (pre, target): (Vec<_>, Vec<_>) = all.into_iter().partition(|m| {
            m.name().version().to_string() != "20261113000064"
        });
        assert_eq!(
            target.len(),
            1,
            "000064 not found in embedded migrations"
        );
        conn.setup().unwrap();
        conn.run_migrations(&pre).unwrap();

        // Seed the raced state directly (bypasses the repository's
        // check-then-insert): three Pending rows for (t1, a@x.com) —
        // created_at 100, 200, and a created_at tie broken by id — plus
        // one Pending row for a different email.
        let insert = "INSERT INTO tenant_invitations \
             (id, tenant_id, email, invited_by, role, workspace_ids, workspace_roles, \
              status, token_hash, expires_at, created_at, accepted_at) \
             VALUES (?, 't1', ?, 'u1', 'Member', '[]', '[]', 'Pending', ?, 9999999999, ?, NULL)";
        for (id, email, hash, created) in [
            ("inv-old", "a@x.com", "h-old", 100i32),
            ("inv-new", "a@x.com", "h-new", 200),
            ("inv-tie-b", "a@x.com", "h-tie-b", 200),
            ("inv-other", "b@x.com", "h-other", 50),
        ] {
            diesel::sql_query(insert)
                .bind::<Text, _>(id)
                .bind::<Text, _>(email)
                .bind::<Text, _>(hash)
                .bind::<Integer, _>(created)
                .execute(&mut conn)
                .unwrap();
        }

        // The migration must not fail on the duplicates (CREATE UNIQUE
        // INDEX would abort without the dedupe pass).
        conn.run_migrations(&target).unwrap();

        // `QueryableByName`'s derive (diesel_derives 2.3.7) expands to
        // `Self { field: field, … }`, so clippy::redundant_field_names
        // fires on derive-generated code that inherits neither struct-
        // nor field-level `#[allow]` (same repo-verified quirk as the
        // row-carrier modules in sqlite/mod.rs) — scope the lint off for
        // this small module instead.
        mod rows {
            #![allow(clippy::redundant_field_names)]
            use diesel::sql_types::Text;

            #[derive(diesel::QueryableByName)]
            pub(super) struct StatusRow {
                #[diesel(sql_type = Text)]
                pub(super) id: String,
                #[diesel(sql_type = Text)]
                pub(super) status: String,
            }
        }
        use rows::StatusRow;
        let rows: Vec<StatusRow> =
            diesel::sql_query("SELECT id, status FROM tenant_invitations ORDER BY id")
                .load(&mut conn)
                .unwrap();
        let by_id: std::collections::HashMap<_, _> =
            rows.into_iter().map(|r| (r.id, r.status)).collect();
        // Superseded duplicates are Expired — never deleted (audit):
        // the older created_at (inv-old, 100) and the id-losing side of
        // the created_at tie ("inv-new" < "inv-tie-b", both 200).
        assert_eq!(by_id["inv-old"], "Expired");
        assert_eq!(by_id["inv-new"], "Expired");
        // The single newest per scope (greatest created_at, then greatest
        // id) survives as Pending.
        assert_eq!(by_id["inv-tie-b"], "Pending");
        // Other scopes are untouched.
        assert_eq!(by_id["inv-other"], "Pending");

        // And the index is live after the migration: a new duplicate
        // Pending row for (t1, a@x.com) is rejected by the database.
        let err = diesel::sql_query(insert)
            .bind::<Text, _>("inv-race")
            .bind::<Text, _>("a@x.com")
            .bind::<Text, _>("h-race")
            .bind::<Integer, _>(300)
            .execute(&mut conn)
            .unwrap_err();
        assert!(
            is_tenant_pending_scope_violation(&err),
            "index not enforced after migration: {err}"
        );
    }
}
