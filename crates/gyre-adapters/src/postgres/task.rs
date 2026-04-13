use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Task, TaskPriority, TaskStatus, TaskType};
use gyre_ports::TaskRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::tasks;

fn status_to_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "Backlog",
        TaskStatus::InProgress => "InProgress",
        TaskStatus::Review => "Review",
        TaskStatus::Done => "Done",
        TaskStatus::Blocked => "Blocked",
        TaskStatus::Cancelled => "Cancelled",
    }
}

fn str_to_status(s: &str) -> Result<TaskStatus> {
    match s {
        "Backlog" => Ok(TaskStatus::Backlog),
        "InProgress" => Ok(TaskStatus::InProgress),
        "Review" => Ok(TaskStatus::Review),
        "Done" => Ok(TaskStatus::Done),
        "Blocked" => Ok(TaskStatus::Blocked),
        "Cancelled" => Ok(TaskStatus::Cancelled),
        other => Err(anyhow!("unknown task status: {}", other)),
    }
}

fn task_type_to_str(t: &TaskType) -> &'static str {
    match t {
        TaskType::Implementation => "Implementation",
        TaskType::Delegation => "Delegation",
        TaskType::Coordination => "Coordination",
    }
}

fn str_to_task_type(s: &str) -> Result<TaskType> {
    match s {
        "Implementation" => Ok(TaskType::Implementation),
        "Delegation" => Ok(TaskType::Delegation),
        "Coordination" => Ok(TaskType::Coordination),
        other => Err(anyhow!("unknown task type: {}", other)),
    }
}

fn priority_to_str(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::Low => "Low",
        TaskPriority::Medium => "Medium",
        TaskPriority::High => "High",
        TaskPriority::Critical => "Critical",
    }
}

