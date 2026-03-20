//! In-memory implementations of port traits for development and testing.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{
    Agent, AgentCommit, AgentStatus, AgentWorktree, AnalyticsEvent, AuditEvent, CostEntry,
    MergeQueueEntry, MergeQueueEntryStatus, MergeRequest, MrStatus, NetworkPeer, Project,
    Repository, Review, ReviewComment, ReviewDecision, Task, TaskStatus, User,
};
#[cfg(test)]
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, MergeResult};
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AnalyticsRepository, ApiKeyRepository, AuditRepository,
    CostRepository, MergeQueueRepository, MergeRequestRepository, NetworkPeerRepository,
    ProjectRepository, RepoRepository, ReviewRepository, TaskRepository, UserRepository,
    WorktreeRepository,
};
#[cfg(test)]
use gyre_ports::{GitOpsPort, JjChange, JjOpsPort};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// No-op git operations adapter for tests (never touches the filesystem).
#[cfg(test)]
#[derive(Default)]
pub struct NoopGitOps;

/// No-op jj operations adapter for tests (always succeeds with empty results).
#[cfg(test)]
#[derive(Default)]
pub struct NoopJjOps;

#[cfg(test)]
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

    async fn merge_branches(
        &self,
        _repo_path: &str,
        _source: &str,
        _target: &str,
    ) -> Result<MergeResult> {
        Ok(MergeResult::Success {
            merge_commit_sha: "0000000000000000000000000000000000000000".to_string(),
        })
    }

    async fn create_worktree(
        &self,
        _repo_path: &str,
        _worktree_path: &str,
        _branch: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn remove_worktree(&self, _repo_path: &str, _worktree_path: &str) -> Result<()> {
        Ok(())
    }

    async fn list_worktrees(&self, _repo_path: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn create_initial_commit(&self, _repo_path: &str, _branch: &str) -> Result<String> {
        Ok("0000000000000000000000000000000000000000".to_string())
    }

    async fn clone_mirror(&self, _url: &str, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn fetch_mirror(&self, _path: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[async_trait]
impl JjOpsPort for NoopJjOps {
    async fn jj_init(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_new(&self, _repo_path: &str, _description: &str) -> Result<String> {
        Ok("noop-change-id".to_string())
    }

    async fn jj_describe(
        &self,
        _repo_path: &str,
        _change_id: &str,
        _description: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn jj_log(&self, _repo_path: &str, _limit: usize) -> Result<Vec<JjChange>> {
        Ok(vec![])
    }

    async fn jj_squash(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_bookmark_create(
        &self,
        _repo_path: &str,
        _name: &str,
        _change_id: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn jj_undo(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct MemAgentCommitRepository {
    store: Arc<Mutex<Vec<AgentCommit>>>,
}

#[async_trait]
impl AgentCommitRepository for MemAgentCommitRepository {
    async fn record(&self, mapping: &AgentCommit) -> Result<()> {
        self.store.lock().await.push(mapping.clone());
        Ok(())
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentCommit>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|ac| ac.agent_id.as_str() == agent_id.as_str())
            .cloned()
            .collect())
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentCommit>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|ac| ac.repository_id.as_str() == repo_id.as_str())
            .cloned()
            .collect())
    }

    async fn find_by_commit(&self, sha: &str) -> Result<Option<AgentCommit>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|ac| ac.commit_sha == sha)
            .cloned())
    }
}

#[derive(Default)]
pub struct MemWorktreeRepository {
    store: Arc<Mutex<HashMap<String, AgentWorktree>>>,
}

#[async_trait]
impl WorktreeRepository for MemWorktreeRepository {
    async fn create(&self, worktree: &AgentWorktree) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(worktree.id.to_string(), worktree.clone());
        Ok(())
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Vec<AgentWorktree>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|wt| wt.agent_id.as_str() == agent_id.as_str())
            .cloned()
            .collect())
    }

    async fn find_by_repo(&self, repo_id: &Id) -> Result<Vec<AgentWorktree>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|wt| wt.repository_id.as_str() == repo_id.as_str())
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
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
        Ok(reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::Approved))
    }
}

