use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{WorkspaceMembership, WorkspaceRole};
use gyre_ports::WorkspaceMembershipRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::workspace_memberships;

#[derive(Queryable, Selectable)]
#[diesel(table_name = workspace_memberships)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MembershipRow {
    id: String,
    user_id: String,
    workspace_id: String,
    role: String,
    invited_by: String,
    accepted: i32,
    accepted_at: Option<i64>,
    created_at: i64,
}

impl MembershipRow {
    fn into_membership(self) -> Result<WorkspaceMembership> {
        let role = WorkspaceRole::parse_role(&self.role)
            .ok_or_else(|| anyhow!("unknown workspace role: {}", self.role))?;
        Ok(WorkspaceMembership {
            id: Id::new(self.id),
            user_id: Id::new(self.user_id),
            workspace_id: Id::new(self.workspace_id),
            role,
            invited_by: Id::new(self.invited_by),
            accepted: self.accepted != 0,
            accepted_at: self.accepted_at.map(|v| v as u64),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = workspace_memberships)]
struct NewMembershipRow<'a> {
    id: &'a str,
    user_id: &'a str,
    workspace_id: &'a str,
    role: &'a str,
    invited_by: &'a str,
    accepted: i32,
    accepted_at: Option<i64>,
    created_at: i64,
}

#[async_trait]
impl WorkspaceMembershipRepository for PgStorage {
    async fn create(&self, membership: &WorkspaceMembership) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = membership.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewMembershipRow {
                id: m.id.as_str(),
                user_id: m.user_id.as_str(),
                workspace_id: m.workspace_id.as_str(),
                role: m.role.as_str(),
                invited_by: m.invited_by.as_str(),
                accepted: m.accepted as i32,
                accepted_at: m.accepted_at.map(|v| v as i64),
                created_at: m.created_at as i64,
            };
            diesel::insert_into(workspace_memberships::table)
                .values(&row)
                .on_conflict(workspace_memberships::id)
                .do_update()
                .set((
                    workspace_memberships::role.eq(row.role),
                    workspace_memberships::accepted.eq(row.accepted),
                    workspace_memberships::accepted_at.eq(row.accepted_at),
                ))
                .execute(&mut *conn)
                .context("insert workspace membership")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<WorkspaceMembership>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<WorkspaceMembership>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = workspace_memberships::table
                .find(id.as_str())
                .first::<MembershipRow>(&mut *conn)
                .optional()
                .context("find membership by id")?;
            result.map(MembershipRow::into_membership).transpose()
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<WorkspaceMembership>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceMembership>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = workspace_memberships::table
                .filter(workspace_memberships::workspace_id.eq(wid.as_str()))
                .order(workspace_memberships::created_at.asc())
                .load::<MembershipRow>(&mut *conn)
                .context("list memberships by workspace")?;
            rows.into_iter()
                .map(MembershipRow::into_membership)
                .collect()
        })
        .await?
    }

    async fn list_by_user(&self, user_id: &Id) -> Result<Vec<WorkspaceMembership>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceMembership>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = workspace_memberships::table
                .filter(workspace_memberships::user_id.eq(uid.as_str()))
                .order(workspace_memberships::created_at.asc())
                .load::<MembershipRow>(&mut *conn)
                .context("list memberships by user")?;
            rows.into_iter()
                .map(MembershipRow::into_membership)
                .collect()
        })
        .await?
    }

    async fn find_by_user_and_workspace(
        &self,
        user_id: &Id,
        workspace_id: &Id,
    ) -> Result<Option<WorkspaceMembership>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<WorkspaceMembership>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = workspace_memberships::table
                .filter(workspace_memberships::user_id.eq(uid.as_str()))
                .filter(workspace_memberships::workspace_id.eq(wid.as_str()))
                .first::<MembershipRow>(&mut *conn)
                .optional()
                .context("find membership by user and workspace")?;
            result.map(MembershipRow::into_membership).transpose()
        })
        .await?
    }

    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(workspace_memberships::table.find(id.as_str()))
                .set(workspace_memberships::role.eq(role.as_str()))
                .execute(&mut *conn)
                .context("update membership role")?;
            Ok(())
        })
        .await?
    }

    async fn accept(&self, id: &Id, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(workspace_memberships::table.find(id.as_str()))
                .set((
                    workspace_memberships::accepted.eq(1_i32),
                    workspace_memberships::accepted_at.eq(now as i64),
                ))
                .execute(&mut *conn)
                .context("accept membership")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(workspace_memberships::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete workspace membership")?;
            Ok(())
        })
        .await?
    }
}
