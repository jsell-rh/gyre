use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Team;
use gyre_ports::TeamRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::teams;

#[derive(Queryable, Selectable)]
#[diesel(table_name = teams)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct TeamRow {
    id: String,
    workspace_id: String,
    name: String,
    description: Option<String>,
    member_ids: String,
    created_at: i64,
}

impl TeamRow {
    fn into_team(self) -> Result<Team> {
        let member_ids: Vec<String> = serde_json::from_str(&self.member_ids).unwrap_or_default();
        Ok(Team {
            id: Id::new(self.id),
            workspace_id: Id::new(self.workspace_id),
            name: self.name,
            description: self.description,
            member_ids: member_ids.into_iter().map(Id::new).collect(),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = teams)]
struct NewTeamRow<'a> {
    id: &'a str,
    workspace_id: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    member_ids: String,
    created_at: i64,
}

#[async_trait]
impl TeamRepository for PgStorage {
    async fn create(&self, team: &Team) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = team.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let member_ids_json = serde_json::to_string(
                &t.member_ids
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            )?;
            let row = NewTeamRow {
                id: t.id.as_str(),
                workspace_id: t.workspace_id.as_str(),
                name: &t.name,
                description: t.description.as_deref(),
                member_ids: member_ids_json.clone(),
                created_at: t.created_at as i64,
            };
            diesel::insert_into(teams::table)
                .values(&row)
                .on_conflict(teams::id)
                .do_update()
                .set((
                    teams::name.eq(&t.name),
                    teams::description.eq(t.description.as_deref()),
                    teams::member_ids.eq(&member_ids_json),
                ))
                .execute(&mut *conn)
                .context("insert team")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Team>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Team>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = teams::table
                .find(id.as_str())
                .first::<TeamRow>(&mut *conn)
                .optional()
                .context("find team by id")?;
            result.map(TeamRow::into_team).transpose()
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Team>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = teams::table
                .filter(teams::workspace_id.eq(wid.as_str()))
                .order(teams::created_at.asc())
                .load::<TeamRow>(&mut *conn)
                .context("list teams by workspace")?;
            rows.into_iter().map(TeamRow::into_team).collect()
        })
        .await?
    }

    async fn update(&self, team: &Team) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = team.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let member_ids_json = serde_json::to_string(
                &t.member_ids
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            )?;
            diesel::update(teams::table.find(t.id.as_str()))
                .set((
                    teams::name.eq(&t.name),
                    teams::description.eq(t.description.as_deref()),
                    teams::member_ids.eq(&member_ids_json),
                ))
                .execute(&mut *conn)
                .context("update team")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(teams::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete team")?;
            Ok(())
        })
        .await?
    }
}
