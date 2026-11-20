use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{ScimGroup, ScimGroupMember, ScimWorkspaceMapping};
use gyre_ports::{ScimGroupRepository, ScimMappingRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{scim_group_members, scim_groups, scim_workspace_mappings};

#[derive(Queryable, Selectable)]
#[diesel(table_name = scim_groups)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct GroupRow {
    id: String,
    external_id: Option<String>,
    display_name: String,
    created_at: i64,
    updated_at: i64,
}

impl GroupRow {
    fn into_group(self) -> ScimGroup {
        ScimGroup {
            id: Id::new(self.id),
            external_id: self.external_id.unwrap_or_default(),
            display_name: self.display_name,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = scim_groups)]
struct NewGroupRow<'a> {
    id: &'a str,
    external_id: Option<&'a str>,
    display_name: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = scim_group_members)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MemberRow {
    group_id: String,
    user_id: String,
    added_at: i64,
    display: Option<String>,
}

impl MemberRow {
    fn into_member(self) -> ScimGroupMember {
        ScimGroupMember {
            group_id: Id::new(self.group_id),
            user_id: Id::new(self.user_id),
            added_at: self.added_at as u64,
            display: self.display,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = scim_group_members)]
struct NewMemberRow<'a> {
    group_id: &'a str,
    user_id: &'a str,
    added_at: i64,
    display: Option<&'a str>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = scim_workspace_mappings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MappingRow {
    workspace_id: String,
    idp_group: String,
    workspace_role: String,
    updated_at: i64,
}

impl MappingRow {
    fn into_mapping(self) -> Result<ScimWorkspaceMapping> {
        Ok(ScimWorkspaceMapping {
            workspace_id: Id::new(self.workspace_id),
            idp_group: self.idp_group,
            workspace_role: gyre_domain::WorkspaceRole::parse_role(&self.workspace_role)
                .ok_or_else(|| {
                    anyhow!(
                        "unknown workspace role in SCIM mapping: {}",
                        self.workspace_role
                    )
                })?,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = scim_workspace_mappings)]
struct NewMappingRow<'a> {
    workspace_id: &'a str,
    idp_group: &'a str,
    workspace_role: &'a str,
    updated_at: i64,
}

/// True when `err` is the unique index on `scim_groups.external_id` — the
/// database-level enforcement of the "one group per externalId" port
/// contract (mirrors the invitation adapters' scope-violation matchers).
fn is_external_id_violation(err: &diesel::result::Error) -> bool {
    use diesel::result::{DatabaseErrorKind, Error};
    match err {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, e) => {
            e.message().contains("idx_scim_groups_external_id")
        }
        _ => false,
    }
}

