//! In-memory implementations of port traits for development and testing.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::BudgetUsage;
use gyre_domain::{
    Agent, AgentCommit, AgentStatus, AgentUsage, AgentWorktree, AnalyticsEvent, AuditEvent,
    CostEntry, DependencyEdge, LlmFunctionConfig, MergeQueueEntry, MergeQueueEntryStatus,
    MergeRequest, MrStatus, NetworkPeer, Persona, PersonaScope, Repository, Review, ReviewComment,
    ReviewDecision, Task, TaskStatus, Tenant, User, Workspace,
};
#[cfg(test)]
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, MergeResult};
use gyre_ports::{
    AgentCommitRepository, AgentRepository, AnalyticsRepository, ApiKeyRepository, AuditRepository,
    BudgetRepository, BudgetUsageRepository, CostRepository, DependencyRepository, KvJsonStore,
    LlmConfigRepository, MergeQueueRepository, MergeRequestRepository, MetaSpecSetRepository,
    NetworkPeerRepository, PersonaRepository, RepoRepository, ReviewRepository, SpawnLogEntry,
    SpawnLogRepository, TaskRepository, TenantRepository, UserRepository,
    UserWorkspaceStateRepository, WorkspaceRepository, WorktreeRepository,
};
#[cfg(test)]
use gyre_ports::{GitOpsPort, JjChange, JjOpsPort};
use sha2::{Digest, Sha256};
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

    async fn jj_squash(&self, _repo_path: &str) -> Result<String> {
        Ok("0000000000000000000000000000000000000000".to_string())
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

    async fn find_by_task(&self, task_id: &str) -> Result<Vec<AgentCommit>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|ac| ac.task_id.as_deref() == Some(task_id))
            .cloned()
            .collect())
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

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Repository>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|r| &r.workspace_id == workspace_id)
            .cloned()
            .collect())
    }

    async fn find_by_name_and_workspace(
        &self,
        workspace_id: &Id,
        name: &str,
    ) -> Result<Option<Repository>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|r| &r.workspace_id == workspace_id && r.name == name)
            .cloned())
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

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Agent>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|a| &a.workspace_id == workspace_id)
            .cloned()
            .collect())
    }

    async fn update_status(&self, agent_id: &Id, status: AgentStatus) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(agent) = store.get_mut(agent_id.as_str()) {
            agent.status = status;
        }
        Ok(())
    }

    async fn record_usage(&self, _usage: &AgentUsage) -> Result<()> {
        // In-memory adapter: usage tracking is a no-op (not needed for tests).
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

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| &t.workspace_id == workspace_id)
            .cloned()
            .collect())
    }

    async fn list_by_spec_path(&self, spec_path: &str) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| t.spec_path.as_deref() == Some(spec_path))
            .cloned()
            .collect())
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<Task>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| &t.repo_id == repo_id)
            .cloned()
            .collect())
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

    async fn list_dependents(&self, mr_id: &Id) -> Result<Vec<Id>> {
        let store = self.store.lock().await;
        let dependents = store
            .values()
            .filter(|mr| {
                mr.depends_on
                    .iter()
                    .any(|dep| dep.as_str() == mr_id.as_str())
            })
            .map(|mr| mr.id.clone())
            .collect();
        Ok(dependents)
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<MergeRequest>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|mr| &mr.workspace_id == workspace_id)
            .cloned()
            .collect())
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

    async fn update_endpoint(&self, id: &Id, endpoint: &str) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(p) = store.iter_mut().find(|p| p.id.as_str() == id.as_str()) {
            p.endpoint = Some(endpoint.to_string());
        }
        Ok(())
    }

    async fn mark_stale_older_than(&self, cutoff: u64) -> Result<usize> {
        let mut store = self.store.lock().await;
        let mut count = 0usize;
        for p in store.iter_mut() {
            if p.is_stale {
                continue;
            }
            let stale = match p.last_seen {
                Some(ts) => ts < cutoff,
                None => p.registered_at < cutoff,
            };
            if stale {
                p.is_stale = true;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let mut store = self.store.lock().await;
        store.retain(|p| p.id.as_str() != id.as_str());
        Ok(())
    }
}

/// In-memory cross-repo dependency graph store (M22.4).
#[derive(Default)]
pub struct MemDependencyRepository {
    store: Arc<Mutex<Vec<DependencyEdge>>>,
}

#[async_trait]
impl DependencyRepository for MemDependencyRepository {
    async fn save(&self, edge: &DependencyEdge) -> Result<()> {
        let mut store = self.store.lock().await;
        store.retain(|e| e.id.as_str() != edge.id.as_str());
        store.push(edge.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<DependencyEdge>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|e| e.id.as_str() == id.as_str())
            .cloned())
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|e| e.source_repo_id.as_str() == repo_id.as_str())
            .cloned()
            .collect())
    }

    async fn list_dependents(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|e| e.target_repo_id.as_str() == repo_id.as_str())
            .cloned()
            .collect())
    }

    async fn list_all(&self) -> Result<Vec<DependencyEdge>> {
        Ok(self.store.lock().await.clone())
    }

    async fn delete(&self, id: &Id) -> Result<bool> {
        let mut store = self.store.lock().await;
        let before = store.len();
        store.retain(|e| e.id.as_str() != id.as_str());
        Ok(store.len() < before)
    }
}

/// In-memory spawn log + revoked tokens store (M13.7).
#[derive(Default)]
pub struct MemSpawnLogRepository {
    entries: Arc<Mutex<Vec<SpawnLogEntry>>>,
    revoked: Arc<Mutex<std::collections::HashSet<String>>>,
}

#[async_trait]
impl SpawnLogRepository for MemSpawnLogRepository {
    async fn append_spawn_step(
        &self,
        agent_id: &str,
        step: &str,
        status: &str,
        detail: Option<&str>,
        occurred_at: u64,
    ) -> Result<()> {
        self.entries.lock().await.push(SpawnLogEntry {
            agent_id: agent_id.to_string(),
            step: step.to_string(),
            status: status.to_string(),
            detail: detail.map(|s| s.to_string()),
            occurred_at,
        });
        Ok(())
    }

    async fn get_spawn_log(&self, agent_id: &str) -> Result<Vec<SpawnLogEntry>> {
        Ok(self
            .entries
            .lock()
            .await
            .iter()
            .filter(|e| e.agent_id == agent_id)
            .map(|e| SpawnLogEntry {
                agent_id: e.agent_id.clone(),
                step: e.step.clone(),
                status: e.status.clone(),
                detail: e.detail.clone(),
                occurred_at: e.occurred_at,
            })
            .collect())
    }

    async fn revoke_token(
        &self,
        token_hash: &str,
        _agent_id: &str,
        _revoked_at: u64,
    ) -> Result<()> {
        self.revoked.lock().await.insert(token_hash.to_string());
        Ok(())
    }

    async fn is_token_revoked(&self, token_hash: &str) -> Result<bool> {
        Ok(self.revoked.lock().await.contains(token_hash))
    }
}

#[derive(Default)]
pub struct MemTenantRepository {
    store: Arc<Mutex<HashMap<String, Tenant>>>,
}

