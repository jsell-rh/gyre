//! In-memory implementations of port traits for development and testing.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, BranchInfo, CommitInfo, DiffResult, MergeRequest, MrStatus, Project,
    Repository, Review, ReviewComment, ReviewDecision, Task, TaskStatus,
};
use gyre_ports::{
    AgentRepository, GitOpsPort, MergeRequestRepository, ProjectRepository, RepoRepository,
    ReviewRepository, TaskRepository,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// No-op git operations adapter for tests (never touches the filesystem).
#[derive(Default)]
pub struct NoopGitOps;

#[async_trait]
impl GitOpsPort for NoopGitOps {
    async fn init_bare(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn list_branches(&self, _repo_path: &str) -> Result<Vec<BranchInfo>> {
        Ok(vec![])
    }

    async fn commit_log(
        &self,
        _repo_path: &str,
        _branch: &str,
        _limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        Ok(vec![])
    }

    async fn diff(&self, _repo_path: &str, _from: &str, _to: &str) -> Result<DiffResult> {
        Ok(DiffResult {
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            patches: vec![],
        })
    }

    async fn is_repo(&self, _path: &str) -> Result<bool> {
        Ok(false)
    }

    async fn can_merge(&self, _repo_path: &str, _source: &str, _target: &str) -> Result<bool> {
        Ok(true)
    }
}

#[derive(Default)]
pub struct MemProjectRepository {
    store: Arc<Mutex<HashMap<String, Project>>>,
}

#[async_trait]
impl ProjectRepository for MemProjectRepository {
    async fn create(&self, project: &Project) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(project.id.to_string(), project.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Project>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list(&self) -> Result<Vec<Project>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn update(&self, project: &Project) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(project.id.to_string(), project.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemRepoRepository {
    store: Arc<Mutex<HashMap<String, Repository>>>,
}

#[async_trait]
impl RepoRepository for MemRepoRepository {
    async fn create(&self, repo: &Repository) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(repo.id.to_string(), repo.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_project(&self, project_id: &Id) -> Result<Vec<Repository>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|r| r.project_id.as_str() == project_id.as_str())
            .cloned()
            .collect())
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(repo.id.to_string(), repo.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemAgentRepository {
    store: Arc<Mutex<HashMap<String, Agent>>>,
}

#[async_trait]
impl AgentRepository for MemAgentRepository {
    async fn create(&self, agent: &Agent) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(agent.id.to_string(), agent.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Agent>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Agent>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|a| a.name == name)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Agent>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_status(&self, status: &AgentStatus) -> Result<Vec<Agent>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|a| &a.status == status)
            .cloned()
            .collect())
    }

    async fn update(&self, agent: &Agent) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(agent.id.to_string(), agent.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemTaskRepository {
    store: Arc<Mutex<HashMap<String, Task>>>,
}

#[async_trait]
impl TaskRepository for MemTaskRepository {
    async fn create(&self, task: &Task) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(task.id.to_string(), task.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Task>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list(&self) -> Result<Vec<Task>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_status(&self, status: &TaskStatus) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| &t.status == status)
            .cloned()
            .collect())
    }

    async fn list_by_assignee(&self, agent_id: &Id) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| t.assigned_to.as_ref().map(|id| id.as_str()) == Some(agent_id.as_str()))
            .cloned()
            .collect())
    }

    async fn list_by_parent(&self, parent_task_id: &Id) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| {
                t.parent_task_id.as_ref().map(|id| id.as_str()) == Some(parent_task_id.as_str())
            })
            .cloned()
            .collect())
    }

    async fn update(&self, task: &Task) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(task.id.to_string(), task.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemMrRepository {
    store: Arc<Mutex<HashMap<String, MergeRequest>>>,
}

#[async_trait]
impl MergeRequestRepository for MemMrRepository {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(mr.id.to_string(), mr.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|mr| &mr.status == status)
            .cloned()
            .collect())
    }

    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|mr| mr.repository_id.as_str() == repository_id.as_str())
            .cloned()
            .collect())
    }

    async fn update(&self, mr: &MergeRequest) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(mr.id.to_string(), mr.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemReviewRepository {
    comments: Arc<Mutex<HashMap<String, ReviewComment>>>,
    reviews: Arc<Mutex<HashMap<String, Review>>>,
}

#[async_trait]
impl ReviewRepository for MemReviewRepository {
    async fn add_comment(&self, comment: &ReviewComment) -> Result<()> {
        self.comments
            .lock()
            .await
            .insert(comment.id.to_string(), comment.clone());
        Ok(())
    }

    async fn list_comments(&self, mr_id: &Id) -> Result<Vec<ReviewComment>> {
        let mut comments: Vec<ReviewComment> = self
            .comments
            .lock()
            .await
            .values()
            .filter(|c| c.merge_request_id.as_str() == mr_id.as_str())
            .cloned()
            .collect();
        comments.sort_by_key(|c| c.created_at);
        Ok(comments)
    }

    async fn submit_review(&self, review: &Review) -> Result<()> {
        self.reviews
            .lock()
            .await
            .insert(review.id.to_string(), review.clone());
        Ok(())
    }

    async fn list_reviews(&self, mr_id: &Id) -> Result<Vec<Review>> {
        let mut reviews: Vec<Review> = self
            .reviews
            .lock()
            .await
            .values()
            .filter(|r| r.merge_request_id.as_str() == mr_id.as_str())
            .cloned()
            .collect();
        reviews.sort_by_key(|r| r.created_at);
        Ok(reviews)
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
        Ok(reviews.iter().any(|r| r.decision == ReviewDecision::Approved))
    }
}

/// Build an AppState with all in-memory repositories for tests.
#[cfg(test)]
pub fn test_state() -> Arc<crate::AppState> {
    use std::collections::HashMap;
    use tokio::sync::{broadcast, Mutex};
    Arc::new(crate::AppState {
        auth_token: "test-token".to_string(),
        projects: Arc::new(MemProjectRepository::default()),
        repos: Arc::new(MemRepoRepository::default()),
        agents: Arc::new(MemAgentRepository::default()),
        tasks: Arc::new(MemTaskRepository::default()),
        merge_requests: Arc::new(MemMrRepository::default()),
        reviews: Arc::new(MemReviewRepository::default()),
        git_ops: Arc::new(NoopGitOps),
        activity_store: crate::activity::ActivityStore::new(),
        broadcast_tx: broadcast::channel(16).0,
        agent_messages: Arc::new(Mutex::new(HashMap::new())),
        agent_tokens: Arc::new(Mutex::new(HashMap::new())),
    })
}
