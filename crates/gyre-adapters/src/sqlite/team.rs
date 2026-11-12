use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Team, TeamMembership};
use gyre_ports::TeamRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{team_memberships, teams};

#[derive(Queryable, Selectable)]
#[diesel(table_name = teams)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct TeamRow {
    id: String,
    workspace_id: String,
    name: String,
    slug: String,
    description: Option<String>,
    lead: String,
    created_at: i64,
}

impl TeamRow {
    fn into_team(self) -> Team {
        Team {
            id: Id::new(self.id),
            workspace_id: Id::new(self.workspace_id),
            name: self.name,
            slug: self.slug,
            description: self.description,
            lead: Id::new(self.lead),
            created_at: self.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = teams)]
struct NewTeamRow<'a> {
    id: &'a str,
    workspace_id: &'a str,
    name: &'a str,
    slug: &'a str,
    description: Option<&'a str>,
    lead: &'a str,
    created_at: i64,
}

#[derive(Queryable)]
#[diesel(table_name = team_memberships)]
struct TeamMembershipRow {
    team_id: String,
    user_id: String,
    added_at: i64,
}

impl TeamMembershipRow {
    fn into_membership(self) -> TeamMembership {
        TeamMembership {
            team_id: Id::new(self.team_id),
            user_id: Id::new(self.user_id),
            added_at: self.added_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = team_memberships)]
struct NewTeamMembershipRow<'a> {
    team_id: &'a str,
    user_id: &'a str,
    added_at: i64,
}

/// Map a diesel error to a duplicate-slug / duplicate-member failure with a
/// stable message so every adapter surfaces the same contract violation.
fn duplicate_label(e: diesel::result::Error) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("idx_teams_workspace_slug")
        // SQLite surfaces a composite unique index as the column list.
        || msg.contains("teams.workspace_id, teams.slug")
    {
        anyhow::anyhow!("team slug already exists in workspace")
    } else if msg.contains("team_memberships_pkey")
        || (msg.contains("UNIQUE") && msg.contains("team_memberships"))
    {
        anyhow::anyhow!("user is already a member of the team")
    } else {
        anyhow::Error::new(e)
    }
}

