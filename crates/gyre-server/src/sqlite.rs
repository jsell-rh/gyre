//! SQLite-backed implementations of all port traits.
//!
//! Uses `rusqlite` with a `Mutex<Connection>` and `tokio::task::spawn_blocking`
//! to bridge async callers to synchronous SQLite operations.

use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentCommit, AgentStatus, AgentWorktree, AnalyticsEvent, AuditEvent, AuditEventType,
    CostEntry, MergeQueueEntry, MergeQueueEntryStatus, MergeRequest, MrStatus, NetworkPeer,
    Project, Repository, Review, ReviewComment, ReviewDecision, Task, TaskPriority, TaskStatus,
    User, UserRole,
};
use gyre_ports::{
    ActivityQuery, ActivityRepository, AgentCommitRepository, AgentRepository, AnalyticsRepository,
    ApiKeyRepository, AuditRepository, CostRepository, MergeQueueRepository,
    MergeRequestRepository, NetworkPeerRepository, ProjectRepository, RepoRepository,
    ReviewRepository, TaskRepository, UserRepository, WorktreeRepository,
};
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

// ───────────────────────────────────────────────
// SqliteDb — shared connection wrapper
// ───────────────────────────────────────────────

#[derive(Clone)]
pub struct SqliteDb {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDb {
    /// Open (or create) a SQLite database at `path`. Pass `:memory:` for in-memory.
    pub fn open(path: &str) -> Result<Self> {
        let conn =
            Connection::open(path).with_context(|| format!("Failed to open SQLite at {path}"))?;

        // Enable WAL mode for better write concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let db = SqliteDb {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.run_migrations()?;
        Ok(db)
    }

    fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    /// Run migrations in order. Idempotent.
    fn run_migrations(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch(MIGRATIONS)?;
            Ok(())
        })
    }
}

// ───────────────────────────────────────────────
// Schema migrations
// ───────────────────────────────────────────────

const MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS repos (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    default_branch TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    parent_id TEXT,
    current_task_id TEXT,
    lifetime_budget_secs INTEGER,
    spawned_at INTEGER NOT NULL,
    last_heartbeat INTEGER
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL,
    priority TEXT NOT NULL,
    assigned_to TEXT,
    parent_task_id TEXT,
    labels TEXT NOT NULL,
    branch TEXT,
    pr_link TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_requests (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL,
    title TEXT NOT NULL,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    status TEXT NOT NULL,
    author_agent_id TEXT,
    reviewers TEXT NOT NULL,
    diff_stats TEXT,
    has_conflicts INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS review_comments (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL,
    author_agent_id TEXT NOT NULL,
    body TEXT NOT NULL,
    file_path TEXT,
    line_number INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS reviews (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL,
    reviewer_agent_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    body TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_queue (
    id TEXT PRIMARY KEY,
    merge_request_id TEXT NOT NULL,
    priority INTEGER NOT NULL,
    status TEXT NOT NULL,
    enqueued_at INTEGER NOT NULL,
    processed_at INTEGER,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    external_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT,
    roles TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    key TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_commits (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_worktrees (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    task_id TEXT,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS analytics_events (
    id TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    agent_id TEXT,
    properties TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cost_entries (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    task_id TEXT,
    cost_type TEXT NOT NULL,
    amount REAL NOT NULL,
    currency TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    path TEXT,
    details TEXT NOT NULL,
    pid INTEGER,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS network_peers (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    wireguard_pubkey TEXT NOT NULL,
    endpoint TEXT,
    allowed_ips TEXT NOT NULL,
    registered_at INTEGER NOT NULL,
    last_seen INTEGER
);

CREATE TABLE IF NOT EXISTS activity_events (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    description TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);
"#;

// ───────────────────────────────────────────────
// Helper: run blocking rusqlite ops on tokio thread pool
// ───────────────────────────────────────────────

macro_rules! blocking {
    ($db:expr, $f:expr) => {{
        let db = $db.clone();
        tokio::task::spawn_blocking(move || $f(&db))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e}"))?
    }};
}

// ───────────────────────────────────────────────
// Enum serialization helpers
// ───────────────────────────────────────────────

fn agent_status_to_str(s: &AgentStatus) -> &'static str {
    match s {
        AgentStatus::Idle => "Idle",
        AgentStatus::Active => "Active",
        AgentStatus::Blocked => "Blocked",
        AgentStatus::Error => "Error",
        AgentStatus::Dead => "Dead",
    }
}

fn agent_status_from_str(s: &str) -> AgentStatus {
    match s {
        "Active" => AgentStatus::Active,
        "Blocked" => AgentStatus::Blocked,
        "Error" => AgentStatus::Error,
        "Dead" => AgentStatus::Dead,
        _ => AgentStatus::Idle,
    }
}

fn task_status_to_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "Backlog",
        TaskStatus::InProgress => "InProgress",
        TaskStatus::Review => "Review",
        TaskStatus::Done => "Done",
        TaskStatus::Blocked => "Blocked",
    }
}

fn task_status_from_str(s: &str) -> TaskStatus {
    match s {
        "InProgress" => TaskStatus::InProgress,
        "Review" => TaskStatus::Review,
        "Done" => TaskStatus::Done,
        "Blocked" => TaskStatus::Blocked,
        _ => TaskStatus::Backlog,
    }
}

fn task_priority_to_str(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::Low => "Low",
        TaskPriority::Medium => "Medium",
        TaskPriority::High => "High",
        TaskPriority::Critical => "Critical",
    }
}

fn task_priority_from_str(s: &str) -> TaskPriority {
    match s {
        "Low" => TaskPriority::Low,
        "High" => TaskPriority::High,
        "Critical" => TaskPriority::Critical,
        _ => TaskPriority::Medium,
    }
}

fn mr_status_to_str(s: &MrStatus) -> &'static str {
    match s {
        MrStatus::Open => "Open",
        MrStatus::Approved => "Approved",
        MrStatus::Merged => "Merged",
        MrStatus::Closed => "Closed",
    }
}

fn mr_status_from_str(s: &str) -> MrStatus {
    match s {
        "Approved" => MrStatus::Approved,
        "Merged" => MrStatus::Merged,
        "Closed" => MrStatus::Closed,
        _ => MrStatus::Open,
    }
}

fn mq_status_to_str(s: &MergeQueueEntryStatus) -> &'static str {
    match s {
        MergeQueueEntryStatus::Queued => "Queued",
        MergeQueueEntryStatus::Processing => "Processing",
        MergeQueueEntryStatus::Merged => "Merged",
        MergeQueueEntryStatus::Failed => "Failed",
        MergeQueueEntryStatus::Cancelled => "Cancelled",
    }
}

fn mq_status_from_str(s: &str) -> MergeQueueEntryStatus {
    match s {
        "Processing" => MergeQueueEntryStatus::Processing,
        "Merged" => MergeQueueEntryStatus::Merged,
        "Failed" => MergeQueueEntryStatus::Failed,
        "Cancelled" => MergeQueueEntryStatus::Cancelled,
        _ => MergeQueueEntryStatus::Queued,
    }
}

fn review_decision_to_str(d: &ReviewDecision) -> &'static str {
    match d {
        ReviewDecision::Approved => "Approved",
        ReviewDecision::ChangesRequested => "ChangesRequested",
    }
}

fn review_decision_from_str(s: &str) -> ReviewDecision {
    match s {
        "ChangesRequested" => ReviewDecision::ChangesRequested,
        _ => ReviewDecision::Approved,
    }
}

// ───────────────────────────────────────────────
// ProjectRepository
// ───────────────────────────────────────────────

#[async_trait]
impl ProjectRepository for SqliteDb {
    async fn create(&self, project: &Project) -> Result<()> {
        let p = project.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO projects (id, name, description, created_at, updated_at) VALUES (?1,?2,?3,?4,?5)",
                    params![p.id.as_str(), p.name, p.description, p.created_at as i64, p.updated_at as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Project>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(Project {
                        id: Id::new(row.get::<_, String>(0)?),
                        name: row.get(1)?,
                        description: row.get(2)?,
                        created_at: row.get::<_, i64>(3)? as u64,
                        updated_at: row.get::<_, i64>(4)? as u64,
                    }))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<Project>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, description, created_at, updated_at FROM projects",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(Project {
                        id: Id::new(row.get::<_, String>(0)?),
                        name: row.get(1)?,
                        description: row.get(2)?,
                        created_at: row.get::<_, i64>(3)? as u64,
                        updated_at: row.get::<_, i64>(4)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, project: &Project) -> Result<()> {
        ProjectRepository::create(self, project).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM projects WHERE id = ?1", params![id.as_str()])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// RepoRepository
// ───────────────────────────────────────────────

#[async_trait]
impl RepoRepository for SqliteDb {
    async fn create(&self, repo: &Repository) -> Result<()> {
        let r = repo.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO repos (id, project_id, name, path, default_branch, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![r.id.as_str(), r.project_id.as_str(), r.name, r.path, r.default_branch, r.created_at as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, name, path, default_branch, created_at FROM repos WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(Repository {
                        id: Id::new(row.get::<_, String>(0)?),
                        project_id: Id::new(row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        path: row.get(3)?,
                        default_branch: row.get(4)?,
                        created_at: row.get::<_, i64>(5)? as u64,
                    }))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, name, path, default_branch, created_at FROM repos",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(Repository {
                        id: Id::new(row.get::<_, String>(0)?),
                        project_id: Id::new(row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        path: row.get(3)?,
                        default_branch: row.get(4)?,
                        created_at: row.get::<_, i64>(5)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_project(&self, project_id: &Id) -> Result<Vec<Repository>> {
        let pid = project_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, name, path, default_branch, created_at FROM repos WHERE project_id = ?1",
                )?;
                let rows = stmt.query_map(params![pid.as_str()], |row| {
                    Ok(Repository {
                        id: Id::new(row.get::<_, String>(0)?),
                        project_id: Id::new(row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        path: row.get(3)?,
                        default_branch: row.get(4)?,
                        created_at: row.get::<_, i64>(5)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        RepoRepository::create(self, repo).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM repos WHERE id = ?1", params![id.as_str()])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// AgentRepository
// ───────────────────────────────────────────────

fn row_to_agent(row: &rusqlite::Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: Id::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        status: agent_status_from_str(&row.get::<_, String>(2)?),
        parent_id: row.get::<_, Option<String>>(3)?.map(Id::new),
        current_task_id: row.get::<_, Option<String>>(4)?.map(Id::new),
        lifetime_budget_secs: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        spawned_at: row.get::<_, i64>(6)? as u64,
        last_heartbeat: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
    })
}

#[async_trait]
impl AgentRepository for SqliteDb {
    async fn create(&self, agent: &Agent) -> Result<()> {
        let a = agent.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO agents (id, name, status, parent_id, current_task_id, lifetime_budget_secs, spawned_at, last_heartbeat) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        a.id.as_str(),
                        a.name,
                        agent_status_to_str(&a.status),
                        a.parent_id.as_ref().map(|id| id.as_str()),
                        a.current_task_id.as_ref().map(|id| id.as_str()),
                        a.lifetime_budget_secs.map(|v| v as i64),
                        a.spawned_at as i64,
                        a.last_heartbeat.map(|v| v as i64),
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs, spawned_at, last_heartbeat FROM agents WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_agent(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>> {
        let name = name.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs, spawned_at, last_heartbeat FROM agents WHERE name = ?1",
                )?;
                let mut rows = stmt.query(params![name])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_agent(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<Agent>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs, spawned_at, last_heartbeat FROM agents",
                )?;
                let rows = stmt.query_map([], row_to_agent)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>> {
        let status_str = agent_status_to_str(status).to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs, spawned_at, last_heartbeat FROM agents WHERE status = ?1",
                )?;
                let rows = stmt.query_map(params![status_str], row_to_agent)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, agent: &Agent) -> Result<()> {
        AgentRepository::create(self, agent).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM agents WHERE id = ?1", params![id.as_str()])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// TaskRepository
// ───────────────────────────────────────────────

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let labels_json: String = row.get(7)?;
    let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
    Ok(Task {
        id: Id::new(row.get::<_, String>(0)?),
        title: row.get(1)?,
        description: row.get(2)?,
        status: task_status_from_str(&row.get::<_, String>(3)?),
        priority: task_priority_from_str(&row.get::<_, String>(4)?),
        assigned_to: row.get::<_, Option<String>>(5)?.map(Id::new),
        parent_task_id: row.get::<_, Option<String>>(6)?.map(Id::new),
        labels,
        branch: row.get(8)?,
        pr_link: row.get(9)?,
        created_at: row.get::<_, i64>(10)? as u64,
        updated_at: row.get::<_, i64>(11)? as u64,
    })
}

#[async_trait]
impl TaskRepository for SqliteDb {
    async fn create(&self, task: &Task) -> Result<()> {
        let t = task.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let labels = serde_json::to_string(&t.labels)?;
                conn.execute(
                    "INSERT OR REPLACE INTO tasks (id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![
                        t.id.as_str(), t.title, t.description,
                        task_status_to_str(&t.status), task_priority_to_str(&t.priority),
                        t.assigned_to.as_ref().map(|id| id.as_str()),
                        t.parent_task_id.as_ref().map(|id| id.as_str()),
                        labels, t.branch, t.pr_link,
                        t.created_at as i64, t.updated_at as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at FROM tasks WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_task(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<Task>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at FROM tasks",
                )?;
                let rows = stmt.query_map([], row_to_task)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>> {
        let status_str = task_status_to_str(status).to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at FROM tasks WHERE status = ?1",
                )?;
                let rows = stmt.query_map(params![status_str], row_to_task)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at FROM tasks WHERE assigned_to = ?1",
                )?;
                let rows = stmt.query_map(params![aid.as_str()], row_to_task)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>> {
        let pid = parent_task_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, description, status, priority, assigned_to, parent_task_id, labels, branch, pr_link, created_at, updated_at FROM tasks WHERE parent_task_id = ?1",
                )?;
                let rows = stmt.query_map(params![pid.as_str()], row_to_task)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, task: &Task) -> Result<()> {
        TaskRepository::create(self, task).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM tasks WHERE id = ?1", params![id.as_str()])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// MergeRequestRepository
// ───────────────────────────────────────────────

fn row_to_mr(row: &rusqlite::Row) -> rusqlite::Result<MergeRequest> {
    let reviewers_json: String = row.get(7)?;
    let reviewers: Vec<Id> = serde_json::from_str::<Vec<String>>(&reviewers_json)
        .unwrap_or_default()
        .into_iter()
        .map(Id::new)
        .collect();
    let diff_stats_json: Option<String> = row.get(8)?;
    let diff_stats = diff_stats_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let has_conflicts_int: Option<i64> = row.get(9)?;
    Ok(MergeRequest {
        id: Id::new(row.get::<_, String>(0)?),
        repository_id: Id::new(row.get::<_, String>(1)?),
        title: row.get(2)?,
        source_branch: row.get(3)?,
        target_branch: row.get(4)?,
        status: mr_status_from_str(&row.get::<_, String>(5)?),
        author_agent_id: row.get::<_, Option<String>>(6)?.map(Id::new),
        reviewers,
        diff_stats,
        has_conflicts: has_conflicts_int.map(|v| v != 0),
        created_at: row.get::<_, i64>(10)? as u64,
        updated_at: row.get::<_, i64>(11)? as u64,
    })
}

#[async_trait]
impl MergeRequestRepository for SqliteDb {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        let m = mr.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let reviewers: Vec<String> = m.reviewers.iter().map(|id| id.as_str().to_string()).collect();
                let reviewers_json = serde_json::to_string(&reviewers)?;
                let diff_stats_json = m.diff_stats.as_ref().map(serde_json::to_string).transpose()?;
                let has_conflicts = m.has_conflicts.map(|v| if v { 1i64 } else { 0 });
                conn.execute(
                    "INSERT OR REPLACE INTO merge_requests (id, repository_id, title, source_branch, target_branch, status, author_agent_id, reviewers, diff_stats, has_conflicts, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![
                        m.id.as_str(), m.repository_id.as_str(), m.title,
                        m.source_branch, m.target_branch, mr_status_to_str(&m.status),
                        m.author_agent_id.as_ref().map(|id| id.as_str()),
                        reviewers_json, diff_stats_json, has_conflicts,
                        m.created_at as i64, m.updated_at as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, repository_id, title, source_branch, target_branch, status, author_agent_id, reviewers, diff_stats, has_conflicts, created_at, updated_at FROM merge_requests WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_mr(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, repository_id, title, source_branch, target_branch, status, author_agent_id, reviewers, diff_stats, has_conflicts, created_at, updated_at FROM merge_requests",
                )?;
                let rows = stmt.query_map([], row_to_mr)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>> {
        let status_str = mr_status_to_str(status).to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, repository_id, title, source_branch, target_branch, status, author_agent_id, reviewers, diff_stats, has_conflicts, created_at, updated_at FROM merge_requests WHERE status = ?1",
                )?;
                let rows = stmt.query_map(params![status_str], row_to_mr)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>> {
        let rid = repository_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, repository_id, title, source_branch, target_branch, status, author_agent_id, reviewers, diff_stats, has_conflicts, created_at, updated_at FROM merge_requests WHERE repository_id = ?1",
                )?;
                let rows = stmt.query_map(params![rid.as_str()], row_to_mr)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, mr: &MergeRequest) -> Result<()> {
        MergeRequestRepository::create(self, mr).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "DELETE FROM merge_requests WHERE id = ?1",
                    params![id.as_str()],
                )?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// ReviewRepository
// ───────────────────────────────────────────────

#[async_trait]
impl ReviewRepository for SqliteDb {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()> {
        let c = comment.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO review_comments (id, merge_request_id, author_agent_id, body, file_path, line_number, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        c.id.as_str(), c.merge_request_id.as_str(), c.author_agent_id,
                        c.body, c.file_path, c.line_number.map(|v| v as i64), c.created_at as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>> {
        let mr_id = mr_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, merge_request_id, author_agent_id, body, file_path, line_number, created_at FROM review_comments WHERE merge_request_id = ?1 ORDER BY created_at ASC",
                )?;
                let rows = stmt.query_map(params![mr_id.as_str()], |row| {
                    Ok(ReviewComment {
                        id: Id::new(row.get::<_, String>(0)?),
                        merge_request_id: Id::new(row.get::<_, String>(1)?),
                        author_agent_id: row.get(2)?,
                        body: row.get(3)?,
                        file_path: row.get(4)?,
                        line_number: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                        created_at: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn submit_review(&self, review: &Review) -> Result<()> {
        let r = review.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO reviews (id, merge_request_id, reviewer_agent_id, decision, body, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        r.id.as_str(), r.merge_request_id.as_str(), r.reviewer_agent_id,
                        review_decision_to_str(&r.decision), r.body, r.created_at as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>> {
        let mr_id = mr_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, merge_request_id, reviewer_agent_id, decision, body, created_at FROM reviews WHERE merge_request_id = ?1 ORDER BY created_at ASC",
                )?;
                let rows = stmt.query_map(params![mr_id.as_str()], |row| {
                    Ok(Review {
                        id: Id::new(row.get::<_, String>(0)?),
                        merge_request_id: Id::new(row.get::<_, String>(1)?),
                        reviewer_agent_id: row.get(2)?,
                        decision: review_decision_from_str(&row.get::<_, String>(3)?),
                        body: row.get(4)?,
                        created_at: row.get::<_, i64>(5)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn is_approved(&self, mr_id: &Id) -> Result<bool> {
        let reviews = self.list_reviews(mr_id).await?;
        if reviews.is_empty() {
            return Ok(false);
        }
        let has_changes_requested = reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::ChangesRequested);
        if has_changes_requested {
            return Ok(false);
        }
        Ok(reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::Approved))
    }
}

// ───────────────────────────────────────────────
// MergeQueueRepository
// ───────────────────────────────────────────────

fn row_to_mq(row: &rusqlite::Row) -> rusqlite::Result<MergeQueueEntry> {
    Ok(MergeQueueEntry {
        id: Id::new(row.get::<_, String>(0)?),
        merge_request_id: Id::new(row.get::<_, String>(1)?),
        priority: row.get::<_, i64>(2)? as u32,
        status: mq_status_from_str(&row.get::<_, String>(3)?),
        enqueued_at: row.get::<_, i64>(4)? as u64,
        processed_at: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        error_message: row.get(6)?,
    })
}

#[async_trait]
impl MergeQueueRepository for SqliteDb {
    async fn enqueue(&self, entry: &MergeQueueEntry) -> Result<()> {
        let e = entry.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO merge_queue (id, merge_request_id, priority, status, enqueued_at, processed_at, error_message) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        e.id.as_str(), e.merge_request_id.as_str(), e.priority as i64,
                        mq_status_to_str(&e.status), e.enqueued_at as i64,
                        e.processed_at.map(|v| v as i64), e.error_message,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn next_pending(&self) -> Result<Option<MergeQueueEntry>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message FROM merge_queue WHERE status = 'Queued' ORDER BY priority DESC, enqueued_at ASC LIMIT 1",
                )?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_mq(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn update_status(
        &self,
        id: &Id,
        status: MergeQueueEntryStatus,
        error: Option<String>,
    ) -> Result<()> {
        let id = id.clone();
        let status_str = mq_status_to_str(&status).to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "UPDATE merge_queue SET status = ?1, error_message = ?2 WHERE id = ?3",
                    params![status_str, error, id.as_str()],
                )?;
                Ok(())
            })
        })
    }

    async fn list_queue(&self) -> Result<Vec<MergeQueueEntry>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message FROM merge_queue WHERE status NOT IN ('Merged','Failed','Cancelled') ORDER BY priority DESC, enqueued_at ASC",
                )?;
                let rows = stmt.query_map([], row_to_mq)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn cancel(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "UPDATE merge_queue SET status = 'Cancelled' WHERE id = ?1 AND status NOT IN ('Merged','Failed','Cancelled')",
                    params![id.as_str()],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeQueueEntry>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, merge_request_id, priority, status, enqueued_at, processed_at, error_message FROM merge_queue WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_mq(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }
}

// ───────────────────────────────────────────────
// UserRepository
// ───────────────────────────────────────────────

fn row_to_user(row: &rusqlite::Row) -> rusqlite::Result<User> {
    let roles_json: String = row.get(4)?;
    let role_strs: Vec<String> = serde_json::from_str(&roles_json).unwrap_or_default();
    let roles = role_strs
        .iter()
        .filter_map(|s| UserRole::from_str(s))
        .collect();
    Ok(User {
        id: Id::new(row.get::<_, String>(0)?),
        external_id: row.get(1)?,
        name: row.get(2)?,
        email: row.get(3)?,
        roles,
        created_at: row.get::<_, i64>(5)? as u64,
        updated_at: row.get::<_, i64>(6)? as u64,
    })
}

#[async_trait]
impl UserRepository for SqliteDb {
    async fn create(&self, user: &User) -> Result<()> {
        let u = user.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let roles: Vec<&str> = u.roles.iter().map(|r| r.as_str()).collect();
                let roles_json = serde_json::to_string(&roles)?;
                conn.execute(
                    "INSERT OR REPLACE INTO users (id, external_id, name, email, roles, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![u.id.as_str(), u.external_id, u.name, u.email, roles_json, u.created_at as i64, u.updated_at as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<User>> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, external_id, name, email, roles, created_at, updated_at FROM users WHERE id = ?1",
                )?;
                let mut rows = stmt.query(params![id.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_user(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>> {
        let eid = external_id.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, external_id, name, email, roles, created_at, updated_at FROM users WHERE external_id = ?1",
                )?;
                let mut rows = stmt.query(params![eid])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_user(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn list(&self) -> Result<Vec<User>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, external_id, name, email, roles, created_at, updated_at FROM users",
                )?;
                let rows = stmt.query_map([], row_to_user)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn update(&self, user: &User) -> Result<()> {
        UserRepository::create(self, user).await
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM users WHERE id = ?1", params![id.as_str()])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// ApiKeyRepository
// ───────────────────────────────────────────────

#[async_trait]
impl ApiKeyRepository for SqliteDb {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()> {
        let key = key.to_string();
        let uid = user_id.clone();
        let name = name.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO api_keys (key, user_id, name) VALUES (?1,?2,?3)",
                    params![key, uid.as_str(), name],
                )?;
                Ok(())
            })
        })
    }

    async fn find_user_id(&self, key: &str) -> Result<Option<Id>> {
        let key = key.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare("SELECT user_id FROM api_keys WHERE key = ?1")?;
                let mut rows = stmt.query(params![key])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(Id::new(row.get::<_, String>(0)?)))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let key = key.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM api_keys WHERE key = ?1", params![key])?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// AgentCommitRepository
// ───────────────────────────────────────────────

#[async_trait]
impl AgentCommitRepository for SqliteDb {
    async fn record(&self, mapping: &AgentCommit) -> Result<()> {
        let ac = mapping.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO agent_commits (id, agent_id, repository_id, commit_sha, branch, timestamp) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![ac.id.as_str(), ac.agent_id.as_str(), ac.repository_id.as_str(), ac.commit_sha, ac.branch, ac.timestamp as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp FROM agent_commits WHERE agent_id = ?1",
                )?;
                let rows = stmt.query_map(params![aid.as_str()], |row| {
                    Ok(AgentCommit {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        repository_id: Id::new(row.get::<_, String>(2)?),
                        commit_sha: row.get(3)?,
                        branch: row.get(4)?,
                        timestamp: row.get::<_, i64>(5)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>> {
        let rid = repo_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp FROM agent_commits WHERE repository_id = ?1",
                )?;
                let rows = stmt.query_map(params![rid.as_str()], |row| {
                    Ok(AgentCommit {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        repository_id: Id::new(row.get::<_, String>(2)?),
                        commit_sha: row.get(3)?,
                        branch: row.get(4)?,
                        timestamp: row.get::<_, i64>(5)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>> {
        let sha = sha.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, repository_id, commit_sha, branch, timestamp FROM agent_commits WHERE commit_sha = ?1",
                )?;
                let mut rows = stmt.query(params![sha])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(AgentCommit {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        repository_id: Id::new(row.get::<_, String>(2)?),
                        commit_sha: row.get(3)?,
                        branch: row.get(4)?,
                        timestamp: row.get::<_, i64>(5)? as u64,
                    }))
                } else {
                    Ok(None)
                }
            })
        })
    }
}

// ───────────────────────────────────────────────
// WorktreeRepository
// ───────────────────────────────────────────────

#[async_trait]
impl WorktreeRepository for SqliteDb {
    async fn create(&self, worktree: &AgentWorktree) -> Result<()> {
        let wt = worktree.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO agent_worktrees (id, agent_id, repository_id, task_id, branch, path, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        wt.id.as_str(), wt.agent_id.as_str(), wt.repository_id.as_str(),
                        wt.task_id.as_ref().map(|id| id.as_str()),
                        wt.branch, wt.path, wt.created_at as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentWorktree>> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, repository_id, task_id, branch, path, created_at FROM agent_worktrees WHERE agent_id = ?1",
                )?;
                let rows = stmt.query_map(params![aid.as_str()], |row| {
                    Ok(AgentWorktree {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        repository_id: Id::new(row.get::<_, String>(2)?),
                        task_id: row.get::<_, Option<String>>(3)?.map(Id::new),
                        branch: row.get(4)?,
                        path: row.get(5)?,
                        created_at: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentWorktree>> {
        let rid = repo_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, repository_id, task_id, branch, path, created_at FROM agent_worktrees WHERE repository_id = ?1",
                )?;
                let rows = stmt.query_map(params![rid.as_str()], |row| {
                    Ok(AgentWorktree {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        repository_id: Id::new(row.get::<_, String>(2)?),
                        task_id: row.get::<_, Option<String>>(3)?.map(Id::new),
                        branch: row.get(4)?,
                        path: row.get(5)?,
                        created_at: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "DELETE FROM agent_worktrees WHERE id = ?1",
                    params![id.as_str()],
                )?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// AnalyticsRepository
// ───────────────────────────────────────────────

#[async_trait]
impl AnalyticsRepository for SqliteDb {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        let e = event.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let props = serde_json::to_string(&e.properties)?;
                conn.execute(
                    "INSERT OR REPLACE INTO analytics_events (id, event_name, agent_id, properties, timestamp) VALUES (?1,?2,?3,?4,?5)",
                    params![e.id.as_str(), e.event_name, e.agent_id, props, e.timestamp as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>> {
        let event_name = event_name.map(|s| s.to_string());
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let sql = "SELECT id, event_name, agent_id, properties, timestamp FROM analytics_events WHERE (?1 IS NULL OR event_name = ?1) AND (?2 IS NULL OR timestamp >= ?2) ORDER BY timestamp DESC LIMIT ?3";
                let mut stmt = conn.prepare(sql)?;
                let rows = stmt.query_map(
                    params![event_name, since.map(|s| s as i64), limit as i64],
                    |row| {
                        let props_str: String = row.get(3)?;
                        let properties = serde_json::from_str(&props_str)
                            .unwrap_or(serde_json::Value::Null);
                        Ok(AnalyticsEvent {
                            id: Id::new(row.get::<_, String>(0)?),
                            event_name: row.get(1)?,
                            agent_id: row.get(2)?,
                            properties,
                            timestamp: row.get::<_, i64>(4)? as u64,
                        })
                    },
                )?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64> {
        let event_name = event_name.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM analytics_events WHERE event_name = ?1 AND timestamp >= ?2 AND timestamp <= ?3",
                    params![event_name, since as i64, until as i64],
                    |row| row.get(0),
                )?;
                Ok(count as u64)
            })
        })
    }

    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>> {
        let event_name = event_name.to_string();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                // SQLite date formatting: strftime('%Y-%m-%d', timestamp, 'unixepoch')
                let mut stmt = conn.prepare(
                    "SELECT strftime('%Y-%m-%d', timestamp, 'unixepoch') as day, COUNT(*) FROM analytics_events WHERE event_name = ?1 AND timestamp >= ?2 AND timestamp <= ?3 GROUP BY day ORDER BY day ASC",
                )?;
                let rows = stmt.query_map(params![event_name, since as i64, until as i64], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }
}

// ───────────────────────────────────────────────
// CostRepository
// ───────────────────────────────────────────────

#[async_trait]
impl CostRepository for SqliteDb {
    async fn record(&self, entry: &CostEntry) -> Result<()> {
        let e = entry.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO cost_entries (id, agent_id, task_id, cost_type, amount, currency, timestamp) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        e.id.as_str(), e.agent_id.as_str(),
                        e.task_id.as_ref().map(|id| id.as_str()),
                        e.cost_type, e.amount, e.currency, e.timestamp as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, task_id, cost_type, amount, currency, timestamp FROM cost_entries WHERE agent_id = ?1 AND (?2 IS NULL OR timestamp >= ?2) ORDER BY timestamp DESC",
                )?;
                let rows = stmt.query_map(params![aid.as_str(), since.map(|s| s as i64)], |row| {
                    Ok(CostEntry {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        task_id: row.get::<_, Option<String>>(2)?.map(Id::new),
                        cost_type: row.get(3)?,
                        amount: row.get(4)?,
                        currency: row.get(5)?,
                        timestamp: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>> {
        let tid = task_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, task_id, cost_type, amount, currency, timestamp FROM cost_entries WHERE task_id = ?1 ORDER BY timestamp DESC",
                )?;
                let rows = stmt.query_map(params![tid.as_str()], |row| {
                    Ok(CostEntry {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        task_id: row.get::<_, Option<String>>(2)?.map(Id::new),
                        cost_type: row.get(3)?,
                        amount: row.get(4)?,
                        currency: row.get(5)?,
                        timestamp: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let total: f64 = conn.query_row(
                    "SELECT COALESCE(SUM(amount), 0.0) FROM cost_entries WHERE agent_id = ?1",
                    params![aid.as_str()],
                    |row| row.get(0),
                )?;
                Ok(total)
            })
        })
    }

    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let total: f64 = conn.query_row(
                    "SELECT COALESCE(SUM(amount), 0.0) FROM cost_entries WHERE timestamp >= ?1 AND timestamp <= ?2",
                    params![since as i64, until as i64],
                    |row| row.get(0),
                )?;
                Ok(total)
            })
        })
    }
}

// ───────────────────────────────────────────────
// AuditRepository
// ───────────────────────────────────────────────

#[async_trait]
impl AuditRepository for SqliteDb {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        let e = event.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let details = serde_json::to_string(&e.details)?;
                conn.execute(
                    "INSERT OR REPLACE INTO audit_events (id, agent_id, event_type, path, details, pid, timestamp) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        e.id.as_str(), e.agent_id.as_str(), e.event_type.as_str(),
                        e.path, details, e.pid.map(|v| v as i64), e.timestamp as i64,
                    ],
                )?;
                Ok(())
            })
        })
    }

    async fn query(
        &self,
        agent_id: Option<&str>,
        event_type: Option<&str>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let agent_id = agent_id.map(|s| s.to_string());
        let event_type = event_type.map(|s| s.to_string());
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, event_type, path, details, pid, timestamp FROM audit_events WHERE (?1 IS NULL OR agent_id = ?1) AND (?2 IS NULL OR event_type = ?2) AND (?3 IS NULL OR timestamp >= ?3) AND (?4 IS NULL OR timestamp <= ?4) ORDER BY timestamp DESC LIMIT ?5",
                )?;
                let rows = stmt.query_map(
                    params![agent_id, event_type, since.map(|s| s as i64), until.map(|u| u as i64), limit as i64],
                    |row| {
                        let details_str: String = row.get(4)?;
                        let details = serde_json::from_str(&details_str)
                            .unwrap_or(serde_json::Value::Null);
                        Ok(AuditEvent {
                            id: Id::new(row.get::<_, String>(0)?),
                            agent_id: Id::new(row.get::<_, String>(1)?),
                            event_type: AuditEventType::from_str(&row.get::<_, String>(2)?),
                            path: row.get(3)?,
                            details,
                            pid: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                            timestamp: row.get::<_, i64>(6)? as u64,
                        })
                    },
                )?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn count(&self) -> Result<u64> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
                Ok(count as u64)
            })
        })
    }

    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT event_type, COUNT(*) FROM audit_events GROUP BY event_type ORDER BY COUNT(*) DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, event_type, path, details, pid, timestamp FROM audit_events WHERE timestamp > ?1 ORDER BY timestamp ASC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![since as i64, limit as i64], |row| {
                    let details_str: String = row.get(4)?;
                    let details = serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Null);
                    Ok(AuditEvent {
                        id: Id::new(row.get::<_, String>(0)?),
                        agent_id: Id::new(row.get::<_, String>(1)?),
                        event_type: AuditEventType::from_str(&row.get::<_, String>(2)?),
                        path: row.get(3)?,
                        details,
                        pid: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                        timestamp: row.get::<_, i64>(6)? as u64,
                    })
                })?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }
}

// ───────────────────────────────────────────────
// NetworkPeerRepository
// ───────────────────────────────────────────────

fn row_to_peer(row: &rusqlite::Row) -> rusqlite::Result<NetworkPeer> {
    let allowed_ips_json: String = row.get(4)?;
    let allowed_ips: Vec<String> = serde_json::from_str(&allowed_ips_json).unwrap_or_default();
    Ok(NetworkPeer {
        id: Id::new(row.get::<_, String>(0)?),
        agent_id: Id::new(row.get::<_, String>(1)?),
        wireguard_pubkey: row.get(2)?,
        endpoint: row.get(3)?,
        allowed_ips,
        registered_at: row.get::<_, i64>(5)? as u64,
        last_seen: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
    })
}

#[async_trait]
impl NetworkPeerRepository for SqliteDb {
    async fn register(&self, peer: &NetworkPeer) -> Result<()> {
        let p = peer.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let ips = serde_json::to_string(&p.allowed_ips)?;
                conn.execute(
                    "INSERT OR REPLACE INTO network_peers (id, agent_id, wireguard_pubkey, endpoint, allowed_ips, registered_at, last_seen) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![p.id.as_str(), p.agent_id.as_str(), p.wireguard_pubkey, p.endpoint, ips, p.registered_at as i64, p.last_seen.map(|v| v as i64)],
                )?;
                Ok(())
            })
        })
    }

    async fn list(&self) -> Result<Vec<NetworkPeer>> {
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, wireguard_pubkey, endpoint, allowed_ips, registered_at, last_seen FROM network_peers",
                )?;
                let rows = stmt.query_map([], row_to_peer)?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Option<NetworkPeer>> {
        let aid = agent_id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, wireguard_pubkey, endpoint, allowed_ips, registered_at, last_seen FROM network_peers WHERE agent_id = ?1",
                )?;
                let mut rows = stmt.query(params![aid.as_str()])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_peer(row)?))
                } else {
                    Ok(None)
                }
            })
        })
    }

    async fn update_last_seen(&self, id: &Id, now: u64) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "UPDATE network_peers SET last_seen = ?1 WHERE id = ?2",
                    params![now as i64, id.as_str()],
                )?;
                Ok(())
            })
        })
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let id = id.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "DELETE FROM network_peers WHERE id = ?1",
                    params![id.as_str()],
                )?;
                Ok(())
            })
        })
    }
}

