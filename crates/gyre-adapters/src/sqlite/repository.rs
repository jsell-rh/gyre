use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{RepoStatus, Repository};
use gyre_ports::RepoRepository;
use std::str::FromStr;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::repositories;

/// Deserialize a repo budget override from the `budget_json` column.
/// A corrupt budget document must not take the repo read path down: the
/// repo then inherits the workspace limit — the coarser ceiling still
/// bounds spending, and the warn names the row for repair.
fn parse_budget_json(
    json: &Option<String>,
    repo_id: &str,
) -> Option<gyre_domain::BudgetConfig> {
    json.as_deref().and_then(|json| {
        match serde_json::from_str::<gyre_domain::BudgetConfig>(json) {
            Ok(b) => Some(b),
            Err(e) => {
                tracing::warn!(
                    repo_id,
                    error = %e,
                    "corrupt repositories.budget_json; repo inherits workspace budget"
                );
                None
            }
        }
    })
}

/// Serialize a repo budget override for the `budget_json` column.
/// `None` (inherit the workspace limit) stays NULL.
fn budget_to_json(budget: &Option<gyre_domain::BudgetConfig>) -> Option<String> {
    budget
        .as_ref()
        .map(|b| serde_json::to_string(b).expect("BudgetConfig is always JSON-serializable"))
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = repositories)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct RepositoryRow {
    id: String,
    name: String,
    path: String,
    default_branch: String,
    created_at: i64,
    is_mirror: i32,
    mirror_url: Option<String>,
    mirror_interval_secs: Option<i64>,
    last_mirror_sync: Option<i64>,
    #[allow(dead_code)]
    tenant_id: String,
    workspace_id: String,
    description: Option<String>,
    status: String,
    updated_at: i64,
    max_agents: Option<i32>,
    max_agent_iterations: Option<i32>,
    budget_json: Option<String>,
}

impl From<RepositoryRow> for Repository {
    fn from(r: RepositoryRow) -> Self {
        let budget = parse_budget_json(&r.budget_json, &r.id);
        Repository {
            id: Id::new(r.id),
            name: r.name,
            path: r.path,
            default_branch: r.default_branch,
            created_at: r.created_at as u64,
            is_mirror: r.is_mirror != 0,
            mirror_url: r.mirror_url,
            mirror_interval_secs: r.mirror_interval_secs.map(|v| v as u64),
            last_mirror_sync: r.last_mirror_sync.map(|v| v as u64),
            workspace_id: Id::new(r.workspace_id),
            description: r.description,
            status: RepoStatus::from_str(&r.status).unwrap_or_else(|_| {
                tracing::warn!("unknown repo status {:?}, defaulting to Active", r.status);
                RepoStatus::Active
            }),
            updated_at: r.updated_at as u64,
            max_agents: r.max_agents.map(|v| v as u32),
            max_agent_iterations: r.max_agent_iterations.map(|v| v as u32),
            budget,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = repositories)]
struct NewRepositoryRow<'a> {
    id: &'a str,
    name: &'a str,
    path: &'a str,
    default_branch: &'a str,
    created_at: i64,
    is_mirror: i32,
    mirror_url: Option<&'a str>,
    mirror_interval_secs: Option<i64>,
    last_mirror_sync: Option<i64>,
    tenant_id: &'a str,
    workspace_id: &'a str,
    description: Option<&'a str>,
    status: &'a str,
    updated_at: i64,
    max_agents: Option<i32>,
    max_agent_iterations: Option<i32>,
    budget_json: Option<String>,
}