fn str_to_priority(s: &str) -> Result<TaskPriority> {
    match s {
        "Low" => Ok(TaskPriority::Low),
        "Medium" => Ok(TaskPriority::Medium),
        "High" => Ok(TaskPriority::High),
        "Critical" => Ok(TaskPriority::Critical),
        other => Err(anyhow!("unknown task priority: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = tasks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct TaskRow {
    id: String,
    title: String,
    description: Option<String>,
    status: String,
    priority: String,
    assigned_to: Option<String>,
    parent_task_id: Option<String>,
    labels: String,
    branch: Option<String>,
    pr_link: Option<String>,
    created_at: i64,
    updated_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
    workspace_id: String,
    spec_path: Option<String>,
    repo_id: String,
    cancelled_at: Option<i64>,
    cancelled_reason: Option<String>,
    task_type: Option<String>,
    order: Option<i32>,
    depends_on: String,
}

impl TaskRow {
    fn into_task(self) -> Result<Task> {
        let labels: Vec<String> = serde_json::from_str(&self.labels).unwrap_or_default();
        Ok(Task {
            id: Id::new(self.id),
            title: self.title,
            description: self.description,
            status: str_to_status(&self.status)?,
            priority: str_to_priority(&self.priority)?,
            assigned_to: self.assigned_to.map(Id::new),
            parent_task_id: self.parent_task_id.map(Id::new),
            labels,
            branch: self.branch,
            pr_link: self.pr_link,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
            workspace_id: Id::new(self.workspace_id),
            repo_id: Id::new(self.repo_id),
            spec_path: self.spec_path,
            cancelled_at: self.cancelled_at.map(|v| v as u64),
            cancelled_reason: self.cancelled_reason,
            task_type: self
                .task_type
                .as_deref()
                .map(str_to_task_type)
                .transpose()?,
            order: self.order.map(|v| v as u32),
            depends_on: serde_json::from_str::<Vec<String>>(&self.depends_on)
                .unwrap_or_default()
                .into_iter()
                .map(Id::new)
                .collect(),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = tasks)]
struct NewTaskRow<'a> {
    id: &'a str,
    title: &'a str,
    description: Option<&'a str>,
    status: &'a str,
    priority: &'a str,
    assigned_to: Option<&'a str>,
    parent_task_id: Option<&'a str>,
    labels: &'a str,
    branch: Option<&'a str>,
    pr_link: Option<&'a str>,
    created_at: i64,
    updated_at: i64,
    tenant_id: &'a str,
    workspace_id: &'a str,
    spec_path: Option<&'a str>,
    repo_id: &'a str,
    cancelled_at: Option<i64>,
    cancelled_reason: Option<&'a str>,
    task_type: Option<&'a str>,
    order: Option<i32>,
    depends_on: &'a str,
}

#[async_trait]
impl TaskRepository for PgStorage {
    async fn create(&self, task: &Task) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = task.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let labels_json = serde_json::to_string(&t.labels)?;
            let depends_on_json = serde_json::to_string(
                &t.depends_on
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            )?;
            let task_type_str = t.task_type.as_ref().map(task_type_to_str);
            let row = NewTaskRow {
                id: t.id.as_str(),
                title: &t.title,
                description: t.description.as_deref(),
                status: status_to_str(&t.status),
                priority: priority_to_str(&t.priority),
                assigned_to: t.assigned_to.as_ref().map(|id| id.as_str()),
                parent_task_id: t.parent_task_id.as_ref().map(|id| id.as_str()),
                labels: &labels_json,
                branch: t.branch.as_deref(),
                pr_link: t.pr_link.as_deref(),
                created_at: t.created_at as i64,
                updated_at: t.updated_at as i64,
                tenant_id: &tenant,
                workspace_id: t.workspace_id.as_str(),
                spec_path: t.spec_path.as_deref(),
                repo_id: t.repo_id.as_str(),
                cancelled_at: t.cancelled_at.map(|v| v as i64),
                cancelled_reason: t.cancelled_reason.as_deref(),
                task_type: task_type_str,
                order: t.order.map(|v| v as i32),
                depends_on: &depends_on_json,
            };
            diesel::insert_into(tasks::table)
                .values(&row)
                .on_conflict(tasks::id)
                .do_update()
                .set((
                    tasks::title.eq(row.title),
                    tasks::description.eq(row.description),
                    tasks::status.eq(row.status),
                    tasks::priority.eq(row.priority),
                    tasks::assigned_to.eq(row.assigned_to),
                    tasks::parent_task_id.eq(row.parent_task_id),
                    tasks::labels.eq(row.labels),
                    tasks::branch.eq(row.branch),
                    tasks::pr_link.eq(row.pr_link),
                    tasks::updated_at.eq(row.updated_at),
                    tasks::workspace_id.eq(row.workspace_id),
                    tasks::spec_path.eq(row.spec_path),
                    tasks::repo_id.eq(row.repo_id),
                    tasks::cancelled_at.eq(row.cancelled_at),
                    tasks::cancelled_reason.eq(row.cancelled_reason),
                    tasks::task_type.eq(row.task_type),
                    tasks::order.eq(row.order),
                    tasks::depends_on.eq(row.depends_on),
                ))
                .execute(&mut *conn)
                .context("insert task")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = tasks::table
                .find(id.as_str())
                .filter(tasks::tenant_id.eq(&tenant))
                .first::<TaskRow>(&mut *conn)
                .optional()
                .context("find task by id")?;
            result.map(TaskRow::into_task).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let status_str = status_to_str(status).to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::status.eq(&status_str))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by status")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::assigned_to.eq(agent_id.as_str()))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by assignee")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let parent_id = parent_task_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::parent_task_id.eq(parent_id.as_str()))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by parent")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn update(&self, task: &Task) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = task.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let labels_json = serde_json::to_string(&t.labels)?;
            let depends_on_json = serde_json::to_string(
                &t.depends_on
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            )?;
            let task_type_str = t.task_type.as_ref().map(task_type_to_str);
            diesel::update(
                tasks::table
                    .find(t.id.as_str())
                    .filter(tasks::tenant_id.eq(&tenant)),
            )
            .set((
                tasks::title.eq(&t.title),
                tasks::description.eq(t.description.as_deref()),
                tasks::status.eq(status_to_str(&t.status)),
                tasks::priority.eq(priority_to_str(&t.priority)),
                tasks::assigned_to.eq(t.assigned_to.as_ref().map(|id| id.as_str())),
                tasks::parent_task_id.eq(t.parent_task_id.as_ref().map(|id| id.as_str())),
                tasks::labels.eq(&labels_json),
                tasks::branch.eq(t.branch.as_deref()),
                tasks::pr_link.eq(t.pr_link.as_deref()),
                tasks::updated_at.eq(t.updated_at as i64),
                tasks::workspace_id.eq(t.workspace_id.as_str()),
                tasks::spec_path.eq(t.spec_path.as_deref()),
                tasks::repo_id.eq(t.repo_id.as_str()),
                tasks::cancelled_at.eq(t.cancelled_at.map(|v| v as i64)),
                tasks::cancelled_reason.eq(t.cancelled_reason.as_deref()),
                tasks::task_type.eq(task_type_str),
                tasks::order.eq(t.order.map(|v| v as i32)),
                tasks::depends_on.eq(&depends_on_json),
            ))
            .execute(&mut *conn)
            .context("update task")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                tasks::table
                    .find(id.as_str())
                    .filter(tasks::tenant_id.eq(&tenant)),
            )
            .execute(&mut *conn)
            .context("delete task")?;
            Ok(())
        })
        .await?
    }
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::workspace_id.eq(workspace_id.as_str()))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by workspace")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn list_by_spec_path(&self, spec_path: &str) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let sp = spec_path.to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::spec_path.eq(&sp))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by spec_path")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<Task>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tasks::table
                .filter(tasks::tenant_id.eq(&tenant))
                .filter(tasks::repo_id.eq(repo_id.as_str()))
                .order(tasks::created_at.asc())
                .load::<TaskRow>(&mut *conn)
                .context("list tasks by repo")?;
            rows.into_iter().map(TaskRow::into_task).collect()
        })
        .await?
    }
}