// ───────────────────────────────────────────────
// ActivityRepository
// ───────────────────────────────────────────────

use gyre_domain::ActivityEvent;

#[async_trait]
impl ActivityRepository for SqliteDb {
    async fn append(&self, event: &ActivityEvent) -> Result<()> {
        let e = event.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO activity_events (id, agent_id, event_type, description, timestamp) VALUES (?1,?2,?3,?4,?5)",
                    params![e.id.as_str(), e.agent_id, e.event_type, e.description, e.timestamp as i64],
                )?;
                Ok(())
            })
        })
    }

    async fn query(&self, q: &ActivityQuery) -> Result<Vec<ActivityEvent>> {
        let since = q.since;
        let limit = q.limit;
        let agent_id = q.agent_id.clone();
        let event_type = q.event_type.clone();
        blocking!(self, |db: &SqliteDb| {
            db.with_conn(|conn| {
                let lim = limit.unwrap_or(50) as i64;
                let mut stmt = conn.prepare(
                    "SELECT id, agent_id, event_type, description, timestamp FROM activity_events WHERE (?1 IS NULL OR timestamp >= ?1) AND (?2 IS NULL OR agent_id = ?2) AND (?3 IS NULL OR event_type = ?3) ORDER BY timestamp DESC LIMIT ?4",
                )?;
                let rows = stmt.query_map(
                    params![since.map(|s| s as i64), agent_id, event_type, lim],
                    |row| {
                        Ok(ActivityEvent {
                            id: Id::new(row.get::<_, String>(0)?),
                            agent_id: row.get(1)?,
                            event_type: row.get(2)?,
                            description: row.get(3)?,
                            timestamp: row.get::<_, i64>(4)? as u64,
                        })
                    },
                )?;
                rows.map(|r| r.map_err(anyhow::Error::from)).collect()
            })
        })
    }
}