#[async_trait]
impl TenantRepository for MemTenantRepository {
    async fn create(&self, tenant: &Tenant) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(tenant.id.to_string(), tenant.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Tenant>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|t| t.slug == slug)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Tenant>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn update(&self, tenant: &Tenant) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(tenant.id.to_string(), tenant.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemWorkspaceRepository {
    store: Arc<Mutex<HashMap<String, Workspace>>>,
}

#[async_trait]
impl WorkspaceRepository for MemWorkspaceRepository {
    async fn create(&self, workspace: &Workspace) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(workspace.id.to_string(), workspace.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Workspace>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_slug(&self, tenant_id: &Id, slug: &str) -> Result<Option<Workspace>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|ws| ws.tenant_id.as_str() == tenant_id.as_str() && ws.slug == slug)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Workspace>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<Workspace>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|ws| ws.tenant_id.as_str() == tenant_id.as_str())
            .cloned()
            .collect())
    }

    async fn update(&self, workspace: &Workspace) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(workspace.id.to_string(), workspace.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemPersonaRepository {
    store: Arc<Mutex<HashMap<String, Persona>>>,
}

#[async_trait]
impl PersonaRepository for MemPersonaRepository {
    async fn create(&self, persona: &Persona) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(persona.id.to_string(), persona.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Persona>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_slug_and_scope(
        &self,
        slug: &str,
        scope: &PersonaScope,
    ) -> Result<Option<Persona>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|p| p.slug == slug && &p.scope == scope)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Persona>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn list_by_scope(&self, scope: &PersonaScope) -> Result<Vec<Persona>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|p| &p.scope == scope)
            .cloned()
            .collect())
    }

    async fn update(&self, persona: &Persona) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(persona.id.to_string(), persona.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemPolicyRepository
// ---------------------------------------------------------------------------

/// In-memory implementation of `PolicyRepository` for testing and dev mode.
#[derive(Default)]
pub struct MemPolicyRepository {
    policies: Arc<Mutex<HashMap<String, gyre_domain::Policy>>>,
    decisions: Arc<Mutex<Vec<gyre_domain::PolicyDecision>>>,
}

#[async_trait]
impl gyre_ports::PolicyRepository for MemPolicyRepository {
    async fn create(&self, policy: &gyre_domain::Policy) -> Result<()> {
        self.policies
            .lock()
            .await
            .insert(policy.id.to_string(), policy.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<gyre_domain::Policy>> {
        Ok(self.policies.lock().await.get(id).cloned())
    }

    async fn list(&self) -> Result<Vec<gyre_domain::Policy>> {
        Ok(self.policies.lock().await.values().cloned().collect())
    }

    async fn list_by_scope(
        &self,
        scope: &gyre_domain::PolicyScope,
        scope_id: Option<&str>,
    ) -> Result<Vec<gyre_domain::Policy>> {
        Ok(self
            .policies
            .lock()
            .await
            .values()
            .filter(|p| &p.scope == scope && p.scope_id.as_deref() == scope_id)
            .cloned()
            .collect())
    }

    async fn update(&self, policy: &gyre_domain::Policy) -> Result<()> {
        self.policies
            .lock()
            .await
            .insert(policy.id.to_string(), policy.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let mut store = self.policies.lock().await;
        if let Some(p) = store.get(id) {
            if p.built_in {
                return Err(anyhow::anyhow!("cannot delete built-in policy '{id}'"));
            }
        }
        store.remove(id);
        Ok(())
    }

    async fn delete_by_name_prefix(&self, prefix: &str) -> Result<u64> {
        let mut store = self.policies.lock().await;
        let to_delete: Vec<String> = store
            .values()
            .filter(|p| p.name.starts_with(prefix))
            .map(|p| p.id.to_string())
            .collect();
        let count = to_delete.len() as u64;
        for id in to_delete {
            store.remove(&id);
        }
        Ok(count)
    }

    async fn delete_by_name_prefix_and_scope_id(
        &self,
        prefix: &str,
        scope_id: &str,
    ) -> Result<u64> {
        let mut store = self.policies.lock().await;
        let to_delete: Vec<String> = store
            .values()
            .filter(|p| p.name.starts_with(prefix) && p.scope_id.as_deref() == Some(scope_id))
            .map(|p| p.id.to_string())
            .collect();
        let count = to_delete.len() as u64;
        for id in to_delete {
            store.remove(&id);
        }
        Ok(count)
    }

    async fn record_decision(&self, decision: &gyre_domain::PolicyDecision) -> Result<()> {
        self.decisions.lock().await.push(decision.clone());
        Ok(())
    }

    async fn list_decisions(
        &self,
        subject_id: Option<&str>,
        resource_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<gyre_domain::PolicyDecision>> {
        let store = self.decisions.lock().await;
        let filtered: Vec<_> = store
            .iter()
            .filter(|d| {
                subject_id.is_none_or(|s| d.subject_id == s)
                    && resource_type.is_none_or(|r| d.resource_type == r)
            })
            .cloned()
            .collect();
        let start = if filtered.len() > limit {
            filtered.len() - limit
        } else {
            0
        };
        Ok(filtered[start..].to_vec())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// WorkspaceMembership
// ──────────────────────────────────────────────────────────────────────────────

use gyre_domain::{WorkspaceMembership, WorkspaceRole};
use gyre_ports::WorkspaceMembershipRepository;

#[derive(Default)]
pub struct MemWorkspaceMembershipRepository {
    store: Arc<Mutex<HashMap<String, WorkspaceMembership>>>,
}

#[async_trait]
impl WorkspaceMembershipRepository for MemWorkspaceMembershipRepository {
    async fn create(&self, m: &WorkspaceMembership) -> Result<()> {
        self.store.lock().await.insert(m.id.to_string(), m.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<WorkspaceMembership>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<WorkspaceMembership>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|m| m.workspace_id == *workspace_id)
            .cloned()
            .collect())
    }

    async fn list_by_user(&self, user_id: &Id) -> Result<Vec<WorkspaceMembership>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|m| m.user_id == *user_id)
            .cloned()
            .collect())
    }

    async fn find_by_user_and_workspace(
        &self,
        user_id: &Id,
        workspace_id: &Id,
    ) -> Result<Option<WorkspaceMembership>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|m| m.user_id == *user_id && m.workspace_id == *workspace_id)
            .cloned())
    }

    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()> {
        if let Some(m) = self.store.lock().await.get_mut(id.as_str()) {
            m.role = role;
        }
        Ok(())
    }

    async fn accept(&self, id: &Id, now: u64) -> Result<()> {
        if let Some(m) = self.store.lock().await.get_mut(id.as_str()) {
            m.accept(now);
        }
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Team
// ──────────────────────────────────────────────────────────────────────────────

use gyre_domain::Team;
use gyre_ports::TeamRepository;

#[derive(Default)]
pub struct MemTeamRepository {
    store: Arc<Mutex<HashMap<String, Team>>>,
}

#[async_trait]
impl TeamRepository for MemTeamRepository {
    async fn create(&self, team: &Team) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(team.id.to_string(), team.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Team>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|t| t.workspace_id == *workspace_id)
            .cloned()
            .collect())
    }

    async fn update(&self, team: &Team) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(team.id.to_string(), team.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Notification
// ──────────────────────────────────────────────────────────────────────────────

use gyre_common::Notification;
use gyre_ports::NotificationRepository;

#[derive(Default)]
pub struct MemNotificationRepository {
    store: Arc<Mutex<Vec<Notification>>>,
}

#[async_trait]
impl NotificationRepository for MemNotificationRepository {
    async fn create(&self, n: &Notification) -> Result<()> {
        self.store.lock().await.push(n.clone());
        Ok(())
    }

    async fn get(&self, id: &Id, user_id: &Id) -> Result<Option<Notification>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|n| n.id == *id && n.user_id == *user_id)
            .cloned())
    }

    async fn list_for_user(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>> {
        let store = self.store.lock().await;
        let mut items: Vec<Notification> = store
            .iter()
            .filter(|n| {
                n.user_id == *user_id
                    && workspace_id.is_none_or(|ws| n.workspace_id == *ws)
                    && min_priority.is_none_or(|min| n.priority >= min)
                    && max_priority.is_none_or(|max| n.priority <= max)
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(b.created_at.cmp(&a.created_at))
        });
        Ok(items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        for n in self.store.lock().await.iter_mut() {
            if n.id == *id && n.user_id == *user_id {
                n.dismissed_at = Some(now);
                break;
            }
        }
        Ok(())
    }

    async fn resolve(&self, id: &Id, user_id: &Id, _action_taken: Option<&str>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        for n in self.store.lock().await.iter_mut() {
            if n.id == *id && n.user_id == *user_id {
                n.resolved_at = Some(now);
                break;
            }
        }
        Ok(())
    }

    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|n| {
                n.user_id == *user_id
                    && workspace_id.is_none_or(|ws| n.workspace_id == *ws)
                    && n.resolved_at.is_none()
                    && n.dismissed_at.is_none()
            })
            .count() as u64)
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<Notification>> {
        let store = self.store.lock().await;
        let mut items: Vec<Notification> = store.iter().cloned().collect();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        items.truncate(limit);
        Ok(items)
    }

    async fn has_recent_dismissal(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        days: u32,
    ) -> Result<bool> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - (days as i64 * 86400);
        Ok(self.store.lock().await.iter().any(|n| {
            n.workspace_id == *workspace_id
                && n.user_id == *user_id
                && n.notification_type.as_str() == notification_type
                && n.dismissed_at.is_some_and(|d| d >= cutoff)
        }))
    }
}