#[derive(Default)]
pub struct MemMergeQueueRepository {
    store: Arc<Mutex<Vec<MergeQueueEntry>>>,
}

#[async_trait]
impl MergeQueueRepository for MemMergeQueueRepository {
    async fn enqueue(&self, entry: &MergeQueueEntry) -> Result<()> {
        self.store.lock().await.push(entry.clone());
        Ok(())
    }

    async fn next_pending(&self) -> Result<Option<MergeQueueEntry>> {
        let store = self.store.lock().await;
        let mut queued: Vec<&MergeQueueEntry> = store
            .iter()
            .filter(|e| e.status == MergeQueueEntryStatus::Queued)
            .collect();
        queued.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.enqueued_at.cmp(&b.enqueued_at))
        });
        Ok(queued.first().map(|e| (*e).clone()))
    }

    async fn update_status(
        &self,
        id: &Id,
        status: MergeQueueEntryStatus,
        error: Option<String>,
    ) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(e) = store.iter_mut().find(|e| e.id.as_str() == id.as_str()) {
            e.status = status;
            e.error_message = error;
        }
        Ok(())
    }

    async fn list_queue(&self) -> Result<Vec<MergeQueueEntry>> {
        let store = self.store.lock().await;
        let mut entries: Vec<MergeQueueEntry> =
            store.iter().filter(|e| !e.is_terminal()).cloned().collect();
        entries.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.enqueued_at.cmp(&b.enqueued_at))
        });
        Ok(entries)
    }

    async fn cancel(&self, id: &Id) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(e) = store.iter_mut().find(|e| e.id.as_str() == id.as_str()) {
            if !e.is_terminal() {
                e.status = MergeQueueEntryStatus::Cancelled;
            }
        }
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeQueueEntry>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|e| e.id.as_str() == id.as_str())
            .cloned())
    }
}

#[derive(Default)]
pub struct MemUserRepository {
    store: Arc<Mutex<HashMap<String, User>>>,
}