// ───────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::Project;

    fn make_db() -> SqliteDb {
        SqliteDb::open(":memory:").expect("in-memory db")
    }

    #[tokio::test]
    async fn sqlite_project_persists_across_store_recreation() {
        // Simulate persistence by writing and reading from same db
        let db = make_db();

        let p = Project::new(Id::new("p1"), "My Project", 1000);
        ProjectRepository::create(&db, &p).await.unwrap();

        // Recreate a "new" reference pointing at same connection
        let db2 = db.clone();
        let found = ProjectRepository::find_by_id(&db2, &Id::new("p1"))
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "My Project");
    }

    #[tokio::test]
    async fn sqlite_task_roundtrip() {
        use gyre_domain::Task;
        let db = make_db();
        let mut t = Task::new(Id::new("t1"), "Test Task", 2000);
        t.labels = vec!["backend".to_string(), "rust".to_string()];
        t.description = Some("A test task".to_string());

        TaskRepository::create(&db, &t).await.unwrap();
        let found = TaskRepository::find_by_id(&db, &Id::new("t1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "Test Task");
        assert_eq!(found.labels, vec!["backend", "rust"]);
        assert_eq!(found.description.as_deref(), Some("A test task"));
    }

    #[tokio::test]
    async fn sqlite_agent_status_filter() {
        use gyre_domain::{Agent, AgentStatus};
        let db = make_db();

        let mut a1 = Agent::new(Id::new("a1"), "agent-one", 1000);
        a1.status = AgentStatus::Active;
        let a2 = Agent::new(Id::new("a2"), "agent-two", 1000);
        // a2 stays Idle

        AgentRepository::create(&db, &a1).await.unwrap();
        AgentRepository::create(&db, &a2).await.unwrap();

        let active = AgentRepository::list_by_status(&db, &AgentStatus::Active)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "agent-one");

        let idle = AgentRepository::list_by_status(&db, &AgentStatus::Idle)
            .await
            .unwrap();
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].name, "agent-two");
    }

    #[tokio::test]
    async fn sqlite_merge_queue_ordering() {
        use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};
        let db = make_db();

        let low = MergeQueueEntry::new(Id::new("e1"), Id::new("mr1"), 25, 1000);
        let high = MergeQueueEntry::new(Id::new("e2"), Id::new("mr2"), 75, 2000);
        let medium = MergeQueueEntry::new(Id::new("e3"), Id::new("mr3"), 50, 1500);

        db.enqueue(&low).await.unwrap();
        db.enqueue(&high).await.unwrap();
        db.enqueue(&medium).await.unwrap();

        let next = db.next_pending().await.unwrap().unwrap();
        assert_eq!(next.id.as_str(), "e2"); // highest priority

        db.update_status(&Id::new("e2"), MergeQueueEntryStatus::Merged, None)
            .await
            .unwrap();
        let next2 = db.next_pending().await.unwrap().unwrap();
        assert_eq!(next2.id.as_str(), "e3"); // medium priority next
    }

    #[tokio::test]
    async fn sqlite_analytics_aggregate_by_day() {
        let db = make_db();

        // Two events on 2024-01-01 (unix: 1704067200), one on 2024-01-02 (1704153600)
        let e1 = AnalyticsEvent::new(
            Id::new("ae1"),
            "task.done",
            None,
            serde_json::Value::Null,
            1704067200,
        );
        let e2 = AnalyticsEvent::new(
            Id::new("ae2"),
            "task.done",
            None,
            serde_json::Value::Null,
            1704067260,
        );
        let e3 = AnalyticsEvent::new(
            Id::new("ae3"),
            "task.done",
            None,
            serde_json::Value::Null,
            1704153600,
        );

        AnalyticsRepository::record(&db, &e1).await.unwrap();
        AnalyticsRepository::record(&db, &e2).await.unwrap();
        AnalyticsRepository::record(&db, &e3).await.unwrap();

        let daily = db
            .aggregate_by_day("task.done", 1704067200, 1704153600 + 86400)
            .await
            .unwrap();
        assert_eq!(daily.len(), 2);
        let (day1, count1) = &daily[0];
        let (day2, count2) = &daily[1];
        assert_eq!(day1, "2024-01-01");
        assert_eq!(*count1, 2);
        assert_eq!(day2, "2024-01-02");
        assert_eq!(*count2, 1);
    }
}