// ── MemUserWorkspaceStateRepository ──────────────────────────────────────────

/// In-memory user workspace state repository for tests and development.
#[derive(Default)]
pub struct MemUserWorkspaceStateRepository {
    store: Mutex<HashMap<(String, String), i64>>,
}

#[async_trait]
impl UserWorkspaceStateRepository for MemUserWorkspaceStateRepository {
    async fn upsert_last_seen(
        &self,
        user_id: &str,
        workspace_id: &str,
        timestamp: i64,
    ) -> Result<()> {
        self.store
            .lock()
            .await
            .insert((user_id.to_owned(), workspace_id.to_owned()), timestamp);
        Ok(())
    }

    async fn get_last_seen(&self, user_id: &str, workspace_id: &str) -> Result<Option<i64>> {
        Ok(self
            .store
            .lock()
            .await
            .get(&(user_id.to_owned(), workspace_id.to_owned()))
            .copied())
    }
}

// ── MemKvStore ────────────────────────────────────────────────────────────────

/// In-memory implementation of KvJsonStore for tests and development.
#[derive(Default)]
pub struct MemKvStore {
    /// (namespace, key) -> value_json
    data: Mutex<HashMap<(String, String), String>>,
}

#[async_trait]
impl KvJsonStore for MemKvStore {
    async fn kv_set(&self, namespace: &str, key: &str, value: String) -> Result<()> {
        self.data
            .lock()
            .await
            .insert((namespace.to_string(), key.to_string()), value);
        Ok(())
    }

