use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{Task, TaskPriority, TaskStatus};
use gyre_ports::TaskRepository;

use super::{open_conn, SqliteStorage};

fn status_to_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "Backlog",
        TaskStatus::InProgress => "InProgress",
        TaskStatus::Review => "Review",
        TaskStatus::Done => "Done",
        TaskStatus::Blocked => "Blocked",
    }
}

fn str_to_status(s: &str) -> Result<TaskStatus> {
    match s {
        "Backlog" => Ok(TaskStatus::Backlog),
        "InProgress" => Ok(TaskStatus::InProgress),
        "Review" => Ok(TaskStatus::Review),
        "Done" => Ok(TaskStatus::Done),
        "Blocked" => Ok(TaskStatus::Blocked),
        other => Err(anyhow!("unknown task status: {}", other)),
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

fn row_to_task(row: &rusqlite::Row<'_>) -> Result<Task> {
    let status_str: String = row.get(3)?;
    let priority_str: String = row.get(4)?;
    let labels_json: String = row.get(7)?;
    let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
    Ok(Task {
        id: Id::new(row.get::<_, String>(0)?),
        title: row.get(1)?,
        description: row.get(2)?,
        status: str_to_status(&status_str)?,
        priority: str_to_priority(&priority_str)?,
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
impl TaskRepository for SqliteStorage {
    async fn create(&self, task: &Task) -> Result<()> {
        let path = self.db_path();
        let t = task.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let labels_json = serde_json::to_string(&t.labels)?;
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO tasks (id, title, description, status, priority,
                                    assigned_to, parent_task_id, labels, branch, pr_link,
                                    created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    t.id.as_str(),
                    t.title,
                    t.description,
                    status_to_str(&t.status),
                    priority_to_str(&t.priority),
                    t.assigned_to.as_ref().map(|id| id.as_str()),
                    t.parent_task_id.as_ref().map(|id| id.as_str()),
                    labels_json,
                    t.branch,
                    t.pr_link,
                    t.created_at as i64,
                    t.updated_at as i64,
                ],
            )
            .context("insert task")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Task>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, title, description, status, priority, assigned_to, parent_task_id,
                        labels, branch, pr_link, created_at, updated_at
                 FROM tasks WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_task(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Task>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, title, description, status, priority, assigned_to, parent_task_id,
                        labels, branch, pr_link, created_at, updated_at
                 FROM tasks ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_task(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>> {
        let path = self.db_path();
        let status_str = status_to_str(status).to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, title, description, status, priority, assigned_to, parent_task_id,
                        labels, branch, pr_link, created_at, updated_at
                 FROM tasks WHERE status = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([&status_str], |row| Ok(row_to_task(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>> {
        let path = self.db_path();
        let agent_id = agent_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, title, description, status, priority, assigned_to, parent_task_id,
                        labels, branch, pr_link, created_at, updated_at
                 FROM tasks WHERE assigned_to = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([agent_id.as_str()], |row| Ok(row_to_task(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>> {
        let path = self.db_path();
        let parent_id = parent_task_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, title, description, status, priority, assigned_to, parent_task_id,
                        labels, branch, pr_link, created_at, updated_at
                 FROM tasks WHERE parent_task_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([parent_id.as_str()], |row| Ok(row_to_task(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, task: &Task) -> Result<()> {
        let path = self.db_path();
        let t = task.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let labels_json = serde_json::to_string(&t.labels)?;
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE tasks SET title=?1, description=?2, status=?3, priority=?4,
                          assigned_to=?5, parent_task_id=?6, labels=?7, branch=?8,
                          pr_link=?9, updated_at=?10
                 WHERE id=?11",
                rusqlite::params![
                    t.title,
                    t.description,
                    status_to_str(&t.status),
                    priority_to_str(&t.priority),
                    t.assigned_to.as_ref().map(|id| id.as_str()),
                    t.parent_task_id.as_ref().map(|id| id.as_str()),
                    labels_json,
                    t.branch,
                    t.pr_link,
                    t.updated_at as i64,
                    t.id.as_str(),
                ],
            )
            .context("update task")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM tasks WHERE id=?1", [id.as_str()])
                .context("delete task")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::Agent;
    use gyre_ports::AgentRepository;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_task(id: &str, title: &str) -> Task {
        Task::new(Id::new(id), title, 1000)
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        let t = make_task("t1", "Do something");
        TaskRepository::create(&s, &t).await.unwrap();
        let found = TaskRepository::find_by_id(&s, &t.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "Do something");
        assert_eq!(found.status, TaskStatus::Backlog);
        assert_eq!(found.priority, TaskPriority::Medium);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        assert!(TaskRepository::find_by_id(&s, &Id::new("nope"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_tasks() {
        let (_tmp, s) = setup();
        TaskRepository::create(&s, &make_task("t1", "A"))
            .await
            .unwrap();
        TaskRepository::create(&s, &make_task("t2", "B"))
            .await
            .unwrap();
        assert_eq!(TaskRepository::list(&s).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_by_status() {
        let (_tmp, s) = setup();
        let mut t1 = make_task("t1", "Active task");
        let t2 = make_task("t2", "Backlog task");
        TaskRepository::create(&s, &t1).await.unwrap();
        TaskRepository::create(&s, &t2).await.unwrap();
        t1.status = TaskStatus::InProgress;
        t1.updated_at = 2000;
        TaskRepository::update(&s, &t1).await.unwrap();

        let in_progress = TaskRepository::list_by_status(&s, &TaskStatus::InProgress)
            .await
            .unwrap();
        assert_eq!(in_progress.len(), 1);
        assert_eq!(in_progress[0].title, "Active task");

        let backlog = TaskRepository::list_by_status(&s, &TaskStatus::Backlog)
            .await
            .unwrap();
        assert_eq!(backlog.len(), 1);
    }

    #[tokio::test]
    async fn list_by_assignee() {
        let (_tmp, s) = setup();
        let agent = Agent::new(Id::new("a1"), "worker", 1000);
        AgentRepository::create(&s, &agent).await.unwrap();

        let mut t1 = make_task("t1", "Assigned");
        t1.assigned_to = Some(Id::new("a1"));
        let t2 = make_task("t2", "Unassigned");
        TaskRepository::create(&s, &t1).await.unwrap();
        TaskRepository::create(&s, &t2).await.unwrap();

        let assigned = TaskRepository::list_by_assignee(&s, &Id::new("a1"))
            .await
            .unwrap();
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].title, "Assigned");
    }

    #[tokio::test]
    async fn list_by_parent() {
        let (_tmp, s) = setup();
        let parent = make_task("p1", "Parent");
        TaskRepository::create(&s, &parent).await.unwrap();
        let mut child1 = make_task("c1", "Child 1");
        child1.parent_task_id = Some(Id::new("p1"));
        let mut child2 = make_task("c2", "Child 2");
        child2.parent_task_id = Some(Id::new("p1"));
        TaskRepository::create(&s, &child1).await.unwrap();
        TaskRepository::create(&s, &child2).await.unwrap();

        let children = TaskRepository::list_by_parent(&s, &Id::new("p1"))
            .await
            .unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn update_task() {
        let (_tmp, s) = setup();
        let mut t = make_task("t1", "Original");
        TaskRepository::create(&s, &t).await.unwrap();
        t.title = "Updated".to_string();
        t.status = TaskStatus::InProgress;
        t.priority = TaskPriority::High;
        t.labels = vec!["bug".to_string(), "urgent".to_string()];
        t.branch = Some("feat/x".to_string());
        t.updated_at = 9999;
        TaskRepository::update(&s, &t).await.unwrap();

        let found = TaskRepository::find_by_id(&s, &t.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "Updated");
        assert_eq!(found.status, TaskStatus::InProgress);
        assert_eq!(found.priority, TaskPriority::High);
        assert_eq!(found.labels, vec!["bug", "urgent"]);
        assert_eq!(found.branch.as_deref(), Some("feat/x"));
        assert_eq!(found.updated_at, 9999);
    }

    #[tokio::test]
    async fn delete_task() {
        let (_tmp, s) = setup();
        let t = make_task("t1", "Temp");
        TaskRepository::create(&s, &t).await.unwrap();
        TaskRepository::delete(&s, &t.id).await.unwrap();
        assert!(TaskRepository::find_by_id(&s, &t.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn labels_roundtrip() {
        let (_tmp, s) = setup();
        let mut t = make_task("t1", "Labeled");
        t.labels = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        TaskRepository::create(&s, &t).await.unwrap();
        let found = TaskRepository::find_by_id(&s, &t.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.labels, t.labels);
    }
}