#[async_trait]
impl RepoRepository for SqliteStorage {
    async fn create(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let status_str = r.status.to_string();
            let row = NewRepositoryRow {
                id: r.id.as_str(),
                name: &r.name,
                path: &r.path,
                default_branch: &r.default_branch,
                created_at: r.created_at as i64,
                is_mirror: r.is_mirror as i32,
                mirror_url: r.mirror_url.as_deref(),
                mirror_interval_secs: r.mirror_interval_secs.map(|v| v as i64),
                last_mirror_sync: r.last_mirror_sync.map(|v| v as i64),
                tenant_id: "default",
                workspace_id: r.workspace_id.as_str(),
                description: r.description.as_deref(),
                status: &status_str,
                updated_at: r.updated_at as i64,
                max_agents: r.max_agents.map(|v| v as i32),
                max_agent_iterations: r.max_agent_iterations.map(|v| v as i32),
                budget_json: budget_to_json(&r.budget),
            };
            diesel::insert_into(repositories::table)
                .values(&row)
                .on_conflict(repositories::id)
                .do_update()
                .set((
                    repositories::name.eq(row.name),
                    repositories::path.eq(row.path),
                    repositories::default_branch.eq(row.default_branch),
                    repositories::is_mirror.eq(row.is_mirror),
                    repositories::mirror_url.eq(row.mirror_url),
                    repositories::mirror_interval_secs.eq(row.mirror_interval_secs),
                    repositories::last_mirror_sync.eq(row.last_mirror_sync),
                    repositories::workspace_id.eq(row.workspace_id),
                    repositories::description.eq(row.description),
                    repositories::status.eq(row.status),
                    repositories::updated_at.eq(row.updated_at),
                    repositories::max_agents.eq(row.max_agents),
                    repositories::max_agent_iterations.eq(row.max_agent_iterations),
                    repositories::budget_json.eq(row.budget_json.as_deref()),
                ))
                .execute(&mut *conn)
                .context("insert repository")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .filter(repositories::id.eq(id.as_str()))
                .first::<RepositoryRow>(&mut *conn)
                .optional()
                .context("find repository by id")?;
            Ok(result.map(Repository::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let status_str = r.status.to_string();
            diesel::update(repositories::table.find(r.id.as_str()))
                .set((
                    repositories::name.eq(&r.name),
                    repositories::path.eq(&r.path),
                    repositories::default_branch.eq(&r.default_branch),
                    repositories::is_mirror.eq(r.is_mirror as i32),
                    repositories::mirror_url.eq(r.mirror_url.as_deref()),
                    repositories::mirror_interval_secs.eq(r.mirror_interval_secs.map(|v| v as i64)),
                    repositories::last_mirror_sync.eq(r.last_mirror_sync.map(|v| v as i64)),
                    repositories::workspace_id.eq(r.workspace_id.as_str()),
                    repositories::description.eq(r.description.as_deref()),
                    repositories::status.eq(&status_str),
                    repositories::updated_at.eq(r.updated_at as i64),
                    repositories::max_agents.eq(r.max_agents.map(|v| v as i32)),
                    repositories::max_agent_iterations.eq(r.max_agent_iterations.map(|v| v as i32)),
                    repositories::budget_json.eq(budget_to_json(&r.budget)),
                ))
                .execute(&mut *conn)
                .context("update repository")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(repositories::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete repository")?;
            Ok(())
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .filter(repositories::workspace_id.eq(workspace_id.as_str()))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories by workspace")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn find_by_name_and_workspace(
        &self,
        workspace_id: &Id,
        name: &str,
    ) -> Result<Option<Repository>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repositories::table
                .filter(repositories::workspace_id.eq(workspace_id.as_str()))
                .filter(repositories::name.eq(&name))
                .first::<RepositoryRow>(&mut *conn)
                .optional()
                .context("find repository by name and workspace")?;
            Ok(result.map(Repository::from))
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_repo(id: &str, workspace_id: &str) -> Repository {
        Repository::new(
            Id::new(id),
            Id::new(workspace_id),
            format!("repo-{}", id),
            format!("/repos/{}", id),
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        let r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.name, "repo-r1");
        assert_eq!(found.workspace_id, Id::new("ws1"));
        assert_eq!(found.default_branch, "main");
        assert_eq!(found.status, RepoStatus::Active);
        assert!(found.description.is_none());
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        assert!(RepoRepository::find_by_id(&s, &Id::new("nope"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_repositories() {
        let (_tmp, s) = setup();
        RepoRepository::create(&s, &make_repo("r1", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "ws1"))
            .await
            .unwrap();
        assert_eq!(RepoRepository::list(&s).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_by_workspace() {
        let (_tmp, s) = setup();
        RepoRepository::create(&s, &make_repo("r1", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r3", "ws2"))
            .await
            .unwrap();

        let ws1_repos = RepoRepository::list_by_workspace(&s, &Id::new("ws1"))
            .await
            .unwrap();
        assert_eq!(ws1_repos.len(), 2);
        let ws2_repos = RepoRepository::list_by_workspace(&s, &Id::new("ws2"))
            .await
            .unwrap();
        assert_eq!(ws2_repos.len(), 1);
    }

    #[tokio::test]
    async fn update_repository() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        r.default_branch = "develop".to_string();
        r.path = "/new/path".to_string();
        RepoRepository::update(&s, &r).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.default_branch, "develop");
        assert_eq!(found.path, "/new/path");
    }

    #[tokio::test]
    async fn delete_repository() {
        let (_tmp, s) = setup();
        let r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        RepoRepository::delete(&s, &r.id).await.unwrap();
        assert!(RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn archive_and_unarchive() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();

        r.archive();
        RepoRepository::update(&s, &r).await.unwrap();

        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, RepoStatus::Archived);

        r.unarchive();
        RepoRepository::update(&s, &r).await.unwrap();

        let found2 = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found2.status, RepoStatus::Active);
    }

    #[tokio::test]
    async fn description_persists() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        r.description = Some("my description".to_string());
        RepoRepository::create(&s, &r).await.unwrap();

        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.description.as_deref(), Some("my description"));
    }

    /// TASK-119 (agent-runtime.md §4 Cascade): the repo-level budget
    /// override persists through create and update — None inherits the
    /// workspace limit, a set budget round-trips field by field.
    #[tokio::test]
    async fn repo_budget_override_persists() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r-budget", "ws1");
        r.budget = Some(gyre_domain::BudgetConfig {
            max_tokens_per_day: Some(100_000),
            max_cost_per_day: Some(12.5),
            max_concurrent_agents: Some(3),
            max_agent_lifetime_secs: Some(3600),
        });
        RepoRepository::create(&s, &r).await.unwrap();

        let mut found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        let budget = found.budget.clone().expect("budget must persist through create");
        assert_eq!(budget.max_tokens_per_day, Some(100_000));
        assert!((budget.max_cost_per_day.unwrap() - 12.5).abs() < 1e-9);
        assert_eq!(budget.max_concurrent_agents, Some(3));
        assert_eq!(budget.max_agent_lifetime_secs, Some(3600));

        // Update path: replace the override, then clear it.
        found.budget = Some(gyre_domain::BudgetConfig {
            max_tokens_per_day: Some(50_000),
            ..Default::default()
        });
        RepoRepository::update(&s, &found).await.unwrap();
        let found2 = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            found2.budget.as_ref().unwrap().max_tokens_per_day,
            Some(50_000)
        );

        let mut cleared = found2;
        cleared.budget = None;
        RepoRepository::update(&s, &cleared).await.unwrap();
        let found3 = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found3.budget, None, "clearing the override must persist");
    }

    /// TASK-118 (agent-runtime.md §1 Phase 4/Phase 7, repo-lifecycle §3):
    /// the per-repo agent limits persist through create and update — NULL
    /// inherits, values round-trip.
    #[tokio::test]
    async fn max_agent_limits_persist() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r-limits", "ws1");
        r.max_agents = Some(2);
        r.max_agent_iterations = Some(4);
        RepoRepository::create(&s, &r).await.unwrap();

        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.max_agents, Some(2));
        assert_eq!(found.max_agent_iterations, Some(4));

        // NULL = inherit: a repo without overrides round-trips None.
        let plain = make_repo("r-plain", "ws1");
        RepoRepository::create(&s, &plain).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &plain.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.max_agents, None);
        assert_eq!(found.max_agent_iterations, None);

        // Update path persists changed values.
        let mut updated = found;
        updated.max_agents = Some(5);
        updated.max_agent_iterations = Some(10);
        RepoRepository::update(&s, &updated).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &updated.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.max_agents, Some(5));
        assert_eq!(found.max_agent_iterations, Some(10));
    }
}