    async fn kv_get(&self, namespace: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .data
            .lock()
            .await
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn kv_remove(&self, namespace: &str, key: &str) -> Result<()> {
        self.data
            .lock()
            .await
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }

    async fn kv_list(&self, namespace: &str) -> Result<Vec<(String, String)>> {
        let guard = self.data.lock().await;
        Ok(guard
            .iter()
            .filter(|((ns, _), _)| ns == namespace)
            .map(|((_, k), v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn kv_clear(&self, namespace: &str) -> Result<()> {
        let mut guard = self.data.lock().await;
        guard.retain(|(ns, _), _| ns != namespace);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Batch-A in-memory repositories (quality gates, push gates, spec policy,
// attestation, container audit, spec ledger, spec approval event history)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemQualityGateRepository {
    gates: Arc<Mutex<HashMap<String, gyre_domain::QualityGate>>>,
}

#[async_trait]
impl gyre_ports::QualityGateRepository for MemQualityGateRepository {
    async fn save(&self, gate: &gyre_domain::QualityGate) -> Result<()> {
        self.gates
            .lock()
            .await
            .insert(gate.id.to_string(), gate.clone());
        Ok(())
    }
    async fn find_by_id(&self, id: &str) -> Result<Option<gyre_domain::QualityGate>> {
        Ok(self.gates.lock().await.get(id).cloned())
    }
    async fn list_by_repo_id(&self, repo_id: &str) -> Result<Vec<gyre_domain::QualityGate>> {
        Ok(self
            .gates
            .lock()
            .await
            .values()
            .filter(|g| g.repo_id.to_string() == repo_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, id: &str) -> Result<()> {
        self.gates.lock().await.remove(id);
        Ok(())
    }
}

// ── MemBudgetUsageRepository ──────────────────────────────────────────────────

/// In-memory BudgetUsageRepository for tests and development.
#[derive(Default)]
pub struct MemBudgetUsageRepository {
    store: Mutex<HashMap<String, BudgetUsage>>,
}

#[allow(dead_code)]
fn now_secs_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl BudgetUsageRepository for MemBudgetUsageRepository {
    async fn set_usage(&self, entity_key: &str, usage: &BudgetUsage) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(entity_key.to_string(), usage.clone());
        Ok(())
    }

    async fn get_usage(&self, entity_key: &str) -> Result<Option<BudgetUsage>> {
        Ok(self.store.lock().await.get(entity_key).cloned())
    }

    async fn delete_usage(&self, entity_key: &str) -> Result<()> {
        self.store.lock().await.remove(entity_key);
        Ok(())
    }

    async fn list_all_usage(&self) -> Result<Vec<(String, BudgetUsage)>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn increment_active(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
    ) -> Result<BudgetUsage> {
        let mut guard = self.store.lock().await;
        let usage = guard
            .entry(entity_key.to_string())
            .or_insert_with(|| BudgetUsage {
                entity_type: entity_type.to_string(),
                entity_id: gyre_common::Id::new(entity_id.to_string()),
                tokens_used_today: 0,
                cost_today: 0.0,
                active_agents: 0,
                period_start: now,
            });
        usage.active_agents = usage.active_agents.saturating_add(1);
        Ok(usage.clone())
    }

    async fn decrement_active(&self, entity_key: &str) -> Result<()> {
        let mut guard = self.store.lock().await;
        if let Some(usage) = guard.get_mut(entity_key) {
            usage.active_agents = usage.active_agents.saturating_sub(1);
        }
        Ok(())
    }

    async fn add_tokens_cost(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
        tokens: u64,
        cost_usd: f64,
    ) -> Result<()> {
        let mut guard = self.store.lock().await;
        let usage = guard
            .entry(entity_key.to_string())
            .or_insert_with(|| BudgetUsage {
                entity_type: entity_type.to_string(),
                entity_id: gyre_common::Id::new(entity_id.to_string()),
                tokens_used_today: 0,
                cost_today: 0.0,
                active_agents: 0,
                period_start: now,
            });
        usage.tokens_used_today = usage.tokens_used_today.saturating_add(tokens);
        usage.cost_today += cost_usd;
        Ok(())
    }

    async fn reset_daily_counters(&self, now: u64) -> Result<()> {
        let mut guard = self.store.lock().await;
        for usage in guard.values_mut() {
            usage.tokens_used_today = 0;
            usage.cost_today = 0.0;
            usage.period_start = now;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemGateResultRepository {
    results: Arc<Mutex<HashMap<String, gyre_domain::GateResult>>>,
}

#[async_trait]
impl gyre_ports::GateResultRepository for MemGateResultRepository {
    async fn save(&self, result: &gyre_domain::GateResult) -> Result<()> {
        self.results
            .lock()
            .await
            .insert(result.id.to_string(), result.clone());
        Ok(())
    }
    async fn update_status(
        &self,
        id: &str,
        status: gyre_domain::GateStatus,
        started_at: Option<u64>,
        finished_at: Option<u64>,
        output: Option<String>,
    ) -> Result<()> {
        if let Some(r) = self.results.lock().await.get_mut(id) {
            r.status = status;
            if let Some(s) = started_at {
                r.started_at = Some(s);
            }
            if let Some(f) = finished_at {
                r.finished_at = Some(f);
            }
            if output.is_some() {
                r.output = output;
            }
        }
        Ok(())
    }
    async fn find_by_id(&self, id: &str) -> Result<Option<gyre_domain::GateResult>> {
        Ok(self.results.lock().await.get(id).cloned())
    }
    async fn list_by_mr_id(&self, mr_id: &str) -> Result<Vec<gyre_domain::GateResult>> {
        Ok(self
            .results
            .lock()
            .await
            .values()
            .filter(|r| r.mr_id.to_string() == mr_id)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct MemPushGateRepository {
    store: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

#[async_trait]
impl gyre_ports::PushGateRepository for MemPushGateRepository {
    async fn get_for_repo(&self, repo_id: &str) -> Result<Vec<String>> {
        Ok(self
            .store
            .lock()
            .await
            .get(repo_id)
            .cloned()
            .unwrap_or_default())
    }
    async fn set_for_repo(&self, repo_id: &str, gates: Vec<String>) -> Result<()> {
        self.store.lock().await.insert(repo_id.to_string(), gates);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemSpecApprovalRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::SpecApproval>>>,
}

#[async_trait]
impl gyre_ports::SpecApprovalRepository for MemSpecApprovalRepository {
    async fn create(&self, approval: &gyre_domain::SpecApproval) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(approval.id.to_string(), approval.clone());
        Ok(())
    }
    async fn find_by_id(&self, id: &gyre_common::Id) -> Result<Option<gyre_domain::SpecApproval>> {
        Ok(self.store.lock().await.get(&id.to_string()).cloned())
    }
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<gyre_domain::SpecApproval>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|a| a.spec_path == spec_path)
            .cloned()
            .collect())
    }
    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<gyre_domain::SpecApproval>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|a| a.spec_path == spec_path && a.is_active())
            .cloned()
            .collect())
    }
    async fn list_all(&self) -> Result<Vec<gyre_domain::SpecApproval>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }
    async fn revoke(
        &self,
        id: &gyre_common::Id,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<()> {
        if let Some(a) = self.store.lock().await.get_mut(&id.to_string()) {
            a.revoked_at = Some(now);
            a.revoked_by = Some(revoked_by.to_string());
            a.revocation_reason = Some(reason.to_string());
        }
        Ok(())
    }
    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<()> {
        for a in self.store.lock().await.values_mut() {
            if a.spec_path == spec_path && a.is_active() {
                a.revoked_at = Some(now);
                a.revoked_by = Some(revoked_by.to_string());
                a.revocation_reason = Some(reason.to_string());
            }
        }
        Ok(())
    }

    async fn reject(&self, id: &Id, rejected_by: &str, reason: &str, now: u64) -> Result<()> {
        if let Some(a) = self.store.lock().await.get_mut(id.as_str()) {
            a.rejected_at = Some(now);
            a.rejected_by = Some(Id::new(rejected_by));
            a.rejected_reason = Some(reason.to_string());
        }
        Ok(())
    }
}

/// In-memory BudgetRepository (stores BudgetConfig limits by entity key).
#[derive(Default)]
pub struct MemBudgetConfigRepository {
    store: Mutex<HashMap<String, gyre_domain::BudgetConfig>>,
}

#[async_trait]
impl BudgetRepository for MemBudgetConfigRepository {
    async fn set_config(
        &self,
        entity_key: &str,
        config: &gyre_domain::BudgetConfig,
    ) -> anyhow::Result<()> {
        self.store
            .lock()
            .await
            .insert(entity_key.to_string(), config.clone());
        Ok(())
    }

    async fn get_config(
        &self,
        entity_key: &str,
    ) -> anyhow::Result<Option<gyre_domain::BudgetConfig>> {
        Ok(self.store.lock().await.get(entity_key).cloned())
    }

    async fn delete_config(&self, entity_key: &str) -> anyhow::Result<()> {
        self.store.lock().await.remove(entity_key);
        Ok(())
    }

    async fn list_all(&self) -> anyhow::Result<Vec<(String, gyre_domain::BudgetConfig)>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
}

#[derive(Default)]
pub struct MemSpecPolicyRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::SpecPolicy>>>,
}

#[async_trait]
impl gyre_ports::SpecPolicyRepository for MemSpecPolicyRepository {
    async fn get_for_repo(&self, repo_id: &str) -> Result<gyre_domain::SpecPolicy> {
        Ok(self
            .store
            .lock()
            .await
            .get(repo_id)
            .cloned()
            .unwrap_or_default())
    }
    async fn set_for_repo(&self, repo_id: &str, policy: gyre_domain::SpecPolicy) -> Result<()> {
        self.store.lock().await.insert(repo_id.to_string(), policy);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemAttestationRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::AttestationBundle>>>,
}

#[async_trait]
impl gyre_ports::AttestationRepository for MemAttestationRepository {
    async fn find_by_mr_id(&self, mr_id: &str) -> Result<Option<gyre_domain::AttestationBundle>> {
        Ok(self.store.lock().await.get(mr_id).cloned())
    }
    async fn save(&self, mr_id: &str, bundle: &gyre_domain::AttestationBundle) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(mr_id.to_string(), bundle.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemContainerAuditRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::ContainerAuditRecord>>>,
}

#[async_trait]
impl gyre_ports::ContainerAuditRepository for MemContainerAuditRepository {
    async fn find_by_agent_id(
        &self,
        agent_id: &str,
    ) -> Result<Option<gyre_domain::ContainerAuditRecord>> {
        Ok(self.store.lock().await.get(agent_id).cloned())
    }
    async fn save(&self, record: &gyre_domain::ContainerAuditRecord) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(record.agent_id.clone(), record.clone());
        Ok(())
    }
    async fn update_exit(
        &self,
        agent_id: &str,
        exit_code: Option<i32>,
        stopped_at: Option<u64>,
    ) -> Result<()> {
        if let Some(r) = self.store.lock().await.get_mut(agent_id) {
            r.exit_code = exit_code;
            r.stopped_at = stopped_at;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemSpecLedgerRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::SpecLedgerEntry>>>,
}

#[async_trait]
impl gyre_ports::SpecLedgerRepository for MemSpecLedgerRepository {
    async fn find_by_path(&self, path: &str) -> Result<Option<gyre_domain::SpecLedgerEntry>> {
        Ok(self.store.lock().await.get(path).cloned())
    }
    async fn list_all(&self) -> Result<Vec<gyre_domain::SpecLedgerEntry>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }
    async fn save(&self, entry: &gyre_domain::SpecLedgerEntry) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(entry.path.clone(), entry.clone());
        Ok(())
    }
    async fn delete_by_path(&self, path: &str) -> Result<()> {
        self.store.lock().await.remove(path);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemSpecApprovalEventRepository {
    store: Arc<Mutex<Vec<gyre_domain::SpecApprovalEvent>>>,
}

#[async_trait]
impl gyre_ports::SpecApprovalEventRepository for MemSpecApprovalEventRepository {
    async fn record(&self, event: &gyre_domain::SpecApprovalEvent) -> Result<()> {
        self.store.lock().await.push(event.clone());
        Ok(())
    }
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<gyre_domain::SpecApprovalEvent>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|e| e.spec_path == spec_path)
            .cloned()
            .collect())
    }
    async fn list_all(&self) -> Result<Vec<gyre_domain::SpecApprovalEvent>> {
        Ok(self.store.lock().await.clone())
    }
    async fn revoke_event(
        &self,
        id: &str,
        revoked_at: u64,
        revoked_by: &str,
        reason: &str,
    ) -> Result<()> {
        for e in self.store.lock().await.iter_mut() {
            if e.id == id {
                e.revoked_at = Some(revoked_at);
                e.revoked_by = Some(revoked_by.to_string());
                e.revocation_reason = Some(reason.to_string());
                break;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemMetaSpecSetRepository {
    store: Arc<Mutex<HashMap<String, String>>>,
}

#[async_trait]
impl MetaSpecSetRepository for MemMetaSpecSetRepository {
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>> {
        Ok(self.store.lock().await.get(workspace_id.as_str()).cloned())
    }

    async fn upsert(&self, workspace_id: &Id, json: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(workspace_id.as_str().to_string(), json.to_string());
        Ok(())
    }

    async fn delete(&self, workspace_id: &Id) -> Result<()> {
        self.store.lock().await.remove(workspace_id.as_str());
        Ok(())
    }
}

// ── MemLlmConfigRepository ────────────────────────────────────────────────────

type LlmConfigKey = (Option<String>, String); // (workspace_id, function_key)

#[derive(Default)]
pub struct MemLlmConfigRepository {
    store: Arc<Mutex<HashMap<LlmConfigKey, LlmFunctionConfig>>>,
}

#[async_trait]
impl LlmConfigRepository for MemLlmConfigRepository {
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<LlmFunctionConfig>> {
        let guard = self.store.lock().await;
        // Workspace override first.
        if let Some(cfg) = guard.get(&(
            Some(workspace_id.as_str().to_string()),
            function_key.to_string(),
        )) {
            return Ok(Some(cfg.clone()));
        }
        // Tenant default.
        Ok(guard.get(&(None, function_key.to_string())).cloned())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<LlmFunctionConfig>> {
        let guard = self.store.lock().await;
        let ws_str = workspace_id.as_str().to_string();
        Ok(guard
            .values()
            .filter(|cfg| cfg.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws_str.as_str()))
            .cloned()
            .collect())
    }

    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig> {
        let cfg = LlmFunctionConfig {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: Some(workspace_id.clone()),
            function_key: function_key.to_string(),
            model_name: model_name.to_string(),
            max_tokens,
            updated_by: updated_by.clone(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let key = (
            Some(workspace_id.as_str().to_string()),
            function_key.to_string(),
        );
        self.store.lock().await.insert(key, cfg.clone());
        Ok(cfg)
    }

    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig> {
        let cfg = LlmFunctionConfig {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: None,
            function_key: function_key.to_string(),
            model_name: model_name.to_string(),
            max_tokens,
            updated_by: updated_by.clone(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let key = (None, function_key.to_string());
        self.store.lock().await.insert(key, cfg.clone());
        Ok(cfg)
    }

    async fn delete_workspace_override(&self, workspace_id: &Id, function_key: &str) -> Result<()> {
        let key = (
            Some(workspace_id.as_str().to_string()),
            function_key.to_string(),
        );
        self.store.lock().await.remove(&key);
        Ok(())
    }

    async fn list_tenant_defaults(&self) -> Result<Vec<LlmFunctionConfig>> {
        let guard = self.store.lock().await;
        Ok(guard
            .values()
            .filter(|cfg| cfg.workspace_id.is_none())
            .cloned()
            .collect())
    }
}

/// sha -> (agent_id, workspace_id, tenant_id, blob)
type ConvMap = Arc<Mutex<HashMap<String, (String, String, String, Vec<u8>)>>>;

/// In-memory ConversationRepository for development and tests.
pub struct MemConversationRepository {
    convs: ConvMap,
    links: Arc<Mutex<Vec<gyre_common::TurnCommitLink>>>,
}

impl Default for MemConversationRepository {
    fn default() -> Self {
        Self {
            convs: Arc::new(Mutex::new(HashMap::new())),
            links: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl gyre_ports::ConversationRepository for MemConversationRepository {
    async fn store(
        &self,
        agent_id: &Id,
        workspace_id: &Id,
        tenant_id: &Id,
        conversation: &[u8],
    ) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(conversation);
        let sha = hex::encode(hasher.finalize());
        self.convs
            .lock()
            .await
            .entry(sha.clone())
            .or_insert_with(|| {
                (
                    agent_id.as_str().to_string(),
                    workspace_id.as_str().to_string(),
                    tenant_id.as_str().to_string(),
                    conversation.to_vec(),
                )
            });
        Ok(sha)
    }

    async fn get(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<Vec<u8>>> {
        let guard = self.convs.lock().await;
        let Some((_, _, tid, blob)) = guard.get(conversation_sha) else {
            return Ok(None);
        };
        if tid != tenant_id.as_str() {
            return Ok(None);
        }
        // Decompress.
        let decompressed = zstd::decode_all(blob.as_slice())
            .map_err(|e| anyhow::anyhow!("zstd decompress: {e}"))?;
        Ok(Some(decompressed))
    }

    async fn record_turn_link(&self, link: &gyre_common::TurnCommitLink) -> Result<()> {
        self.links.lock().await.push(link.clone());
        Ok(())
    }

    async fn get_turn_links(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Vec<gyre_common::TurnCommitLink>> {
        let guard = self.links.lock().await;
        Ok(guard
            .iter()
            .filter(|l| {
                l.conversation_sha.as_deref() == Some(conversation_sha)
                    && l.tenant_id.as_str() == tenant_id.as_str()
            })
            .cloned()
            .collect())
    }

    async fn get_metadata(
        &self,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<Option<(Id, Id)>> {
        let guard = self.convs.lock().await;
        let Some((aid, wid, tid, _)) = guard.get(conversation_sha) else {
            return Ok(None);
        };
        if tid != tenant_id.as_str() {
            return Ok(None);
        }
        Ok(Some((Id::new(aid), Id::new(wid))))
    }

    async fn list_by_agent(&self, agent_id: &Id, tenant_id: &Id) -> Result<Vec<String>> {
        let guard = self.convs.lock().await;
        Ok(guard
            .iter()
            .filter(|(_, (aid, _, tid, _))| aid == agent_id.as_str() && tid == tenant_id.as_str())
            .map(|(sha, _)| sha.clone())
            .collect())
    }

    async fn backfill_turn_links(
        &self,
        agent_id: &Id,
        conversation_sha: &str,
        tenant_id: &Id,
    ) -> Result<u64> {
        let mut guard = self.links.lock().await;
        let mut count = 0u64;
        for link in guard.iter_mut() {
            if link.agent_id.as_str() == agent_id.as_str()
                && link.tenant_id.as_str() == tenant_id.as_str()
                && link.conversation_sha.is_none()
            {
                link.conversation_sha = Some(conversation_sha.to_string());
                count += 1;
            }
        }
        Ok(count)
    }
}

/// In-memory MessageRepository for tests.
pub struct MemMessageRepository {
    store: Arc<Mutex<HashMap<String, gyre_common::message::Message>>>,
}

impl Default for MemMessageRepository {
    fn default() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl gyre_ports::MessageRepository for MemMessageRepository {
    async fn store(&self, message: &gyre_common::message::Message) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(message.id.as_str().to_string(), message.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<gyre_common::message::Message>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn list_after(
        &self,
        agent_id: &Id,
        after_ts: u64,
        after_id: Option<&Id>,
        limit: usize,
    ) -> Result<Vec<gyre_common::message::Message>> {
        use gyre_common::message::Destination;
        let guard = self.store.lock().await;
        let mut msgs: Vec<_> = guard
            .values()
            .filter(|m| matches!(&m.to, Destination::Agent(id) if id == agent_id))
            .filter(|m| match after_id {
                Some(aid) => {
                    m.created_at > after_ts
                        || (m.created_at == after_ts && m.id.as_str() > aid.as_str())
                }
                None => m.created_at > after_ts,
            })
            .cloned()
            .collect();
        msgs.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then(a.id.as_str().cmp(b.id.as_str()))
        });
        msgs.truncate(limit);
        Ok(msgs)
    }

    async fn list_unacked(
        &self,
        agent_id: &Id,
        limit: usize,
    ) -> Result<Vec<gyre_common::message::Message>> {
        use gyre_common::message::Destination;
        let guard = self.store.lock().await;
        let mut msgs: Vec<_> = guard
            .values()
            .filter(|m| matches!(&m.to, Destination::Agent(id) if id == agent_id))
            .filter(|m| !m.acknowledged)
            .cloned()
            .collect();
        msgs.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then(a.id.as_str().cmp(b.id.as_str()))
        });
        msgs.truncate(limit);
        Ok(msgs)
    }

    async fn count_unacked(&self, agent_id: &Id) -> Result<u64> {
        use gyre_common::message::Destination;
        let guard = self.store.lock().await;
        let count = guard
            .values()
            .filter(|m| matches!(&m.to, Destination::Agent(id) if id == agent_id))
            .filter(|m| !m.acknowledged)
            .count();
        Ok(count as u64)
    }

    async fn acknowledge(&self, message_id: &Id, agent_id: &Id) -> Result<()> {
        use gyre_common::message::Destination;
        let mut guard = self.store.lock().await;
        if let Some(m) = guard.get_mut(message_id.as_str()) {
            if matches!(&m.to, Destination::Agent(id) if id == agent_id) {
                m.acknowledged = true;
            }
        }
        Ok(())
    }

    async fn acknowledge_all(&self, agent_id: &Id, _reason: &str) -> Result<u64> {
        use gyre_common::message::Destination;
        let mut guard = self.store.lock().await;
        let mut count = 0u64;
        for m in guard.values_mut() {
            if matches!(&m.to, Destination::Agent(id) if id == agent_id) && !m.acknowledged {
                m.acknowledged = true;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn list_by_workspace(
        &self,
        workspace_id: &Id,
        kind: Option<&str>,
        since: Option<u64>,
        before_ts: Option<u64>,
        before_id: Option<&Id>,
        limit: Option<usize>,
    ) -> Result<Vec<gyre_common::message::Message>> {
        use gyre_common::message::Destination;
        let guard = self.store.lock().await;
        let mut msgs: Vec<_> = guard
            .values()
            .filter(|m| {
                m.workspace_id
                    .as_ref()
                    .map(|ws| ws == workspace_id)
                    .unwrap_or(false)
            })
            .filter(|m| !matches!(&m.to, Destination::Agent(_)))
            .filter(|m| kind.map(|k| m.kind.as_str() == k).unwrap_or(true))
            .filter(|m| since.map(|s| m.created_at >= s).unwrap_or(true))
            .filter(|m| match (before_ts, before_id) {
                (Some(bts), Some(bid)) => {
                    m.created_at < bts || (m.created_at == bts && m.id.as_str() < bid.as_str())
                }
                (Some(bts), None) => m.created_at < bts,
                _ => true,
            })
            .cloned()
            .collect();
        msgs.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then(b.id.as_str().cmp(a.id.as_str()))
        });
        if let Some(lim) = limit {
            msgs.truncate(lim);
        }
        Ok(msgs)
    }

    async fn expire_events(&self, older_than: u64) -> Result<u64> {
        use gyre_common::message::Destination;
        let mut guard = self.store.lock().await;
        let before = guard.len();
        guard.retain(|_, m| matches!(&m.to, Destination::Agent(_)) || m.created_at >= older_than);
        Ok((before - guard.len()) as u64)
    }

    async fn expire_acked_inboxes(&self, older_than: u64) -> Result<u64> {
        let mut guard = self.store.lock().await;
        let before = guard.len();
        guard.retain(|_, m| !m.acknowledged || m.created_at >= older_than);
        Ok((before - guard.len()) as u64)
    }

    async fn expire_for_agents(&self, agent_ids: &[Id], older_than: u64) -> Result<u64> {
        use gyre_common::message::Destination;
        let mut guard = self.store.lock().await;
        let before = guard.len();
        guard.retain(|_, m| {
            let is_target = matches!(&m.to, Destination::Agent(id) if agent_ids.contains(id));
            !(is_target && m.created_at < older_than)
        });
        Ok((before - guard.len()) as u64)
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
        telemetry_buffer: Arc::new(gyre_common::message::TelemetryBuffer::new(1_000, 10)),
        message_broadcast_tx: broadcast::channel(16).0,
        kv_store: Arc::new(MemKvStore::default()),
        agent_signing_key: Arc::new(crate::auth::AgentSigningKey::generate()),
        agent_jwt_ttl_secs: 3600,
        users: Arc::new(MemUserRepository::default()),
        api_keys: Arc::new(MemApiKeyRepository::default()),
        jwt_config: None,
        http_client: reqwest::Client::new(),
        metrics: Arc::new(crate::metrics::Metrics::new().expect("test metrics")),
        started_at_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        compose_sessions: Arc::new(Mutex::new(HashMap::new())),
        retention_store: crate::retention::RetentionStore::new(),
        job_registry: Arc::new(crate::jobs::JobRegistry::new()),
        analytics: Arc::new(MemAnalyticsRepository::default()),
        costs: Arc::new(MemCostRepository::default()),
        audit: Arc::new(MemAuditRepository::default()),
        siem_store: crate::siem::SiemStore::new(),
        audit_broadcast_tx: broadcast::channel(64).0,
        network_peers: Arc::new(MemNetworkPeerRepository::default()),
        dependencies: Arc::new(MemDependencyRepository::default()),
        rate_limiter: crate::rate_limit::RateLimiter::new(1000),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(Mutex::new(HashMap::new())),
        agent_log_tx: Arc::new(Mutex::new(HashMap::new())),
        quality_gates: Arc::new(MemQualityGateRepository::default()),
        gate_results: Arc::new(MemGateResultRepository::default()),
        push_gate_registry: Arc::new(crate::pre_accept::builtin_gates()),
        repo_push_gates: Arc::new(MemPushGateRepository::default()),
        speculative_results: Arc::new(Mutex::new(HashMap::new())),
        spawn_log: Arc::new(MemSpawnLogRepository::default()),
        db_storage: None,
        spec_approvals: Arc::new(MemSpecApprovalRepository::default()),
        spec_policies: Arc::new(MemSpecPolicyRepository::default()),
        attestation_store: Arc::new(MemAttestationRepository::default()),
        trusted_issuers: vec![],
        remote_jwks_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        commit_signatures: Arc::new(Mutex::new(HashMap::new())),
        sigstore_mode: crate::commit_signatures::SigstoreMode::Local,
        tunnel_store: Arc::new(Mutex::new(HashMap::new())),
        container_audits: Arc::new(MemContainerAuditRepository::default()),
        spec_ledger: Arc::new(MemSpecLedgerRepository::default()),
        spec_approval_history: Arc::new(MemSpecApprovalEventRepository::default()),
        spec_links_store: Arc::new(Mutex::new(Vec::new())),
        budget_configs: Arc::new(MemBudgetConfigRepository::default()),
        budget_usages: Arc::new(MemBudgetUsageRepository::default()),
        search: Arc::new(gyre_adapters::MemSearchAdapter::new()),
        tenants: Arc::new(MemTenantRepository::default()),
        workspaces: Arc::new(MemWorkspaceRepository::default()),
        personas: Arc::new(MemPersonaRepository::default()),
        policies: Arc::new(MemPolicyRepository::default()),
        workspace_memberships: Arc::new(MemWorkspaceMembershipRepository::default()),
        teams: Arc::new(MemTeamRepository::default()),
        notifications: Arc::new(MemNotificationRepository::default()),
        graph_store: Arc::new(gyre_adapters::MemGraphStore::new()),
        saved_views: Arc::new(gyre_adapters::MemSavedViewRepository::default()),
        wg_config: crate::WireGuardConfig::from_env(),
        meta_specs: Arc::new(MemMetaSpecRepository::default()),
        meta_spec_bindings: Arc::new(MemMetaSpecBindingRepository::default()),
        meta_spec_sets: Arc::new(MemMetaSpecSetRepository::default()),
        messages: Arc::new(MemMessageRepository::default()),
        message_dispatch_tx: {
            let (tx, rx) = tokio::sync::mpsc::channel(256);
            tokio::spawn(async move {
                let mut rx = rx;
                while rx.recv().await.is_some() {}
            });
            tx
        },
        agent_inbox_max: 1000,
        user_workspace_state: Arc::new(MemUserWorkspaceStateRepository::default()),
        last_seen_debounce: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        llm_rate_limiter: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_configs: Arc::new(MemLlmConfigRepository::default()),
        presence: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        ws_connections: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        ws_connection_counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        ws_connection_workspaces: Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        traces: Arc::new(MemTraceRepository::default()),
        otlp_config: crate::otlp_receiver::OtlpServerConfig {
            enabled: false,
            grpc_port: 4317,
            max_spans_per_trace: 10_000,
        },
        conversations: Arc::new(MemConversationRepository::default()),
        // Use a non-existent path that unit tests will never actually access via real git.
        // NoopGitOps does not create files; commits_since() on a missing path returns 0.
        repos_root: format!("/tmp/gyre-unit-test-repos-{}", std::process::id()),
        prompt_templates: Arc::new(MemPromptRepository::default()),
        compute_targets: Arc::new(MemComputeTargetRepository::default()),
        llm: Some(Arc::new(gyre_adapters::MockLlmPortFactory::echo())),
        user_notification_prefs: Arc::new(MemUserNotificationPreferenceRepository::default()),
        user_tokens: Arc::new(MemUserTokenRepository::default()),
        judgment_ledger: Arc::new(MemJudgmentLedgerRepository),
    })
}

// ── In-memory TraceRepository ────────────────────────────────────────────────

#[derive(Default)]
pub struct MemTraceRepository {
    store: Arc<Mutex<HashMap<String, gyre_common::GateTrace>>>,
    payloads: Arc<Mutex<HashMap<(String, String), gyre_ports::trace::SpanPayload>>>,
}

#[async_trait]
impl gyre_ports::TraceRepository for MemTraceRepository {
    async fn store(&self, trace: &gyre_common::GateTrace) -> Result<()> {
        let mut guard = self.store.lock().await;
        // Replace any existing trace for same MR (capped at most recent).
        guard.retain(|_, v| v.mr_id != trace.mr_id);
        guard.insert(trace.mr_id.as_str().to_string(), trace.clone());
        Ok(())
    }

    async fn get_by_mr(&self, mr_id: &Id) -> Result<Option<gyre_common::GateTrace>> {
        Ok(self.store.lock().await.get(mr_id.as_str()).cloned())
    }

    async fn get_span_payload(
        &self,
        gate_run_id: &Id,
        span_id: &str,
    ) -> Result<Option<gyre_ports::trace::SpanPayload>> {
        let guard = self.payloads.lock().await;
        let key = (gate_run_id.as_str().to_string(), span_id.to_string());
        Ok(guard.get(&key).map(|p| gyre_ports::trace::SpanPayload {
            input: p.input.clone(),
            output: p.output.clone(),
        }))
    }

    async fn promote_to_attestation(&self, _mr_id: &Id) -> Result<()> {
        Ok(()) // no-op for in-memory (no eviction logic needed in tests)
    }

    async fn delete_by_mr(&self, mr_id: &Id) -> Result<()> {
        self.store.lock().await.remove(mr_id.as_str());
        Ok(())
    }
}

// ── In-memory PromptRepository ────────────────────────────────────────────────

#[derive(Default)]
pub struct MemPromptRepository {
    templates: Arc<tokio::sync::RwLock<Vec<gyre_domain::PromptTemplate>>>,
}

#[async_trait]
impl gyre_ports::PromptRepository for MemPromptRepository {
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<gyre_domain::PromptTemplate>> {
        let guard = self.templates.read().await;
        // Workspace override first
        if let Some(t) = guard.iter().find(|t| {
            t.workspace_id.as_ref() == Some(workspace_id) && t.function_key == function_key
        }) {
            return Ok(Some(t.clone()));
        }
        // Tenant default
        Ok(guard
            .iter()
            .find(|t| t.workspace_id.is_none() && t.function_key == function_key)
            .cloned())
    }

    async fn list_by_workspace(
        &self,
        workspace_id: &Id,
    ) -> Result<Vec<gyre_domain::PromptTemplate>> {
        let guard = self.templates.read().await;
        Ok(guard
            .iter()
            .filter(|t| t.workspace_id.as_ref() == Some(workspace_id))
            .cloned()
            .collect())
    }

    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<gyre_domain::PromptTemplate> {
        let mut guard = self.templates.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(existing) = guard.iter_mut().find(|t| {
            t.workspace_id.as_ref() == Some(workspace_id) && t.function_key == function_key
        }) {
            existing.content = content.to_string();
            existing.updated_at = now;
            return Ok(existing.clone());
        }
        let tmpl = gyre_domain::PromptTemplate {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: Some(workspace_id.clone()),
            function_key: function_key.to_string(),
            content: content.to_string(),
            created_by: created_by.clone(),
            created_at: now,
            updated_at: now,
        };
        guard.push(tmpl.clone());
        Ok(tmpl)
    }

    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<gyre_domain::PromptTemplate> {
        let mut guard = self.templates.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(existing) = guard
            .iter_mut()
            .find(|t| t.workspace_id.is_none() && t.function_key == function_key)
        {
            existing.content = content.to_string();
            existing.updated_at = now;
            return Ok(existing.clone());
        }
        let tmpl = gyre_domain::PromptTemplate {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: None,
            function_key: function_key.to_string(),
            content: content.to_string(),
            created_by: created_by.clone(),
            created_at: now,
            updated_at: now,
        };
        guard.push(tmpl.clone());
        Ok(tmpl)
    }

    async fn delete_workspace_override(&self, workspace_id: &Id, function_key: &str) -> Result<()> {
        let mut guard = self.templates.write().await;
        guard.retain(|t| {
            !(t.workspace_id.as_ref() == Some(workspace_id) && t.function_key == function_key)
        });
        Ok(())
    }
}

// ── In-memory ComputeTargetRepository ────────────────────────────────────────

#[derive(Default)]
pub struct MemComputeTargetRepository {
    store: Arc<tokio::sync::RwLock<Vec<gyre_domain::ComputeTargetEntity>>>,
}

#[async_trait]
impl gyre_ports::ComputeTargetRepository for MemComputeTargetRepository {
    async fn create(&self, target: &gyre_domain::ComputeTargetEntity) -> Result<()> {
        self.store.write().await.push(target.clone());
        Ok(())
    }

    async fn get_by_id(&self, id: &Id) -> Result<Option<gyre_domain::ComputeTargetEntity>> {
        Ok(self
            .store
            .read()
            .await
            .iter()
            .find(|t| &t.id == id)
            .cloned())
    }

    async fn list_by_tenant(
        &self,
        tenant_id: &Id,
    ) -> Result<Vec<gyre_domain::ComputeTargetEntity>> {
        let mut result: Vec<_> = self
            .store
            .read()
            .await
            .iter()
            .filter(|t| &t.tenant_id == tenant_id)
            .cloned()
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    async fn update(&self, target: &gyre_domain::ComputeTargetEntity) -> Result<()> {
        let mut guard = self.store.write().await;
        if let Some(existing) = guard.iter_mut().find(|t| t.id == target.id) {
            *existing = target.clone();
        }
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.write().await.retain(|t| &t.id != id);
        Ok(())
    }

    async fn get_default_for_tenant(
        &self,
        tenant_id: &Id,
    ) -> Result<Option<gyre_domain::ComputeTargetEntity>> {
        Ok(self
            .store
            .read()
            .await
            .iter()
            .find(|t| &t.tenant_id == tenant_id && t.is_default)
            .cloned())
    }

    async fn has_workspace_references(&self, _id: &Id) -> Result<bool> {
        // The in-memory adapter does not share state with MemWorkspaceRepository,
        // so this always returns false in tests. The 409 Conflict path is covered
        // by the SQLite adapter integration tests.
        Ok(false)
    }
}

// ─── MemUserNotificationPreferenceRepository ─────────────────────────────────

#[derive(Default)]
pub struct MemUserNotificationPreferenceRepository {
    prefs: Arc<tokio::sync::RwLock<Vec<gyre_domain::UserNotificationPreference>>>,
}

#[async_trait]
impl gyre_ports::UserNotificationPreferenceRepository for MemUserNotificationPreferenceRepository {
    async fn list_for_user(
        &self,
        user_id: &Id,
    ) -> Result<Vec<gyre_domain::UserNotificationPreference>> {
        let guard = self.prefs.read().await;
        Ok(guard
            .iter()
            .filter(|p| &p.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, pref: &gyre_domain::UserNotificationPreference) -> Result<()> {
        let mut guard = self.prefs.write().await;
        if let Some(existing) = guard
            .iter_mut()
            .find(|p| p.user_id == pref.user_id && p.notification_type == pref.notification_type)
        {
            existing.enabled = pref.enabled;
        } else {
            guard.push(pref.clone());
        }
        Ok(())
    }

    async fn upsert_batch(&self, prefs: &[gyre_domain::UserNotificationPreference]) -> Result<()> {
        for pref in prefs {
            self.upsert(pref).await?;
        }
        Ok(())
    }
}

// ── In-memory MetaSpecRepository ─────────────────────────────────────────────

#[derive(Default)]
pub struct MemMetaSpecRepository {
    store: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpec>>>,
    versions: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecVersion>>>,
}

#[async_trait]
impl gyre_ports::MetaSpecRepository for MemMetaSpecRepository {
    async fn create(&self, meta_spec: &gyre_domain::MetaSpec) -> Result<()> {
        self.store.write().await.push(meta_spec.clone());
        Ok(())
    }

    async fn get_by_id(&self, id: &Id) -> Result<Option<gyre_domain::MetaSpec>> {
        Ok(self
            .store
            .read()
            .await
            .iter()
            .find(|m| &m.id == id)
            .cloned())
    }

    async fn list(
        &self,
        filter: &gyre_ports::MetaSpecFilter,
    ) -> Result<Vec<gyre_domain::MetaSpec>> {
        let guard = self.store.read().await;
        Ok(guard
            .iter()
            .filter(|m| {
                if let Some(ref scope) = filter.scope {
                    if &m.scope != scope {
                        return false;
                    }
                }
                if let Some(ref scope_id) = filter.scope_id {
                    if m.scope_id.as_deref() != Some(scope_id.as_str()) {
                        return false;
                    }
                }
                if let Some(ref kind) = filter.kind {
                    if &m.kind != kind {
                        return false;
                    }
                }
                if let Some(required) = filter.required {
                    if m.required != required {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect())
    }

    async fn update(&self, meta_spec: &gyre_domain::MetaSpec) -> Result<()> {
        let mut store = self.store.write().await;
        let mut versions = self.versions.write().await;
        if let Some(existing) = store.iter().find(|m| m.id == meta_spec.id).cloned() {
            // Archive old version.
            let ver = gyre_domain::MetaSpecVersion {
                id: Id::new(uuid::Uuid::new_v4().to_string()),
                meta_spec_id: existing.id.clone(),
                version: existing.version,
                prompt: existing.prompt.clone(),
                content_hash: existing.content_hash.clone(),
                created_at: existing.updated_at,
            };
            versions.push(ver);
        }
        store.retain(|m| m.id != meta_spec.id);
        store.push(meta_spec.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.write().await.retain(|m| &m.id != id);
        Ok(())
    }

    async fn list_versions(&self, meta_spec_id: &Id) -> Result<Vec<gyre_domain::MetaSpecVersion>> {
        Ok(self
            .versions
            .read()
            .await
            .iter()
            .filter(|v| &v.meta_spec_id == meta_spec_id)
            .cloned()
            .collect())
    }

    async fn get_version(
        &self,
        meta_spec_id: &Id,
        version: u32,
    ) -> Result<Option<gyre_domain::MetaSpecVersion>> {
        // Check archive first.
        let archived = self
            .versions
            .read()
            .await
            .iter()
            .find(|v| &v.meta_spec_id == meta_spec_id && v.version == version)
            .cloned();
        if archived.is_some() {
            return Ok(archived);
        }
        // Fall back to live row for current version.
        Ok(self
            .store
            .read()
            .await
            .iter()
            .find(|m| &m.id == meta_spec_id && m.version == version)
            .map(|m| gyre_domain::MetaSpecVersion {
                id: m.id.clone(),
                meta_spec_id: m.id.clone(),
                version: m.version,
                prompt: m.prompt.clone(),
                content_hash: m.content_hash.clone(),
                created_at: m.updated_at,
            }))
    }
}

// ─── MemUserTokenRepository ──────────────────────────────────────────────────

#[derive(Default)]
pub struct MemUserTokenRepository {
    tokens: Arc<tokio::sync::RwLock<Vec<gyre_domain::UserToken>>>,
}

#[async_trait]
impl gyre_ports::UserTokenRepository for MemUserTokenRepository {
    async fn create(&self, token: &gyre_domain::UserToken) -> Result<()> {
        self.tokens.write().await.push(token.clone());
        Ok(())
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<gyre_domain::UserToken>> {
        let guard = self.tokens.read().await;
        Ok(guard
            .iter()
            .filter(|t| &t.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<gyre_domain::UserToken>> {
        Ok(self
            .tokens
            .read()
            .await
            .iter()
            .find(|t| &t.id == id)
            .cloned())
    }

    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<gyre_domain::UserToken>> {
        Ok(self
            .tokens
            .read()
            .await
            .iter()
            .find(|t| t.token_hash == token_hash)
            .cloned())
    }

    async fn touch(&self, id: &Id, last_used_at: u64) -> Result<()> {
        let mut guard = self.tokens.write().await;
        if let Some(t) = guard.iter_mut().find(|t| &t.id == id) {
            t.last_used_at = Some(last_used_at);
        }
        Ok(())
    }

    async fn delete(&self, id: &Id, user_id: &Id) -> Result<()> {
        let mut guard = self.tokens.write().await;
        guard.retain(|t| !(t.id == *id && t.user_id == *user_id));
        Ok(())
    }
}

// ─── MemJudgmentLedgerRepository ─────────────────────────────────────────────

#[derive(Default)]
pub struct MemJudgmentLedgerRepository;

#[async_trait]
impl gyre_ports::JudgmentLedgerRepository for MemJudgmentLedgerRepository {
    async fn list_for_user(
        &self,
        _approver_id: &str,
        _workspace_id: Option<&Id>,
        _judgment_type: Option<gyre_domain::JudgmentType>,
        _since: Option<u64>,
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<gyre_domain::JudgmentEntry>> {
        Ok(vec![])
    }
}

// ── In-memory MetaSpecBindingRepository ──────────────────────────────────────

#[derive(Default)]
pub struct MemMetaSpecBindingRepository {
    store: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecBinding>>>,
}

#[async_trait]
impl gyre_ports::MetaSpecBindingRepository for MemMetaSpecBindingRepository {
    async fn create(&self, binding: &gyre_domain::MetaSpecBinding) -> Result<()> {
        self.store.write().await.push(binding.clone());
        Ok(())
    }

    async fn list_by_spec_id(&self, spec_id: &str) -> Result<Vec<gyre_domain::MetaSpecBinding>> {
        Ok(self
            .store
            .read()
            .await
            .iter()
            .filter(|b| b.spec_id == spec_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.write().await.retain(|b| &b.id != id);
        Ok(())
    }

    async fn has_bindings_for(&self, meta_spec_id: &Id) -> Result<bool> {
        Ok(self
            .store
            .read()
            .await
            .iter()
            .any(|b| &b.meta_spec_id == meta_spec_id))
    }
}