#[async_trait]
impl TeamRepository for SqliteStorage {
    async fn create(&self, team: &Team) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = team.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewTeamRow {
                id: t.id.as_str(),
                workspace_id: t.workspace_id.as_str(),
                name: &t.name,
                slug: &t.slug,
                description: t.description.as_deref(),
                lead: t.lead.as_str(),
                created_at: t.created_at as i64,
            };
            // Upsert by id (matches the pre-task-122 behavior); a slug
            // collision with a DIFFERENT team violates
            // idx_teams_workspace_slug and fails the call — the port's
            // duplicate-slug contract.
            diesel::insert_into(teams::table)
                .values(&row)
                .on_conflict(teams::id)
                .do_update()
                .set((
                    teams::name.eq(&t.name),
                    teams::slug.eq(&t.slug),
                    teams::description.eq(t.description.as_deref()),
                    teams::lead.eq(t.lead.as_str()),
                ))
                .execute(&mut *conn)
                .map_err(duplicate_label)
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
            Ok(result.map(TeamRow::into_team))
        })
        .await?
    }

    async fn find_by_slug(&self, workspace_id: &Id, slug: &str) -> Result<Option<Team>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.to_string();
        let slug = slug.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Team>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = teams::table
                .filter(teams::workspace_id.eq(&wid))
                .filter(teams::slug.eq(&slug))
                .first::<TeamRow>(&mut *conn)
                .optional()
                .context("find team by slug")?;
            Ok(result.map(TeamRow::into_team))
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Team>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = teams::table
                .filter(teams::workspace_id.eq(&wid))
                .order(teams::created_at.asc())
                .load::<TeamRow>(&mut *conn)
                .context("list teams by workspace")?;
            Ok(rows.into_iter().map(TeamRow::into_team).collect())
        })
        .await?
    }

    async fn update(&self, team: &Team) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = team.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(teams::table.find(t.id.as_str()))
                .set((
                    teams::name.eq(&t.name),
                    teams::slug.eq(&t.slug),
                    teams::description.eq(t.description.as_deref()),
                    teams::lead.eq(t.lead.as_str()),
                ))
                .execute(&mut *conn)
                .map_err(duplicate_label)
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
            // Memberships go with the team — there is no team left to be a
            // member of.
            diesel::delete(team_memberships::table.filter(team_memberships::team_id.eq(id.as_str())))
                .execute(&mut *conn)
                .context("delete team memberships")?;
            diesel::delete(teams::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete team")?;
            Ok(())
        })
        .await?
    }

    async fn add_member(&self, membership: &TeamMembership) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = membership.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewTeamMembershipRow {
                team_id: m.team_id.as_str(),
                user_id: m.user_id.as_str(),
                added_at: m.added_at as i64,
            };
            diesel::insert_into(team_memberships::table)
                .values(&row)
                .execute(&mut *conn)
                .map_err(duplicate_label)
                .context("add team member")?;
            Ok(())
        })
        .await?
    }

    async fn remove_member(&self, team_id: &Id, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tid = team_id.to_string();
        let uid = user_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                team_memberships::table
                    .filter(team_memberships::team_id.eq(&tid))
                    .filter(team_memberships::user_id.eq(&uid)),
            )
            .execute(&mut *conn)
            .context("remove team member")?;
            Ok(())
        })
        .await?
    }

    async fn list_members(&self, team_id: &Id) -> Result<Vec<TeamMembership>> {
        let pool = Arc::clone(&self.pool);
        let tid = team_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<TeamMembership>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = team_memberships::table
                .filter(team_memberships::team_id.eq(&tid))
                .order(team_memberships::added_at.asc())
                .load::<TeamMembershipRow>(&mut *conn)
                .context("list team members")?;
            Ok(rows.into_iter().map(TeamMembershipRow::into_membership).collect())
        })
        .await?
    }

    async fn list_for_user(&self, workspace_id: &Id, user_id: &Id) -> Result<Vec<Team>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.to_string();
        let uid = user_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Team>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = teams::table
                .inner_join(team_memberships::table)
                .filter(teams::workspace_id.eq(&wid))
                .filter(team_memberships::user_id.eq(&uid))
                .order(teams::created_at.asc())
                .select(TeamRow::as_select())
                .load::<TeamRow>(&mut *conn)
                .context("list teams for user")?;
            Ok(rows.into_iter().map(TeamRow::into_team).collect())
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

    fn team(id: &str, ws: &str, slug: &str, lead: &str) -> Team {
        Team::new(Id::new(id), Id::new(ws), "Platform", slug, Id::new(lead), 1000)
    }

    #[tokio::test]
    async fn create_find_by_slug_round_trips_every_field() {
        let (_tmp, s) = setup();
        let mut t = team("t-1", "ws-1", "platform", "u-lead");
        t.description = Some("Owns the platform".to_string());
        s.create(&t).await.unwrap();

        let got = s
            .find_by_slug(&Id::new("ws-1"), "platform")
            .await
            .unwrap()
            .expect("team must resolve by slug");
        assert_eq!(got.id.as_str(), "t-1");
        assert_eq!(got.slug, "platform");
        assert_eq!(got.lead.as_str(), "u-lead");
        assert_eq!(got.description.as_deref(), Some("Owns the platform"));
        assert_eq!(got.created_at, 1000);

        // find_by_slug is workspace-scoped: the same slug in another
        // workspace does not collide.
        assert!(s.find_by_slug(&Id::new("ws-2"), "platform").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn create_rejects_duplicate_slug_within_workspace() {
        let (_tmp, s) = setup();
        s.create(&team("t-1", "ws-1", "platform", "u-lead")).await.unwrap();
        // Same slug, same workspace, different id — the port's duplicate
        // contract (the unique index is the race-safe backstop).
        let err = s
            .create(&team("t-2", "ws-1", "platform", "u-lead"))
            .await
            .expect_err("duplicate slug must fail");
        assert!(
            format!("{err:#}").contains("team slug already exists in workspace"),
            "wrong error: {err:#}"
        );
        // The same slug in a different workspace is fine.
        s.create(&team("t-3", "ws-2", "platform", "u-lead")).await.unwrap();
    }

    #[tokio::test]
    async fn update_rejects_slug_collision_with_other_team() {
        let (_tmp, s) = setup();
        s.create(&team("t-1", "ws-1", "platform", "u-lead")).await.unwrap();
        s.create(&team("t-2", "ws-1", "sre", "u-lead")).await.unwrap();

        let mut t2 = s.find_by_id(&Id::new("t-2")).await.unwrap().unwrap();
        t2.slug = "platform".to_string();
        let err = s
            .update(&t2)
            .await
            .expect_err("renaming onto another team's slug must fail");
        assert!(
            format!("{err:#}").contains("team slug already exists in workspace"),
            "wrong error: {err:#}"
        );

        // Keeping its own slug is not a collision.
        let mut t2 = s.find_by_id(&Id::new("t-2")).await.unwrap().unwrap();
        t2.name = "SRE Team".to_string();
        s.update(&t2).await.unwrap();
    }

    #[tokio::test]
    async fn membership_add_list_remove_and_duplicate_rejection() {
        let (_tmp, s) = setup();
        s.create(&team("t-1", "ws-1", "platform", "u-lead")).await.unwrap();

        let m1 = TeamMembership::new(Id::new("t-1"), Id::new("u-1"), 2000);
        let m2 = TeamMembership::new(Id::new("t-1"), Id::new("u-2"), 3000);
        s.add_member(&m1).await.unwrap();
        s.add_member(&m2).await.unwrap();

        // Duplicate membership fails — the port's duplicate contract.
        let err = s
            .add_member(&TeamMembership::new(Id::new("t-1"), Id::new("u-1"), 4000))
            .await
            .expect_err("duplicate membership must fail");
        assert!(
            format!("{err:#}").contains("already a member"),
            "wrong error: {err:#}"
        );

        // ordered by added_at
        let members = s.list_members(&Id::new("t-1")).await.unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].user_id.as_str(), "u-1");
        assert_eq!(members[0].added_at, 2000);
        assert_eq!(members[1].user_id.as_str(), "u-2");
        assert_eq!(members[1].added_at, 3000);

        // list_for_user is workspace- and membership-scoped
        let teams = s
            .list_for_user(&Id::new("ws-1"), &Id::new("u-1"))
            .await
            .unwrap();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].id.as_str(), "t-1");
        assert!(s.list_for_user(&Id::new("ws-2"), &Id::new("u-1")).await.unwrap().is_empty());

        s.remove_member(&Id::new("t-1"), &Id::new("u-1")).await.unwrap();
        // Idempotent remove.
        s.remove_member(&Id::new("t-1"), &Id::new("u-1")).await.unwrap();
        assert_eq!(s.list_members(&Id::new("t-1")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn delete_team_removes_its_memberships() {
        let (_tmp, s) = setup();
        s.create(&team("t-1", "ws-1", "platform", "u-lead")).await.unwrap();
        s.add_member(&TeamMembership::new(Id::new("t-1"), Id::new("u-1"), 2000))
            .await
            .unwrap();

        s.delete(&Id::new("t-1")).await.unwrap();
        assert!(s.find_by_id(&Id::new("t-1")).await.unwrap().is_none());
        assert!(s.list_members(&Id::new("t-1")).await.unwrap().is_empty());
    }
}