#[async_trait]
impl ScimGroupRepository for PgStorage {
    async fn create(&self, group: &ScimGroup) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let g = group.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewGroupRow {
                id: g.id.as_str(),
                external_id: if g.external_id.is_empty() {
                    None
                } else {
                    Some(g.external_id.as_str())
                },
                display_name: g.display_name.as_str(),
                created_at: g.created_at as i64,
                updated_at: g.updated_at as i64,
            };
            diesel::insert_into(scim_groups::table)
                .values(&row)
                .execute(&mut *conn)
                .map_err(|e| {
                    if is_external_id_violation(&e) {
                        anyhow!(
                            "SCIM group with external_id {:?} already exists",
                            g.external_id
                        )
                    } else {
                        anyhow::Error::from(e).context("insert SCIM group")
                    }
                })?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<ScimGroup>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<ScimGroup>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_groups::table
                .find(id.as_str())
                .first::<GroupRow>(&mut *conn)
                .optional()
                .context("find SCIM group by id")
                .map(|r| r.map(GroupRow::into_group))
        })
        .await?
    }

    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<ScimGroup>> {
        let pool = Arc::clone(&self.pool);
        let ext = external_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<ScimGroup>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_groups::table
                .filter(scim_groups::external_id.eq(ext.as_str()))
                .first::<GroupRow>(&mut *conn)
                .optional()
                .context("find SCIM group by external id")
                .map(|r| r.map(GroupRow::into_group))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<ScimGroup>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<ScimGroup>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_groups::table
                .order(scim_groups::created_at.asc())
                .then_order_by(scim_groups::id.asc())
                .load::<GroupRow>(&mut *conn)
                .context("list SCIM groups")
                .map(|rows| rows.into_iter().map(GroupRow::into_group).collect())
        })
        .await?
    }

    async fn update(&self, group: &ScimGroup) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let g = group.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(scim_groups::table.find(g.id.as_str()))
                .set((
                    scim_groups::display_name.eq(g.display_name.as_str()),
                    scim_groups::updated_at.eq(g.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update SCIM group")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            conn.transaction(|conn| -> Result<()> {
                diesel::delete(scim_group_members::table)
                    .filter(scim_group_members::group_id.eq(id.as_str()))
                    .execute(conn)
                    .context("delete SCIM group members")?;
                diesel::delete(scim_groups::table.find(id.as_str()))
                    .execute(conn)
                    .context("delete SCIM group")?;
                Ok(())
            })
        })
        .await?
    }

    async fn list_members(&self, group_id: &Id) -> Result<Vec<ScimGroupMember>> {
        let pool = Arc::clone(&self.pool);
        let gid = group_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ScimGroupMember>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_group_members::table
                .filter(scim_group_members::group_id.eq(gid.as_str()))
                .order(scim_group_members::added_at.asc())
                .then_order_by(scim_group_members::user_id.asc())
                .load::<MemberRow>(&mut *conn)
                .context("list SCIM group members")
                .map(|rows| rows.into_iter().map(MemberRow::into_member).collect())
        })
        .await?
    }

    /// Add one member. Idempotent row-wise: re-pushing an existing
    /// `(group, user)` membership never duplicates the row — when the
    /// push carries a `display`, the upsert refreshes the stored one
    /// (the IdP's current view of the name); a push without one leaves
    /// any previously stored `display` untouched (partial pushes must
    /// not erase data they did not carry).
    async fn add_member(&self, member: &ScimGroupMember) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = member.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewMemberRow {
                group_id: m.group_id.as_str(),
                user_id: m.user_id.as_str(),
                added_at: m.added_at as i64,
                display: m.display.as_deref(),
            };
            if m.display.is_some() {
                diesel::insert_into(scim_group_members::table)
                    .values(&row)
                    .on_conflict((scim_group_members::group_id, scim_group_members::user_id))
                    .do_update()
                    .set(scim_group_members::display.eq(m.display.as_deref()))
                    .execute(&mut *conn)
                    .context("add SCIM group member")?;
            } else {
                diesel::insert_into(scim_group_members::table)
                    .values(&row)
                    .on_conflict_do_nothing()
                    .execute(&mut *conn)
                    .context("add SCIM group member")?;
            }
            Ok(())
        })
        .await?
    }

    async fn remove_member(&self, group_id: &Id, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let gid = group_id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(scim_group_members::table)
                .filter(scim_group_members::group_id.eq(gid.as_str()))
                .filter(scim_group_members::user_id.eq(uid.as_str()))
                .execute(&mut *conn)
                .context("remove SCIM group member")?;
            Ok(())
        })
        .await?
    }

    async fn replace_members(&self, group_id: &Id, members: &[ScimGroupMember]) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let gid = group_id.clone();
        let members = members.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            conn.transaction(|conn| -> Result<()> {
                diesel::delete(scim_group_members::table)
                    .filter(scim_group_members::group_id.eq(gid.as_str()))
                    .execute(conn)
                    .context("clear SCIM group members")?;
                for m in &members {
                    diesel::insert_into(scim_group_members::table)
                        .values(NewMemberRow {
                            group_id: m.group_id.as_str(),
                            user_id: m.user_id.as_str(),
                            added_at: m.added_at as i64,
                            display: m.display.as_deref(),
                        })
                        .execute(conn)
                        .context("insert SCIM group member")?;
                }
                Ok(())
            })
        })
        .await?
    }
}

#[async_trait]
impl ScimMappingRepository for PgStorage {
    async fn replace_for_workspace(
        &self,
        workspace_id: &Id,
        mappings: &[ScimWorkspaceMapping],
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        let mappings = mappings.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            conn.transaction(|conn| -> Result<()> {
                diesel::delete(scim_workspace_mappings::table)
                    .filter(scim_workspace_mappings::workspace_id.eq(wid.as_str()))
                    .execute(conn)
                    .context("clear SCIM workspace mappings")?;
                for m in &mappings {
                    diesel::insert_into(scim_workspace_mappings::table)
                        .values(NewMappingRow {
                            workspace_id: m.workspace_id.as_str(),
                            idp_group: m.idp_group.as_str(),
                            workspace_role: m.workspace_role.as_str(),
                            updated_at: m.updated_at as i64,
                        })
                        .execute(conn)
                        .context("insert SCIM workspace mapping")?;
                }
                Ok(())
            })
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<ScimWorkspaceMapping>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ScimWorkspaceMapping>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_workspace_mappings::table
                .filter(scim_workspace_mappings::workspace_id.eq(wid.as_str()))
                .order(scim_workspace_mappings::idp_group.asc())
                .load::<MappingRow>(&mut *conn)
                .context("list SCIM workspace mappings")
                .and_then(|rows| rows.into_iter().map(MappingRow::into_mapping).collect())
        })
        .await?
    }

    async fn list_by_idp_group(&self, idp_group: &str) -> Result<Vec<ScimWorkspaceMapping>> {
        let pool = Arc::clone(&self.pool);
        let key = idp_group.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<ScimWorkspaceMapping>> {
            let mut conn = pool.get().context("get db connection")?;
            scim_workspace_mappings::table
                .filter(scim_workspace_mappings::idp_group.eq(key.as_str()))
                .order(scim_workspace_mappings::workspace_id.asc())
                .load::<MappingRow>(&mut *conn)
                .context("list SCIM mappings by idp group")
                .and_then(|rows| rows.into_iter().map(MappingRow::into_mapping).collect())
        })
        .await?
    }
}