#[async_trait]
impl UserRepository for MemUserRepository {
    async fn create(&self, user: &User) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(user.id.to_string(), user.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<User>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|u| u.external_id == external_id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<User>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn update(&self, user: &User) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(user.id.to_string(), user.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemApiKeyRepository {
    /// key -> (user_id, name)
    store: Arc<Mutex<HashMap<String, Id>>>,
}

#[async_trait]
impl ApiKeyRepository for MemApiKeyRepository {
    async fn create(&self, key: &str, user_id: &Id, _name: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(key.to_string(), user_id.clone());
        Ok(())
    }

    async fn find_user_id(&self, key: &str) -> Result<Option<Id>> {
        Ok(self.store.lock().await.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.store.lock().await.remove(key);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemAnalyticsRepository {
    store: Arc<Mutex<Vec<AnalyticsEvent>>>,
}

#[async_trait]
impl AnalyticsRepository for MemAnalyticsRepository {
    async fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        self.store.lock().await.push(event.clone());
        Ok(())
    }

    async fn query(
        &self,
        event_name: Option<&str>,
        since: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AnalyticsEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AnalyticsEvent> = store
            .iter()
            .filter(|e| {
                event_name.is_none_or(|n| e.event_name == n)
                    && since.is_none_or(|s| e.timestamp >= s)
            })
            .cloned()
            .collect();
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        events.truncate(limit);
        Ok(events)
    }

    async fn count(&self, event_name: &str, since: u64, until: u64) -> Result<u64> {
        let store = self.store.lock().await;
        let count = store
            .iter()
            .filter(|e| e.event_name == event_name && e.timestamp >= since && e.timestamp <= until)
            .count();
        Ok(count as u64)
    }

    async fn aggregate_by_day(
        &self,
        event_name: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<(String, u64)>> {
        use std::collections::BTreeMap;
        let store = self.store.lock().await;
        let mut by_day: BTreeMap<String, u64> = BTreeMap::new();
        for e in store.iter() {
            if e.event_name == event_name && e.timestamp >= since && e.timestamp <= until {
                // Simple day bucketing: seconds / 86400 -> day number, format as YYYY-MM-DD
                let secs = e.timestamp as i64;
                let days_since_epoch = secs / 86400;
                // Use a simple date calculation
                let day_str = epoch_days_to_date(days_since_epoch);
                *by_day.entry(day_str).or_insert(0) += 1;
            }
        }
        Ok(by_day.into_iter().collect())
    }
}

fn epoch_days_to_date(days: i64) -> String {
    // Simplified Gregorian calendar calculation
    let n = days + 719468;
    let era = if n >= 0 { n } else { n - 146096 } / 146097;
    let doe = n - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[derive(Default)]
pub struct MemCostRepository {
    store: Arc<Mutex<Vec<CostEntry>>>,
}

#[async_trait]
impl CostRepository for MemCostRepository {
    async fn record(&self, entry: &CostEntry) -> Result<()> {
        self.store.lock().await.push(entry.clone());
        Ok(())
    }

    async fn query_by_agent(&self, agent_id: &Id, since: Option<u64>) -> Result<Vec<CostEntry>> {
        let store = self.store.lock().await;
        let mut entries: Vec<CostEntry> = store
            .iter()
            .filter(|e| {
                e.agent_id.as_str() == agent_id.as_str() && since.is_none_or(|s| e.timestamp >= s)
            })
            .cloned()
            .collect();
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(entries)
    }

    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>> {
        let store = self.store.lock().await;
        let mut entries: Vec<CostEntry> = store
            .iter()
            .filter(|e| e.task_id.as_ref().map(|id| id.as_str()) == Some(task_id.as_str()))
            .cloned()
            .collect();
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(entries)
    }

    async fn total_by_agent(&self, agent_id: &Id) -> Result<f64> {
        let store = self.store.lock().await;
        Ok(store
            .iter()
            .filter(|e| e.agent_id.as_str() == agent_id.as_str())
            .map(|e| e.amount)
            .sum())
    }

    async fn total_by_period(&self, since: u64, until: u64) -> Result<f64> {
        let store = self.store.lock().await;
        Ok(store
            .iter()
            .filter(|e| e.timestamp >= since && e.timestamp <= until)
            .map(|e| e.amount)
            .sum())
    }
}

#[derive(Default)]
pub struct MemAuditRepository {
    store: Arc<Mutex<Vec<AuditEvent>>>,
}

#[async_trait]
impl AuditRepository for MemAuditRepository {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        self.store.lock().await.push(event.clone());
        Ok(())
    }

    async fn query(
        &self,
        agent_id: Option<&str>,
        event_type: Option<&str>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AuditEvent> = store
            .iter()
            .filter(|e| {
                agent_id.is_none_or(|a| e.agent_id.as_str() == a)
                    && event_type.is_none_or(|t| e.event_type.as_str() == t)
                    && since.is_none_or(|s| e.timestamp >= s)
                    && until.is_none_or(|u| e.timestamp <= u)
            })
            .cloned()
            .collect();
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        events.truncate(limit);
        Ok(events)
    }

    async fn count(&self) -> Result<u64> {
        Ok(self.store.lock().await.len() as u64)
    }

    async fn stats_by_type(&self) -> Result<Vec<(String, u64)>> {
        use std::collections::HashMap;
        let store = self.store.lock().await;
        let mut counts: HashMap<String, u64> = HashMap::new();
        for e in store.iter() {
            *counts.entry(e.event_type.as_str()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(result)
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AuditEvent> = store
            .iter()
            .filter(|e| e.timestamp > since)
            .cloned()
            .collect();
        events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        events.truncate(limit);
        Ok(events)
    }
}

#[derive(Default)]
pub struct MemNetworkPeerRepository {
    store: Arc<Mutex<Vec<NetworkPeer>>>,
}

#[async_trait]
impl NetworkPeerRepository for MemNetworkPeerRepository {
    async fn register(&self, peer: &NetworkPeer) -> Result<()> {
        self.store.lock().await.push(peer.clone());
        Ok(())
    }

    async fn list(&self) -> Result<Vec<NetworkPeer>> {
        Ok(self.store.lock().await.clone())
    }

    async fn find_by_agent(&self, agent_id: &Id) -> Result<Option<NetworkPeer>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|p| p.agent_id.as_str() == agent_id.as_str())
            .cloned())
    }

    async fn update_last_seen(&self, id: &Id, now: u64) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(p) = store.iter_mut().find(|p| p.id.as_str() == id.as_str()) {
            p.last_seen = Some(now);
        }
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let mut store = self.store.lock().await;
        store.retain(|p| p.id.as_str() != id.as_str());
        Ok(())
    }
}

/// Build an AppState with all in-memory repositories for tests.
#[cfg(test)]
pub fn test_state() -> Arc<crate::AppState> {
    use std::collections::HashMap;
    use tokio::sync::{broadcast, Mutex};
    Arc::new(crate::AppState {
        auth_token: "test-token".to_string(),
        base_url: "http://localhost:3000".to_string(),
        projects: Arc::new(MemProjectRepository::default()),
        repos: Arc::new(MemRepoRepository::default()),
        agents: Arc::new(MemAgentRepository::default()),
        tasks: Arc::new(MemTaskRepository::default()),
        merge_requests: Arc::new(MemMrRepository::default()),
        reviews: Arc::new(MemReviewRepository::default()),
        merge_queue: Arc::new(MemMergeQueueRepository::default()),
        git_ops: Arc::new(NoopGitOps),
        jj_ops: Arc::new(NoopJjOps),
        agent_commits: Arc::new(MemAgentCommitRepository::default()),
        worktrees: Arc::new(MemWorktreeRepository::default()),
        activity_store: crate::activity::ActivityStore::new(),
        broadcast_tx: broadcast::channel(16).0,
        event_tx: broadcast::channel(16).0,
        agent_messages: Arc::new(Mutex::new(HashMap::new())),
        agent_tokens: Arc::new(Mutex::new(HashMap::new())),
        users: Arc::new(MemUserRepository::default()),
        api_keys: Arc::new(MemApiKeyRepository::default()),
        jwt_config: None,
        http_client: reqwest::Client::new(),
        metrics: Arc::new(crate::metrics::Metrics::new().expect("test metrics")),
        started_at_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        agent_cards: Arc::new(Mutex::new(HashMap::new())),
        compose_sessions: Arc::new(Mutex::new(HashMap::new())),
        retention_store: crate::retention::RetentionStore::new(),
        job_registry: Arc::new(crate::jobs::JobRegistry::new()),
        analytics: Arc::new(MemAnalyticsRepository::default()),
        costs: Arc::new(MemCostRepository::default()),
        audit: Arc::new(MemAuditRepository::default()),
        siem_store: crate::siem::SiemStore::new(),
        audit_broadcast_tx: broadcast::channel(64).0,
        compute_targets: Arc::new(Mutex::new(HashMap::new())),
        network_peers: Arc::new(MemNetworkPeerRepository::default()),
        rate_limiter: crate::rate_limit::RateLimiter::new(1000),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(Mutex::new(HashMap::new())),
        agent_log_tx: Arc::new(Mutex::new(HashMap::new())),
        quality_gates: Arc::new(Mutex::new(HashMap::new())),
        gate_results: Arc::new(Mutex::new(HashMap::new())),
        push_gate_registry: Arc::new(crate::pre_accept::builtin_gates()),
        repo_push_gates: Arc::new(Mutex::new(HashMap::new())),
    })
}
