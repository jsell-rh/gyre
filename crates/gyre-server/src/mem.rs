//! In-memory implementations of port traits for development and testing.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::spec_approval::ApprovalTransitionError;
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
    AgentCommitRepository, AgentRepository, AnalyticsQueryFilter, AnalyticsRepository,
    ApiKeyRepository, AuditQueryFilter, AuditRepository, BudgetRepository, BudgetUsageRepository,
    CostRepository, DependencyRepository, KvJsonStore, LlmConfigRepository, MergeQueueRepository,
    MergeRequestRepository, MetaSpecSetRepository, NetworkPeerRepository, PersonaRepository,
    RepoRepository, ReviewRepository, RolloutPolicyRepository, SpawnLogEntry, SpawnLogRepository,
    TaskRepository, TenantMetaSpecSetRepository, TenantRepository, UserRepository,
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

    async fn create_detached_worktree(
        &self,
        _repo_path: &str,
        _worktree_path: &str,
        _sha: &str,
    ) -> Result<()> {
        // In-memory mode has no git repository on disk; post-merge gates fall
        // back to running in the server cwd (task-095 R2-1).
        Err(anyhow::anyhow!(
            "create_detached_worktree not supported in mem mode"
        ))
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

    async fn branch_exists(&self, _repo_path: &str, _branch_name: &str) -> Result<bool> {
        Ok(false)
    }

    async fn create_branch(
        &self,
        _repo_path: &str,
        _branch_name: &str,
        _from_ref: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn write_file(
        &self,
        _repo_path: &str,
        _branch: &str,
        _file_path: &str,
        _content: &[u8],
        _message: &str,
    ) -> Result<String> {
        Ok("0000000000000000000000000000000000000000".to_string())
    }

    async fn reset_branch(&self, _repo_path: &str, _branch: &str, _target_sha: &str) -> Result<()> {
        Ok(())
    }

    async fn read_file(
        &self,
        _repo_path: &str,
        _branch: &str,
        _file_path: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn read_file_at_commit(
        &self,
        _repo_path: &str,
        _sha: &str,
        _file_path: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn revert_commit(
        &self,
        _repo_path: &str,
        _branch: &str,
        _sha_to_revert: &str,
    ) -> Result<gyre_domain::RevertResult> {
        Ok(gyre_domain::RevertResult::Success {
            revert_commit_sha: "0000000000000000000000000000000000000000".to_string(),
        })
    }
}

/// Configurable git operations adapter for tests that need to control
/// `can_merge` results per branch. Branches in the `conflict_branches` set
/// return `Ok(false)` from `can_merge`; all others return `Ok(true)`.
/// All other operations delegate to `NoopGitOps` behavior.
#[cfg(test)]
pub struct ConfigurableGitOps {
    pub conflict_branches: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// SHAs passed to `revert_commit`, in call order (task-095 R3-F1:
    /// tests must be able to observe WHICH commit was reverted).
    pub revert_calls: Arc<parking_lot::Mutex<Vec<String>>>,
    /// When true, `revert_commit` fails — the double must be able to fail,
    /// so error paths of the revert flow are testable (task-095 F8 lesson).
    pub revert_fails: bool,
    /// When true, `revert_commit` returns a Conflict — the R4-F1
    /// conflict path must be testable without a real git repo.
    pub revert_conflicts: bool,
    /// Shared flag consulted by `diff`: when set, `diff` reports one
    /// changed file (task-106 R19 F2 — lets a test observe, through the
    /// MR's diff_stats, whether the jj git export ran BEFORE the stats
    /// were computed; the flag is flipped by the jj double's export side
    /// effect at the exact call point).
    pub diff_change_when: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl Default for ConfigurableGitOps {
    fn default() -> Self {
        Self {
            conflict_branches: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            revert_calls: Arc::new(parking_lot::Mutex::new(Vec::new())),
            revert_fails: false,
            revert_conflicts: false,
            diff_change_when: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[cfg(test)]
impl ConfigurableGitOps {
    pub fn with_conflicts(branches: Vec<&str>) -> Self {
        let set: std::collections::HashSet<String> =
            branches.into_iter().map(|s| s.to_string()).collect();
        Self {
            conflict_branches: Arc::new(std::sync::Mutex::new(set)),
            ..Default::default()
        }
    }
}

#[cfg(test)]
#[async_trait]
impl GitOpsPort for ConfigurableGitOps {
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
        let changed = usize::from(
            self.diff_change_when
                .load(std::sync::atomic::Ordering::SeqCst),
        );
        Ok(DiffResult {
            files_changed: changed,
            insertions: changed,
            deletions: 0,
            patches: vec![],
        })
    }

    async fn is_repo(&self, _path: &str) -> Result<bool> {
        Ok(false)
    }

    async fn can_merge(&self, _repo_path: &str, source: &str, _target: &str) -> Result<bool> {
        let conflicts = self.conflict_branches.lock().unwrap();
        Ok(!conflicts.contains(source))
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

    async fn create_detached_worktree(
        &self,
        _repo_path: &str,
        _worktree_path: &str,
        _sha: &str,
    ) -> Result<()> {
        // Test double without a real repository; callers fall back to the
        // server-cwd path (task-095 R2-1).
        Err(anyhow::anyhow!(
            "create_detached_worktree not supported by ConfigurableGitOps"
        ))
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

    async fn branch_exists(&self, _repo_path: &str, _branch_name: &str) -> Result<bool> {
        Ok(false)
    }

    async fn create_branch(
        &self,
        _repo_path: &str,
        _branch_name: &str,
        _from_ref: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn write_file(
        &self,
        _repo_path: &str,
        _branch: &str,
        _file_path: &str,
        _content: &[u8],
        _message: &str,
    ) -> Result<String> {
        Ok("0000000000000000000000000000000000000000".to_string())
    }

    async fn reset_branch(&self, _repo_path: &str, _branch: &str, _target_sha: &str) -> Result<()> {
        Ok(())
    }

    async fn read_file(
        &self,
        _repo_path: &str,
        _branch: &str,
        _file_path: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn read_file_at_commit(
        &self,
        _repo_path: &str,
        _sha: &str,
        _file_path: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn revert_commit(
        &self,
        _repo_path: &str,
        _branch: &str,
        sha_to_revert: &str,
    ) -> Result<gyre_domain::RevertResult> {
        if self.revert_fails {
            return Err(anyhow::anyhow!(
                "revert_commit failed (ConfigurableGitOps revert_fails)"
            ));
        }
        self.revert_calls.lock().push(sha_to_revert.to_string());
        if self.revert_conflicts {
            return Ok(gyre_domain::RevertResult::Conflict {
                message: format!(
                    "revert of {sha_to_revert} conflicts with later changes (test double)"
                ),
            });
        }
        Ok(gyre_domain::RevertResult::Success {
            revert_commit_sha: format!("revert-of-{sha_to_revert}"),
        })
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

    async fn jj_bookmark_set(&self, _repo_path: &str, _name: &str, _change_id: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_undo(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_rebase(
        &self,
        _workspace_path: &str,
        _revision: &str,
        _destination: &str,
    ) -> Result<gyre_ports::JjRebaseOutcome> {
        Ok(gyre_ports::JjRebaseOutcome::Success { rebased_count: 0 })
    }

    async fn jj_main_checkout_init(
        &self,
        _main_checkout_path: &str,
        _git_repo_path: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn jj_workspace_add(
        &self,
        _main_checkout_path: &str,
        _workspace_path: &str,
        _name: &str,
        _revision: &str,
        _description: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn jj_workspace_forget(&self, _main_checkout_path: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_git_export(&self, _workspace_path: &str) -> Result<()> {
        Ok(())
    }
}

/// Configurable jj double for tests: records every call and replays a
/// programmed rebase outcome. `parking_lot` Mutex — never held across await.
#[cfg(test)]
#[derive(Default)]
pub struct ConfigurableJjOps {
    pub rebase_calls: parking_lot::Mutex<Vec<(String, String, String)>>,
    /// Programmed outcome replayed by `jj_rebase`. The failure variant is
    /// stored as a message (`anyhow::Error` is not `Clone`) and turned
    /// into an error at replay time.
    pub rebase_outcome: parking_lot::Mutex<Option<Result<gyre_ports::JjRebaseOutcome, String>>>,
    pub workspace_forget_calls: parking_lot::Mutex<Vec<String>>,
    pub workspace_add_calls: parking_lot::Mutex<Vec<String>>,
    /// The `-r <revision>` argument per `jj_workspace_add` call (F9: the
    /// spawn branch handed to jj provisioning).
    pub workspace_add_revisions: parking_lot::Mutex<Vec<String>>,
    /// `"{main_checkout_path}:{git_repo_path}"` per call.
    pub main_checkout_init_calls: parking_lot::Mutex<Vec<String>>,
    /// When true, `jj_main_checkout_init` fails — lets tests exercise
    /// the spawn flow's plain-git worktree fallback.
    pub main_checkout_init_fails: parking_lot::Mutex<bool>,
    pub git_export_calls: parking_lot::Mutex<Vec<String>>,
    /// `jj_bookmark_set` calls as `(repo_path, name, change_id)` in order
    /// (task-106 R19 F1: the completion path must SET the imported
    /// bookmark, not create).
    pub bookmark_set_calls: parking_lot::Mutex<Vec<(String, String, String)>>,
    /// `jj_bookmark_create` calls as `(repo_path, name, change_id)` in
    /// order — recorded so tests can pin that the completion path never
    /// regresses to create (task-106 R19 F1).
    pub bookmark_create_calls: parking_lot::Mutex<Vec<(String, String, String)>>,
    /// Synchronous side effect run inside `jj_git_export` before it
    /// returns (task-106 R19 F2: lets tests make the export's effect on
    /// git-visible state observable at the exact call point, so
    /// export-before-stats ordering is provable with doubles).
    pub git_export_effect: parking_lot::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[cfg(test)]
#[async_trait]
impl JjOpsPort for ConfigurableJjOps {
    async fn jj_init(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_new(&self, _repo_path: &str, _description: &str) -> Result<String> {
        Ok("cfg-change-id".to_string())
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

    async fn jj_bookmark_create(&self, repo_path: &str, name: &str, change_id: &str) -> Result<()> {
        self.bookmark_create_calls.lock().push((
            repo_path.to_string(),
            name.to_string(),
            change_id.to_string(),
        ));
        Ok(())
    }

    async fn jj_bookmark_set(&self, repo_path: &str, name: &str, change_id: &str) -> Result<()> {
        self.bookmark_set_calls.lock().push((
            repo_path.to_string(),
            name.to_string(),
            change_id.to_string(),
        ));
        Ok(())
    }

    async fn jj_undo(&self, _repo_path: &str) -> Result<()> {
        Ok(())
    }

    async fn jj_rebase(
        &self,
        workspace_path: &str,
        revision: &str,
        destination: &str,
    ) -> Result<gyre_ports::JjRebaseOutcome> {
        self.rebase_calls.lock().push((
            workspace_path.to_string(),
            revision.to_string(),
            destination.to_string(),
        ));
        match self.rebase_outcome.lock().clone() {
            Some(Ok(outcome)) => Ok(outcome),
            Some(Err(msg)) => Err(anyhow::anyhow!(msg)),
            None => Ok(gyre_ports::JjRebaseOutcome::Success { rebased_count: 0 }),
        }
    }

    async fn jj_main_checkout_init(
        &self,
        main_checkout_path: &str,
        git_repo_path: &str,
    ) -> Result<()> {
        self.main_checkout_init_calls
            .lock()
            .push(format!("{main_checkout_path}:{git_repo_path}"));
        if *self.main_checkout_init_fails.lock() {
            return Err(anyhow::anyhow!("programmed jj main checkout init failure"));
        }
        Ok(())
    }

    async fn jj_workspace_add(
        &self,
        _main_checkout_path: &str,
        workspace_path: &str,
        name: &str,
        _revision: &str,
        _description: &str,
    ) -> Result<()> {
        self.workspace_add_calls
            .lock()
            .push(format!("{name}:{workspace_path}"));
        self.workspace_add_revisions
            .lock()
            .push(_revision.to_string());
        Ok(())
    }

    async fn jj_workspace_forget(&self, _main_checkout_path: &str, name: &str) -> Result<()> {
        self.workspace_forget_calls.lock().push(name.to_string());
        Ok(())
    }

    async fn jj_git_export(&self, workspace_path: &str) -> Result<()> {
        if let Some(effect) = self.git_export_effect.lock().as_ref() {
            effect();
        }
        self.git_export_calls
            .lock()
            .push(workspace_path.to_string());
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
        let mut store = self.store.lock().await;
        // Port contract: (workspace_id, name) is unique — the pair maps to
        // one bare-repo path, so a duplicate row would alias another repo's
        // storage. Guarded in code because the mem adapter has no schema to
        // enforce SQLite's idx_repos_workspace_name unique index.
        if let Some(existing) = store
            .values()
            .find(|r| r.workspace_id == repo.workspace_id && r.name == repo.name)
        {
            if existing.id != repo.id {
                anyhow::bail!(
                    "repo '{}' already exists in workspace {}",
                    repo.name,
                    repo.workspace_id
                );
            }
        }
        store.insert(repo.id.to_string(), repo.clone());
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
                    .any(|dep| dep.target_mr_id.as_str() == mr_id.as_str())
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
        // Port contract: fail if id, external_id, or username already exists.
        let mut store = self.store.lock().await;
        if store.values().any(|u| {
            u.id == user.id || u.external_id == user.external_id || u.username == user.username
        }) {
            anyhow::bail!(
                "user already exists for username {}, external_id {}",
                user.username,
                user.external_id
            );
        }
        store.insert(user.id.to_string(), user.clone());
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

    async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|u| u.username == username)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<User>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn update(&self, user: &User) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: username and external_id are immutable after creation.
        if let Some(existing) = store.get(user.id.as_str()) {
            if existing.username != user.username {
                anyhow::bail!(
                    "username is immutable: cannot change {} to {}",
                    existing.username,
                    user.username
                );
            }
            if existing.external_id != user.external_id {
                anyhow::bail!(
                    "external_id is immutable: cannot change {} to {}",
                    existing.external_id,
                    user.external_id
                );
            }
        } else {
            anyhow::bail!("cannot update user {}: not found", user.id);
        }
        store.insert(user.id.to_string(), user.clone());
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

/// Shared filter predicate for `MemAnalyticsRepository` — the exact
/// semantics of `AnalyticsRepository::query_filtered` (exact or
/// trailing-`*` event_name, scope equality, inclusive since/until).
/// `count_grouped` reuses it so the grouped path can never drift from
/// the filtered path.
fn analytics_event_matches_filter(e: &AnalyticsEvent, filter: &AnalyticsQueryFilter) -> bool {
    let name_ok = match &filter.event_name {
        None => true,
        Some(pat) => match pat.strip_suffix('*') {
            Some(prefix) => e.event_name.starts_with(prefix),
            None => e.event_name == *pat,
        },
    };
    name_ok
        && filter
            .agent_id
            .as_ref()
            .is_none_or(|id| e.agent_id.as_ref() == Some(id))
        && filter
            .user_id
            .as_ref()
            .is_none_or(|id| e.user_id.as_ref() == Some(id))
        && filter
            .workspace_id
            .as_ref()
            .is_none_or(|id| e.workspace_id.as_ref() == Some(id))
        && filter
            .repo_id
            .as_ref()
            .is_none_or(|id| e.repo_id.as_ref() == Some(id))
        && filter.since.is_none_or(|s| e.timestamp >= s)
        && filter.until.is_none_or(|u| e.timestamp <= u)
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
        events.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        events.truncate(limit);
        Ok(events)
    }

    async fn query_filtered(&self, filter: &AnalyticsQueryFilter) -> Result<Vec<AnalyticsEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AnalyticsEvent> = store
            .iter()
            .filter(|e| analytics_event_matches_filter(e, filter))
            .cloned()
            .collect();
        events.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        events.truncate(filter.limit);
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

    async fn count_grouped(
        &self,
        filter: &AnalyticsQueryFilter,
        group_by: gyre_ports::AnalyticsGroupBy,
    ) -> Result<Vec<(String, u64)>> {
        use std::collections::BTreeMap;
        // Aggregate the FULL filtered set (task-146 review F1): the
        // event-level limit never truncates the input; `filter.limit`
        // bounds the number of returned groups, mirroring the SQL
        // adapters' `GROUP BY ... LIMIT`.
        let store = self.store.lock().await;
        let mut groups: BTreeMap<String, u64> = BTreeMap::new();
        for e in store.iter() {
            if !analytics_event_matches_filter(e, filter) {
                continue;
            }
            let key = match group_by {
                gyre_ports::AnalyticsGroupBy::EventName => e.event_name.clone(),
                gyre_ports::AnalyticsGroupBy::AgentId => {
                    e.agent_id.clone().unwrap_or_else(|| "(none)".into())
                }
                gyre_ports::AnalyticsGroupBy::WorkspaceId => {
                    e.workspace_id.clone().unwrap_or_else(|| "(none)".into())
                }
                gyre_ports::AnalyticsGroupBy::Day => {
                    epoch_days_to_date((e.timestamp / 86_400) as i64)
                }
            };
            *groups.entry(key).or_insert(0) += 1;
        }
        Ok(groups.into_iter().take(filter.limit).collect())
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

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        let mut store = self.store.lock().await;
        let before = store.len();
        store.retain(|e| e.timestamp >= cutoff_secs);
        Ok((before - store.len()) as u64)
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

#[cfg(test)]
mod analytics_query_tests {
    use super::*;
    use gyre_ports::AnalyticsGroupBy;
    use gyre_ports::AnalyticsRepository;

    fn scoped_event(
        id: &str,
        name: &str,
        agent_id: Option<&str>,
        user_id: Option<&str>,
        workspace_id: Option<&str>,
        repo_id: Option<&str>,
        ts: u64,
    ) -> AnalyticsEvent {
        AnalyticsEvent {
            id: Id::new(id),
            event_name: name.to_string(),
            agent_id: agent_id.map(str::to_string),
            user_id: user_id.map(str::to_string),
            session_id: None,
            workspace_id: workspace_id.map(str::to_string),
            repo_id: repo_id.map(str::to_string),
            properties: serde_json::json!({}),
            timestamp: ts,
        }
    }

    #[tokio::test]
    async fn query_filtered_matches_sqlite_filter_semantics() {
        let repo = MemAnalyticsRepository::default();
        repo.record(&scoped_event(
            "e1",
            "mr.merged",
            Some("a1"),
            None,
            Some("ws-1"),
            Some("r1"),
            100,
        ))
        .await
        .unwrap();
        repo.record(&scoped_event(
            "e2",
            "mr.merged",
            Some("a2"),
            Some("u9"),
            Some("ws-2"),
            Some("r1"),
            200,
        ))
        .await
        .unwrap();
        repo.record(&scoped_event(
            "e3",
            "task.status_changed",
            Some("a1"),
            None,
            Some("ws-1"),
            None,
            300,
        ))
        .await
        .unwrap();

        // Prefix wildcard.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert_eq!(out.len(), 2, "mr.* prefix match");

        // Literal, case-sensitive prefix semantics (task-146 review R4,
        // same contract as the SQLite/Postgres adapters): `MR.*` and a
        // wildcard-bearing `mr.m_rged*` must not match `mr.merged`.
        let f = AnalyticsQueryFilter {
            event_name: Some("MR.*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert!(
            out.is_empty(),
            "case-variant prefix must not match: {out:?}"
        );
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m_rged*".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert!(
            out.is_empty(),
            "wildcard-bearing prefix must not match: {out:?}"
        );

        // Scope filters are conjunctive.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.merged".into()),
            repo_id: Some("r1".into()),
            workspace_id: Some("ws-2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, Id::new("e2"));

        // user_id filter.
        let f = AnalyticsQueryFilter {
            user_id: Some("u9".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, Id::new("e2"));

        // since/until inclusive bounds.
        let f = AnalyticsQueryFilter {
            since: Some(200),
            until: Some(200),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, Id::new("e2"));

        // limit truncates (newest first ordering).
        let f = AnalyticsQueryFilter {
            limit: 2,
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.query_filtered(&f).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, Id::new("e3"), "newest event first");
    }

    #[tokio::test]
    async fn count_grouped_over_full_filtered_set() {
        // task-146 review F1 regression (mem contract): grouped counts
        // aggregate the FULL filtered set — the event-level limit bounds
        // the number of returned groups, never the input to the
        // aggregation. Mirrors the SQLite adapter test.
        let repo = MemAnalyticsRepository::default();
        for i in 0..150u64 {
            repo.record(&scoped_event(
                &format!("m{i}"),
                "mr.merged",
                Some("a1"),
                None,
                Some("ws-1"),
                None,
                1000 + i,
            ))
            .await
            .unwrap();
        }
        repo.record(&scoped_event(
            "c0",
            "mr.closed",
            Some("a2"),
            None,
            Some("ws-2"),
            None,
            2000,
        ))
        .await
        .unwrap();
        repo.record(&scoped_event(
            "c1",
            "mr.closed",
            None,
            None,
            Some("ws-2"),
            None,
            2001,
        ))
        .await
        .unwrap();

        let f = AnalyticsQueryFilter {
            limit: 100,
            ..AnalyticsQueryFilter::new()
        };
        let out = repo
            .count_grouped(&f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![("mr.closed".to_string(), 2), ("mr.merged".to_string(), 150)],
            "grouped totals must not be truncated by the event-level limit"
        );

        // Consistent with count() on the same question (i64::MAX, not
        // u64::MAX: the SQL adapters bind `until` as i64).
        assert_eq!(
            repo.count("mr.merged", 0, i64::MAX as u64).await.unwrap(),
            150
        );

        // limit bounds the number of returned groups.
        let f = AnalyticsQueryFilter {
            limit: 1,
            ..AnalyticsQueryFilter::new()
        };
        let out = repo
            .count_grouped(&f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(out, vec![("mr.closed".to_string(), 2)]);

        // NULL agent_id groups under "(none)".
        let out = repo
            .count_grouped(&AnalyticsQueryFilter::new(), AnalyticsGroupBy::AgentId)
            .await
            .unwrap();
        assert_eq!(
            out,
            vec![
                ("(none)".to_string(), 1),
                ("a1".to_string(), 150),
                ("a2".to_string(), 1)
            ]
        );

        // Same filter semantics as query_filtered: prefix + workspace.
        let f = AnalyticsQueryFilter {
            event_name: Some("mr.m*".into()),
            workspace_id: Some("ws-1".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo
            .count_grouped(&f, AnalyticsGroupBy::EventName)
            .await
            .unwrap();
        assert_eq!(out, vec![("mr.merged".to_string(), 150)]);

        // Day grouping: Jan 1 / Jan 2 2024 UTC buckets.
        let day1: u64 = 1_704_067_200;
        repo.record(&scoped_event("d1", "ev2", None, None, None, None, day1))
            .await
            .unwrap();
        repo.record(&scoped_event(
            "d2",
            "ev2",
            None,
            None,
            None,
            None,
            day1 + 3600,
        ))
        .await
        .unwrap();
        repo.record(&scoped_event(
            "d3",
            "ev2",
            None,
            None,
            None,
            None,
            day1 + 86_400,
        ))
        .await
        .unwrap();
        let f = AnalyticsQueryFilter {
            event_name: Some("ev2".into()),
            ..AnalyticsQueryFilter::new()
        };
        let out = repo.count_grouped(&f, AnalyticsGroupBy::Day).await.unwrap();
        assert_eq!(
            out,
            vec![("2024-01-01".to_string(), 2), ("2024-01-02".to_string(), 1)]
        );
    }
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
        entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        Ok(entries)
    }

    async fn query_by_task(&self, task_id: &Id) -> Result<Vec<CostEntry>> {
        let store = self.store.lock().await;
        let mut entries: Vec<CostEntry> = store
            .iter()
            .filter(|e| e.task_id.as_ref().map(|id| id.as_str()) == Some(task_id.as_str()))
            .cloned()
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
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

/// One stored audit event. Once the log-compression job compresses an
/// event's detail (observability.md §Log Retention & Compression), the
/// raw JSON lives only in `detail_zstd` and `event.detail` is `Null` —
/// mirroring the SQLite/Postgres adapters, where compression blanks the
/// `detail` column and stores the zstd payload in `detail_zstd`.
struct MemAuditEntry {
    event: AuditEvent,
    detail_zstd: Option<Vec<u8>>,
}

impl MemAuditEntry {
    /// Materialize the domain event, decompressing the detail payload
    /// when compressed. A corrupt compressed payload is a hard error —
    /// same contract as the DB adapters.
    fn materialize(&self) -> Result<AuditEvent> {
        let mut event = self.event.clone();
        if let Some(z) = &self.detail_zstd {
            let raw = zstd::decode_all(z.as_slice())
                .map_err(|e| anyhow::anyhow!("zstd decompress: {e}"))?;
            event.detail = serde_json::from_slice(&raw)
                .map_err(|e| anyhow::anyhow!("parse compressed audit detail: {e}"))?;
        }
        Ok(event)
    }
}

#[derive(Default)]
pub struct MemAuditRepository {
    store: Arc<Mutex<Vec<MemAuditEntry>>>,
}

#[async_trait]
impl AuditRepository for MemAuditRepository {
    async fn record(&self, event: &AuditEvent) -> Result<()> {
        self.store.lock().await.push(MemAuditEntry {
            event: event.clone(),
            detail_zstd: None,
        });
        Ok(())
    }

    async fn query(&self, filter: &AuditQueryFilter) -> Result<Vec<AuditEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AuditEvent> =
            store
                .iter()
                .filter(|e| {
                    filter.agent_id.as_ref().is_none_or(|a| {
                        e.event.agent_id.as_ref().is_some_and(|id| id.as_str() == a)
                    }) && filter
                        .event_type
                        .as_ref()
                        .is_none_or(|t| e.event.event_type.as_str() == *t)
                        && filter.workspace_id.as_ref().is_none_or(|w| {
                            e.event
                                .workspace_id
                                .as_ref()
                                .is_some_and(|id| id.as_str() == w)
                        })
                        && filter.user_id.as_ref().is_none_or(|u| {
                            e.event.user_id.as_ref().is_some_and(|id| id.as_str() == u)
                        })
                        && filter
                            .resource_type
                            .as_ref()
                            .is_none_or(|rt| e.event.resource_type == *rt)
                        && filter.outcome.is_none_or(|o| e.event.outcome == o)
                        && filter.since.is_none_or(|s| e.event.timestamp >= s)
                        && filter.until.is_none_or(|u| e.event.timestamp <= u)
                })
                .map(MemAuditEntry::materialize)
                .collect::<Result<Vec<_>>>()?;
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        events.truncate(filter.limit);
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
            *counts.entry(e.event.event_type.as_str()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(result)
    }

    async fn since_timestamp(&self, since: u64, limit: usize) -> Result<Vec<AuditEvent>> {
        let store = self.store.lock().await;
        let mut events: Vec<AuditEvent> = store
            .iter()
            .filter(|e| e.event.timestamp > since)
            .map(MemAuditEntry::materialize)
            .collect::<Result<Vec<_>>>()?;
        events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        events.truncate(limit);
        Ok(events)
    }

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        let mut store = self.store.lock().await;
        let before = store.len();
        store.retain(|e| e.event.timestamp >= cutoff_secs);
        Ok((before - store.len()) as u64)
    }

    async fn compress_details_before(
        &self,
        cutoff_secs: u64,
        limit: usize,
    ) -> Result<gyre_ports::audit::CompressionStats> {
        let mut store = self.store.lock().await;
        let mut stats = gyre_ports::audit::CompressionStats::default();
        for e in store.iter_mut() {
            if (stats.compressed as usize) >= limit {
                break;
            }
            if e.detail_zstd.is_some() || e.event.timestamp >= cutoff_secs {
                continue;
            }
            let raw = serde_json::to_vec(&e.event.detail)?;
            let compressed = zstd::encode_all(raw.as_slice(), 3)?;
            stats.compressed += 1;
            stats.bytes_before += raw.len() as u64;
            stats.bytes_after += compressed.len() as u64;
            e.detail_zstd = Some(compressed);
            e.event.detail = serde_json::Value::Null;
        }
        Ok(stats)
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

/// In-memory breaking change repository.
#[derive(Default)]
pub struct MemBreakingChangeRepository {
    store: Arc<Mutex<Vec<gyre_domain::BreakingChange>>>,
}

#[async_trait]
impl gyre_ports::BreakingChangeRepository for MemBreakingChangeRepository {
    async fn create(&self, bc: &gyre_domain::BreakingChange) -> Result<()> {
        self.store.lock().await.push(bc.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<gyre_domain::BreakingChange>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|b| b.id.as_str() == id.as_str())
            .cloned())
    }

    async fn list_unacknowledged(&self) -> Result<Vec<gyre_domain::BreakingChange>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|b| !b.acknowledged)
            .cloned()
            .collect())
    }

    async fn list_by_source_repo(
        &self,
        source_repo_id: &Id,
    ) -> Result<Vec<gyre_domain::BreakingChange>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|b| b.source_repo_id.as_str() == source_repo_id.as_str())
            .cloned()
            .collect())
    }

    async fn acknowledge(&self, id: &Id, acknowledged_by: &str, at: u64) -> Result<bool> {
        let mut store = self.store.lock().await;
        if let Some(bc) = store.iter_mut().find(|b| b.id.as_str() == id.as_str()) {
            bc.acknowledged = true;
            bc.acknowledged_by = Some(acknowledged_by.to_string());
            bc.acknowledged_at = Some(at);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// In-memory dependency policy repository.
#[derive(Default)]
pub struct MemDependencyPolicyRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::DependencyPolicy>>>,
}

#[async_trait]
impl gyre_ports::DependencyPolicyRepository for MemDependencyPolicyRepository {
    async fn get_for_workspace(&self, workspace_id: &Id) -> Result<gyre_domain::DependencyPolicy> {
        Ok(self
            .store
            .lock()
            .await
            .get(workspace_id.as_str())
            .cloned()
            .unwrap_or_default())
    }

    async fn set_for_workspace(
        &self,
        workspace_id: &Id,
        policy: &gyre_domain::DependencyPolicy,
    ) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(workspace_id.as_str().to_string(), policy.clone());
        Ok(())
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
        let mut store = self.store.lock().await;
        // Port contract: slug is unique. The mem adapter has no schema, so
        // the SQLite `tenants.slug UNIQUE` constraint is guarded in code —
        // without it, a bootstrap re-run against the default in-memory dev
        // server silently created a second tenant with the same slug and
        // the CLI's reuse-by-slug fallback never triggered (round-3
        // finding 4b).
        if let Some(existing) = store.values().find(|t| t.slug == tenant.slug) {
            if existing.id != tenant.id {
                anyhow::bail!(
                    "tenant with slug '{}' already exists ({})",
                    tenant.slug,
                    existing.id
                );
            }
        }
        store.insert(tenant.id.to_string(), tenant.clone());
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
    /// Policy store shared with the paired `MemPolicyRepository` so that
    /// `apply_trust_transition` writes are visible to the ABAC engine (which
    /// reads through `state.policies`). Defaults to an independent empty store.
    policies: Arc<Mutex<HashMap<String, gyre_domain::Policy>>>,
    /// Test hook: when set, `apply_trust_transition` returns an error to
    /// simulate a DB transaction rollback (exercises the handler's 409 path).
    fail_trust_transition: Arc<std::sync::atomic::AtomicBool>,
}

impl MemWorkspaceRepository {
    /// Construct a workspace repo that shares its policy store with a paired
    /// `MemPolicyRepository`, giving `apply_trust_transition` real policy-write
    /// behavior in in-memory mode.
    pub fn with_policy_store(policies: Arc<Mutex<HashMap<String, gyre_domain::Policy>>>) -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
            policies,
            fail_trust_transition: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Set the workspace store (builder over `with_policy_store`): lets a
    /// paired `MemComputeTargetRepository` share this repo's workspace map.
    pub fn with_workspace_store(
        mut self,
        workspaces: Arc<Mutex<HashMap<String, Workspace>>>,
    ) -> Self {
        self.store = workspaces;
        self
    }

    /// Test hook: cause subsequent `apply_trust_transition` calls to fail.
    /// Only used by `#[cfg(test)]` wiring (`shared_workspace_policy_pair`),
    /// so gated to test builds to avoid dead-code warnings in the lib.
    #[cfg(test)]
    pub fn fail_trust_transitions(&self) {
        self.fail_trust_transition
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl WorkspaceRepository for MemWorkspaceRepository {
    async fn create(&self, workspace: &Workspace) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: (tenant_id, slug) is unique — /git/:ws_slug/:repo
        // URL resolution must stay unambiguous. Guarded in code because the
        // mem adapter has no schema to enforce SQLite's UNIQUE constraint.
        if let Some(existing) = store
            .values()
            .find(|ws| ws.tenant_id == workspace.tenant_id && ws.slug == workspace.slug)
        {
            if existing.id != workspace.id {
                anyhow::bail!(
                    "workspace with slug '{}' already exists in tenant {}",
                    workspace.slug,
                    workspace.tenant_id
                );
            }
        }
        store.insert(workspace.id.to_string(), workspace.clone());
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

    async fn apply_trust_transition(
        &self,
        workspace: &Workspace,
        delete_trust_policies: bool,
        new_policies: &[gyre_domain::Policy],
    ) -> Result<()> {
        if self
            .fail_trust_transition
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            anyhow::bail!("simulated trust transition failure");
        }
        // In-memory writes cannot partially fail, so this is inherently atomic.
        //
        // Port contract (same as `create`): (tenant_id, slug) is unique.
        // This is the path POST /api/v1/workspaces takes, so the guard here
        // is what makes a bootstrap re-run's create fail and fall back to
        // reuse-by-name on the default in-memory dev server.
        {
            let store = self.store.lock().await;
            if let Some(existing) = store
                .values()
                .find(|ws| ws.tenant_id == workspace.tenant_id && ws.slug == workspace.slug)
            {
                if existing.id != workspace.id {
                    anyhow::bail!(
                        "workspace with slug '{}' already exists in tenant {}",
                        workspace.slug,
                        workspace.tenant_id
                    );
                }
            }
        }
        self.store
            .lock()
            .await
            .insert(workspace.id.to_string(), workspace.clone());
        let mut policies = self.policies.lock().await;
        if delete_trust_policies {
            policies.retain(|_, p| {
                !(p.name.starts_with("trust:")
                    && p.scope_id.as_ref().map(|i| i.as_str()) == Some(workspace.id.as_str()))
            });
        }
        for p in new_policies {
            policies.insert(p.id.to_string(), p.clone());
        }
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
    /// Test hook: when set, `create` returns an error to simulate a policy-store
    /// write failure (exercises fail-closed seeding, task-077 F6 class).
    fail_creates: Arc<std::sync::atomic::AtomicBool>,
    /// Test hook: when set, `list` returns an error to simulate a policy-store
    /// read failure (exercises the fail-closed supervised-trust merge gate,
    /// task-077 F9 class).
    fail_lists: Arc<std::sync::atomic::AtomicBool>,
}

impl MemPolicyRepository {
    /// Construct a policy repo backed by an externally-owned policy store,
    /// so a paired `MemWorkspaceRepository` can share the same map for atomic
    /// trust-transition writes.
    pub fn from_store(policies: Arc<Mutex<HashMap<String, gyre_domain::Policy>>>) -> Self {
        Self {
            policies,
            decisions: Arc::new(Mutex::new(Vec::new())),
            fail_creates: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fail_lists: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Test hook: cause subsequent `create` calls to fail.
    #[cfg(test)]
    pub fn fail_creates(&self) {
        self.fail_creates
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Test hook: cause subsequent `list` calls to fail.
    #[cfg(test)]
    pub fn fail_lists(&self) {
        self.fail_lists
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl gyre_ports::PolicyRepository for MemPolicyRepository {
    async fn create(&self, policy: &gyre_domain::Policy) -> Result<()> {
        if self.fail_creates.load(std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!("simulated policy store failure");
        }
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
        if self.fail_lists.load(std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!("simulated policy store list failure");
        }
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
            .filter(|p| &p.scope == scope && p.scope_id.as_ref().map(|i| i.as_str()) == scope_id)
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
            .filter(|p| {
                p.name.starts_with(prefix)
                    && p.scope_id.as_ref().map(|i| i.as_str()) == Some(scope_id)
            })
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
// Repo access overrides (user-management.md §Repo Access)
// ──────────────────────────────────────────────────────────────────────────────

use gyre_domain::RepoAccessOverride;
use gyre_ports::RepoAccessOverrideRepository;

#[derive(Default)]
pub struct MemRepoAccessOverrideRepository {
    store: Arc<Mutex<HashMap<String, RepoAccessOverride>>>,
}

#[async_trait]
impl RepoAccessOverrideRepository for MemRepoAccessOverrideRepository {
    async fn create(&self, o: &RepoAccessOverride) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: create fails when an override for the same
        // (repo_id, user_id) pair already exists. The mem adapter has no
        // schema, so the guard lives in code (check-mem-port-contracts).
        if store
            .values()
            .any(|e| e.repo_id == o.repo_id && e.user_id == o.user_id)
        {
            return Err(anyhow::anyhow!(
                "repo access override for user {} on repo {} already exists",
                o.user_id,
                o.repo_id
            ));
        }
        store.insert(o.id.to_string(), o.clone());
        Ok(())
    }

    async fn find_by_repo_and_user(
        &self,
        repo_id: &Id,
        user_id: &Id,
    ) -> Result<Option<RepoAccessOverride>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|e| e.repo_id == *repo_id && e.user_id == *user_id)
            .cloned())
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<RepoAccessOverride>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .filter(|e| e.repo_id == *repo_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, repo_id: &Id, user_id: &Id) -> Result<bool> {
        let mut store = self.store.lock().await;
        let key = store
            .values()
            .find(|e| e.repo_id == *repo_id && e.user_id == *user_id)
            .map(|e| e.id.to_string());
        match key {
            Some(k) => {
                store.remove(&k);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Team
// ──────────────────────────────────────────────────────────────────────────────

use gyre_domain::{Team, TeamMembership};
use gyre_ports::TeamRepository;

#[derive(Default)]
pub struct MemTeamRepository {
    store: Arc<Mutex<HashMap<String, Team>>>,
    members: Arc<Mutex<HashMap<(String, String), TeamMembership>>>,
}

#[async_trait]
impl TeamRepository for MemTeamRepository {
    async fn create(&self, team: &Team) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: the slug is unique within the workspace.
        if store
            .values()
            .any(|t| t.workspace_id == team.workspace_id && t.slug == team.slug)
        {
            anyhow::bail!("team slug already exists in workspace");
        }
        store.insert(team.id.to_string(), team.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Team>> {
        Ok(self.store.lock().await.get(id.as_str()).cloned())
    }

    async fn find_by_slug(&self, workspace_id: &Id, slug: &str) -> Result<Option<Team>> {
        Ok(self
            .store
            .lock()
            .await
            .values()
            .find(|t| t.workspace_id == *workspace_id && t.slug == slug)
            .cloned())
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Team>> {
        let mut teams: Vec<Team> = self
            .store
            .lock()
            .await
            .values()
            .filter(|t| t.workspace_id == *workspace_id)
            .cloned()
            .collect();
        teams.sort_by_key(|t| t.created_at);
        Ok(teams)
    }

    async fn update(&self, team: &Team) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: the new slug must not collide with another team's
        // slug in the same workspace.
        if store
            .values()
            .any(|t| t.id != team.id && t.workspace_id == team.workspace_id && t.slug == team.slug)
        {
            anyhow::bail!("team slug already exists in workspace");
        }
        store.insert(team.id.to_string(), team.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.store.lock().await.remove(id.as_str());
        // Memberships go with the team.
        self.members
            .lock()
            .await
            .retain(|(team_id, _), _| team_id != id.as_str());
        Ok(())
    }

    async fn add_member(&self, membership: &TeamMembership) -> Result<()> {
        let mut members = self.members.lock().await;
        let key = (
            membership.team_id.to_string(),
            membership.user_id.to_string(),
        );
        // Port contract: duplicate membership fails.
        if members.contains_key(&key) {
            anyhow::bail!("user is already a member of the team");
        }
        members.insert(key, membership.clone());
        Ok(())
    }

    async fn remove_member(&self, team_id: &Id, user_id: &Id) -> Result<()> {
        self.members
            .lock()
            .await
            .remove(&(team_id.to_string(), user_id.to_string()));
        Ok(())
    }

    async fn list_members(&self, team_id: &Id) -> Result<Vec<TeamMembership>> {
        let mut memberships: Vec<TeamMembership> = self
            .members
            .lock()
            .await
            .values()
            .filter(|m| m.team_id == *team_id)
            .cloned()
            .collect();
        memberships.sort_by_key(|m| m.added_at);
        Ok(memberships)
    }

    async fn list_for_user(&self, workspace_id: &Id, user_id: &Id) -> Result<Vec<Team>> {
        let store = self.store.lock().await;
        let members = self.members.lock().await;
        let mut teams: Vec<Team> = members
            .values()
            .filter(|m| m.user_id == *user_id)
            .filter_map(|m| store.get(m.team_id.as_str()))
            .filter(|t| t.workspace_id == *workspace_id)
            .cloned()
            .collect();
        teams.sort_by_key(|t| t.created_at);
        Ok(teams)
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
        notification_types: Option<&[&str]>,
        repo_id: Option<&Id>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>> {
        let store = self.store.lock().await;
        let mut items: Vec<Notification> = store
            .iter()
            .filter(|n| {
                n.user_id == *user_id
                    && workspace_id.is_none_or(|ws| n.workspace_id == *ws)
                    && min_priority.is_none_or(|min| n.priority_rank >= min)
                    && max_priority.is_none_or(|max| n.priority_rank <= max)
                    // Exact-match set on the type (port contract): an empty
                    // set matches nothing — `contains` is false for every
                    // item — never a no-op.
                    && notification_types.is_none_or(|ts| ts.contains(&n.notification_type.as_str()))
                    && repo_id.is_none_or(|r| n.source.repo_id.as_ref() == Some(r))
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            a.priority_rank
                .cmp(&b.priority_rank)
                .then(b.created_at.cmp(&a.created_at))
        });
        Ok(items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    async fn update_body_by_entity_ref(
        &self,
        entity_ref: &str,
        notification_type: &str,
        body: &str,
    ) -> Result<u64> {
        let mut updated = 0u64;
        for n in self.store.lock().await.iter_mut() {
            // Same match + unresolved guard as the SQL adapters: resolved or
            // dismissed cards keep their historical body (audit record of what
            // was acted on), so the guard is enforced in code here too.
            if n.source.entity_id == entity_ref
                && n.notification_type.as_str() == notification_type
                && n.resolved_at.is_none()
                && n.dismissed_at.is_none()
            {
                n.body = Some(body.to_string());
                updated += 1;
            }
        }
        Ok(updated)
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

    async fn mark_read(&self, id: &Id, user_id: &Id, read: bool) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        for n in self.store.lock().await.iter_mut() {
            if n.id == *id && n.user_id == *user_id {
                n.read = read;
                n.read_at = if read { Some(now) } else { None };
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

    async fn count_unread(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|n| {
                n.user_id == *user_id
                    && workspace_id.is_none_or(|ws| n.workspace_id == *ws)
                    && !n.read
                    && n.resolved_at.is_none()
                    && n.dismissed_at.is_none()
            })
            .count() as u64)
    }

    async fn mark_all_read(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut updated = 0u64;
        for n in self.store.lock().await.iter_mut() {
            if n.user_id == *user_id
                && !n.read
                && workspace_id.is_none_or(|ws| n.workspace_id == *ws)
            {
                n.read = true;
                n.read_at = Some(now);
                updated += 1;
            }
        }
        Ok(updated)
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

    async fn has_active_notification(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        entity_ref: &str,
        since: i64,
    ) -> Result<bool> {
        Ok(self.store.lock().await.iter().any(|n| {
            n.workspace_id == *workspace_id
                && n.user_id == *user_id
                && n.notification_type.as_str() == notification_type
                && n.source.entity_id == entity_ref
                && n.resolved_at.is_none()
                && n.dismissed_at.is_none()
                && n.created_at >= since
        }))
    }

    async fn delete_older_than(
        &self,
        read_cutoff_secs: u64,
        unread_cutoff_secs: u64,
    ) -> Result<u64> {
        let mut store = self.store.lock().await;
        let before = store.len();
        // "Read" = the human resolved it, dismissed it, or marked it read in
        // the UI (business-continuity.md §5 + user-management.md read state).
        store.retain(|n| {
            let read = n.read || n.resolved_at.is_some() || n.dismissed_at.is_some();
            let cutoff = if read {
                read_cutoff_secs
            } else {
                unread_cutoff_secs
            };
            (n.created_at as u64) >= cutoff
        });
        Ok((before - store.len()) as u64)
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
        let mut gates: Vec<gyre_domain::QualityGate> = self
            .gates
            .lock()
            .await
            .values()
            .filter(|g| g.repo_id.to_string() == repo_id)
            .cloned()
            .collect();
        // Same ordering contract as the SQL adapters: (position, created_at).
        gates.sort_by(|a, b| {
            a.position
                .cmp(&b.position)
                .then(a.created_at.cmp(&b.created_at))
        });
        Ok(gates)
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
                compute_minutes: 0,
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
                compute_minutes: 0,
            });
        usage.tokens_used_today = usage.tokens_used_today.saturating_add(tokens);
        usage.cost_today += cost_usd;
        Ok(())
    }

    async fn add_compute_minutes(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
        minutes: u64,
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
                compute_minutes: 0,
            });
        usage.compute_minutes = usage.compute_minutes.saturating_add(minutes);
        Ok(())
    }
}

// ── MemBudgetCallRepository ───────────────────────────────────────────────────

/// In-memory BudgetCallRepository for tests and development.
#[derive(Default)]
pub struct MemBudgetCallRepository {
    store: Mutex<Vec<gyre_domain::BudgetCallRecord>>,
}

#[async_trait]
impl gyre_ports::BudgetCallRepository for MemBudgetCallRepository {
    async fn save(&self, record: &gyre_domain::BudgetCallRecord) -> Result<()> {
        let mut store = self.store.lock().await;
        // Port contract: "Insert-only; the record id is unique." The SQL
        // adapters enforce this via the PRIMARY KEY; the mem adapter guards
        // in code (check-mem-port-contracts.sh) so both backends reject a
        // duplicate id identically.
        if store.iter().any(|r| r.id == record.id) {
            anyhow::bail!("budget call record {} already exists", record.id);
        }
        store.push(record.clone());
        Ok(())
    }

    async fn list_by_workspace(
        &self,
        workspace_id: &str,
        since: u64,
        limit: i64,
    ) -> Result<Vec<gyre_domain::BudgetCallRecord>> {
        let mut records: Vec<gyre_domain::BudgetCallRecord> = self
            .store
            .lock()
            .await
            .iter()
            .filter(|r| r.workspace_id.as_str() == workspace_id && r.timestamp >= since)
            .cloned()
            .collect();
        records.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then(b.id.as_str().cmp(a.id.as_str()))
        });
        records.truncate(limit.max(0) as usize);
        Ok(records)
    }

    async fn list_since(
        &self,
        since: u64,
        limit: i64,
    ) -> Result<Vec<gyre_domain::BudgetCallRecord>> {
        let store = self.store.lock().await;
        let mut records: Vec<gyre_domain::BudgetCallRecord> = store
            .iter()
            .filter(|r| r.timestamp >= since)
            .cloned()
            .collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.timestamp));
        if limit >= 0 {
            records.truncate(limit as usize);
        }
        Ok(records)
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
pub struct MemSpecLifecycleRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::SpecLifecycleConfig>>>,
}

#[async_trait]
impl gyre_ports::SpecLifecycleRepository for MemSpecLifecycleRepository {
    async fn get_for_repo(&self, repo_id: &str) -> Result<gyre_domain::SpecLifecycleConfig> {
        Ok(self
            .store
            .lock()
            .await
            .get(repo_id)
            .cloned()
            .unwrap_or_default())
    }
    async fn set_for_repo(
        &self,
        repo_id: &str,
        config: gyre_domain::SpecLifecycleConfig,
    ) -> Result<()> {
        self.store.lock().await.insert(repo_id.to_string(), config);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemSpecApprovalRepository {
    store: Arc<Mutex<HashMap<String, gyre_domain::SpecApproval>>>,
}
impl MemSpecApprovalRepository {
    /// Apply a domain transition (approve/revoke/reject) to a stored entry:
    /// load, validate via the domain's transition rules, persist the full
    /// record so mutual exclusivity is guaranteed. Returns Ok(None) when the
    /// entry does not exist.
    async fn transition(
        &self,
        id: &Id,
        apply: impl FnOnce(&mut gyre_domain::SpecApproval) -> Result<(), ApprovalTransitionError>,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let mut store = self.store.lock().await;
        let Some(approval) = store.get_mut(&id.to_string()) else {
            return Ok(None);
        };
        apply(approval)?;
        Ok(Some(()))
    }
}
#[async_trait]
impl gyre_ports::SpecApprovalRepository for MemSpecApprovalRepository {
    async fn create(&self, approval: &gyre_domain::SpecApproval) -> Result<()> {
        let mut store = self.store.lock().await;
        if store.contains_key(&approval.id.to_string()) {
            anyhow::bail!("spec approval {} already exists", approval.id);
        }
        store.insert(approval.id.to_string(), approval.clone());
        Ok(())
    }
    async fn find_by_id(&self, id: &gyre_common::Id) -> Result<Option<gyre_domain::SpecApproval>> {
        Ok(self.store.lock().await.get(&id.to_string()).cloned())
    }
    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<gyre_domain::SpecApproval>> {
        let mut out: Vec<_> = self
            .store
            .lock()
            .await
            .values()
            .filter(|a| a.spec_path == spec_path)
            .cloned()
            .collect();
        out.reverse(); // insertion order: newest first, mirroring SQL id-desc
        Ok(out)
    }
    async fn find_by_spec_sha(
        &self,
        spec_path: &str,
        spec_sha: &str,
    ) -> Result<Vec<gyre_domain::SpecApproval>> {
        let mut out: Vec<_> = self
            .store
            .lock()
            .await
            .values()
            .filter(|a| a.spec_path == spec_path && a.spec_sha == spec_sha)
            .cloned()
            .collect();
        out.reverse(); // insertion order: newest first, mirroring SQL id-desc
        Ok(out)
    }
    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<gyre_domain::SpecApproval>> {
        let mut out: Vec<_> = self
            .store
            .lock()
            .await
            .values()
            .filter(|a| a.spec_path == spec_path && a.is_active())
            .cloned()
            .collect();
        out.reverse();
        Ok(out)
    }
    async fn list_all(&self) -> Result<Vec<gyre_domain::SpecApproval>> {
        Ok(self.store.lock().await.values().cloned().collect())
    }

    async fn approve(&self, id: &Id, now: u64) -> Result<Option<()>, ApprovalTransitionError> {
        self.transition(id, |a| a.approve(now)).await
    }
    async fn revoke(
        &self,
        id: &Id,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        self.transition(id, |a| a.revoke(revoked_by, reason, now))
            .await
    }
    async fn reject(
        &self,
        id: &Id,
        rejected_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        self.transition(id, |a| a.reject(rejected_by, reason, now))
            .await
    }

    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<u64> {
        let mut store = self.store.lock().await;
        let mut revoked = 0u64;
        for a in store.values_mut() {
            if a.spec_path == spec_path
                && a.status() == gyre_domain::spec_approval::ApprovalStatus::Approved
                && a.revoke(revoked_by, reason, now).is_ok()
            {
                revoked += 1;
            }
        }
        Ok(revoked)
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
pub struct MemSpecLinkRepository {
    store: Arc<Mutex<Vec<gyre_domain::spec_links::SpecLinkEntry>>>,
}

#[async_trait]
impl gyre_ports::SpecLinkRepository for MemSpecLinkRepository {
    async fn list_all(&self) -> Result<Vec<gyre_domain::spec_links::SpecLinkEntry>> {
        Ok(self.store.lock().await.clone())
    }
    async fn replace_for_source(
        &self,
        source_repo_id: &str,
        source_path: &str,
        links: &[gyre_domain::spec_links::SpecLinkEntry],
    ) -> Result<()> {
        let mut store = self.store.lock().await;
        // Match the SQL adapters' (source_repo_id, source_path) scoping:
        // `source_repo_id` None in the entry means the unscoped/legacy bucket.
        let source_repo_key = |e: &gyre_domain::spec_links::SpecLinkEntry| {
            e.source_repo_id.clone().unwrap_or_default()
        };
        store.retain(|l| !(source_repo_key(l) == source_repo_id && l.source_path == source_path));
        store.extend(links.iter().cloned());
        Ok(())
    }
    async fn save(&self, entry: &gyre_domain::spec_links::SpecLinkEntry) -> Result<()> {
        let mut store = self.store.lock().await;
        match store.iter().position(|l| l.id == entry.id) {
            Some(i) => store[i] = entry.clone(),
            None => store.push(entry.clone()),
        }
        Ok(())
    }
    async fn delete_by_source_repo(&self, source_repo_id: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .retain(|l| l.source_repo_id.as_deref().unwrap_or_default() != source_repo_id);
        Ok(())
    }
}

/// Task-198 AC: `SpecLinkEntry` must round-trip losslessly through the mem
/// adapter (used by the in-memory test constructor and mem-mode server) —
/// every `SpecLinkType` variant and every `Option` column, with the same
/// `(source_repo_id, source_path)` scoping contract the SQL adapters enforce.
#[cfg(test)]
mod spec_link_contract_tests {
    use super::*;
    use gyre_domain::spec_links::{SpecLinkEntry, SpecLinkType};
    use gyre_ports::SpecLinkRepository;

    fn entry(id: &str, link_type: SpecLinkType) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: format!("system/source-{id}.md"),
            source_repo_id: Some("repo-1".to_string()),
            source_sha: format!("sha-{id}"),
            link_type,
            target_path: "system/target.md".to_string(),
            target_repo_id: Some("repo-2".to_string()),
            target_display: Some("@ws/repo-2/system/target.md".to_string()),
            target_sha: Some("target-sha".to_string()),
            reason: Some("why".to_string()),
            status: "active".to_string(),
            created_at: 1_700_000_000,
            stale_since: None,
        }
    }

    #[tokio::test]
    async fn round_trips_every_link_type_and_option_columns() {
        let repo = MemSpecLinkRepository::default();
        let variants = [
            SpecLinkType::Implements,
            SpecLinkType::Supersedes,
            SpecLinkType::DependsOn,
            SpecLinkType::ConflictsWith,
            SpecLinkType::Extends,
            SpecLinkType::References,
        ];
        let mut expected: Vec<SpecLinkEntry> = Vec::new();
        for (i, lt) in variants.iter().enumerate() {
            let e = entry(&format!("link-{i}"), lt.clone());
            repo.save(&e).await.unwrap();
            expected.push(e);
        }
        // Option columns exercised in their None forms too.
        let mut none_opts = entry("link-none", SpecLinkType::DependsOn);
        none_opts.source_repo_id = None;
        none_opts.target_repo_id = None;
        none_opts.target_display = None;
        none_opts.target_sha = None;
        none_opts.reason = None;
        none_opts.stale_since = Some(1_700_000_500);
        repo.save(&none_opts).await.unwrap();
        expected.push(none_opts);

        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), expected.len());
        for exp in &expected {
            let got = all.iter().find(|l| l.id == exp.id).expect("link present");
            assert_eq!(
                got,
                exp,
                "entry {id} must round-trip losslessly",
                id = exp.id
            );
        }
    }

    #[tokio::test]
    async fn replace_for_source_scopes_to_repo_and_path() {
        let repo = MemSpecLinkRepository::default();
        let old = entry("old-1", SpecLinkType::Implements);
        repo.save(&old).await.unwrap();
        // Unrelated link — must survive the replace.
        let other = entry("other-1", SpecLinkType::Extends);
        repo.save(&other).await.unwrap();

        let replacement = entry("new-1", SpecLinkType::DependsOn);
        repo.replace_for_source(
            "repo-1",
            &old.source_path,
            std::slice::from_ref(&replacement),
        )
        .await
        .unwrap();

        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 2, "old row replaced, unrelated row kept");
        assert!(all.iter().any(|l| l.id == "new-1"));
        assert!(all.iter().any(|l| l.id == "other-1"));
        assert!(!all.iter().any(|l| l.id == "old-1"));
    }

    #[tokio::test]
    async fn replace_for_source_empty_set_clears_rows() {
        let repo = MemSpecLinkRepository::default();
        let e = entry("gone-1", SpecLinkType::Implements);
        repo.save(&e).await.unwrap();
        // A manifest edit that removes all links must clear the rows, not
        // skip them (an empty set is a delete, matching sync_spec_ledger).
        repo.replace_for_source("repo-1", &e.source_path, &[])
            .await
            .unwrap();
        assert!(repo.list_all().await.unwrap().is_empty());
    }
}

#[derive(Default)]
pub struct MemSpecAssertionResultRepository {
    store: Arc<Mutex<Vec<gyre_domain::SpecAssertionResult>>>,
}

#[async_trait]
impl gyre_ports::SpecAssertionResultRepository for MemSpecAssertionResultRepository {
    async fn save_results(&self, results: &[gyre_domain::SpecAssertionResult]) -> Result<()> {
        if results.is_empty() {
            return Ok(());
        }
        let mut store = self.store.lock().await;
        // Replace semantics: drop prior rows for the same (repo_id, spec_path)
        // so the stored set reflects the latest check only.
        let repo_id = results[0].repo_id.clone();
        let spec_path = results[0].spec_path.clone();
        store.retain(|r| !(r.repo_id == repo_id && r.spec_path == spec_path));
        store.extend(results.iter().cloned());
        Ok(())
    }

    async fn delete_by_spec(&self, repo_id: &str, spec_path: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .retain(|r| !(r.repo_id == repo_id && r.spec_path == spec_path));
        Ok(())
    }

    async fn list_by_spec(
        &self,
        repo_id: &str,
        spec_path: &str,
    ) -> Result<Vec<gyre_domain::SpecAssertionResult>> {
        let mut rows: Vec<gyre_domain::SpecAssertionResult> = self
            .store
            .lock()
            .await
            .iter()
            .filter(|r| r.repo_id == repo_id && r.spec_path == spec_path)
            .cloned()
            .collect();
        rows.sort_by_key(|r| r.line);
        Ok(rows)
    }

    async fn list_spec_paths(&self, repo_id: &str) -> Result<Vec<String>> {
        let mut paths: Vec<String> = self
            .store
            .lock()
            .await
            .iter()
            .filter(|r| r.repo_id == repo_id)
            .map(|r| r.spec_path.clone())
            .collect();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    async fn delete_by_repo(&self, repo_id: &str) -> Result<()> {
        self.store.lock().await.retain(|r| r.repo_id != repo_id);
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

/// In-memory `TenantMetaSpecSetRepository` — same contract as
/// `MemMetaSpecSetRepository`, keyed by tenant id. Kept in a separate store
/// so a tenant and a workspace that happen to share an id string never
/// shadow each other's set (mirrors the two DB tables).
#[derive(Default)]
pub struct MemTenantMetaSpecSetRepository {
    store: Arc<Mutex<HashMap<String, String>>>,
}

#[async_trait]
impl TenantMetaSpecSetRepository for MemTenantMetaSpecSetRepository {
    async fn get(&self, tenant_id: &Id) -> Result<Option<String>> {
        Ok(self.store.lock().await.get(tenant_id.as_str()).cloned())
    }

    async fn upsert(&self, tenant_id: &Id, json: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .insert(tenant_id.as_str().to_string(), json.to_string());
        Ok(())
    }

    async fn delete(&self, tenant_id: &Id) -> Result<()> {
        self.store.lock().await.remove(tenant_id.as_str());
        Ok(())
    }
}

// ── MemRolloutPolicyRepository ──────────────────────────────────────────────

#[derive(Default)]
pub struct MemRolloutPolicyRepository {
    store: Arc<Mutex<HashMap<String, String>>>,
}

#[async_trait]
impl RolloutPolicyRepository for MemRolloutPolicyRepository {
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

/// Stored conversation blob with its scope identity. Named carrier
/// (TASK-048 F1): the previous positional 4-tuple
/// `(agent_id, workspace_id, tenant_id, blob)` was a field-swap hazard.
struct ConversationBlob {
    agent_id: String,
    workspace_id: String,
    tenant_id: String,
    data: Vec<u8>,
}

/// sha -> stored conversation blob
type ConvMap = Arc<Mutex<HashMap<String, ConversationBlob>>>;

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
            .or_insert_with(|| ConversationBlob {
                agent_id: agent_id.as_str().to_string(),
                workspace_id: workspace_id.as_str().to_string(),
                tenant_id: tenant_id.as_str().to_string(),
                data: conversation.to_vec(),
            });
        Ok(sha)
    }

    async fn get(&self, conversation_sha: &str, tenant_id: &Id) -> Result<Option<Vec<u8>>> {
        let guard = self.convs.lock().await;
        let Some(record) = guard.get(conversation_sha) else {
            return Ok(None);
        };
        if record.tenant_id != tenant_id.as_str() {
            return Ok(None);
        }
        // Decompress.
        let decompressed = zstd::decode_all(record.data.as_slice())
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
        let Some(record) = guard.get(conversation_sha) else {
            return Ok(None);
        };
        if record.tenant_id != tenant_id.as_str() {
            return Ok(None);
        }
        Ok(Some((
            Id::new(&record.agent_id),
            Id::new(&record.workspace_id),
        )))
    }

    async fn list_by_agent(&self, agent_id: &Id, tenant_id: &Id) -> Result<Vec<String>> {
        let guard = self.convs.lock().await;
        Ok(guard
            .iter()
            .filter(|(_, rec)| {
                rec.agent_id == agent_id.as_str() && rec.tenant_id == tenant_id.as_str()
            })
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
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides::default(),
    )
}

/// Build an AppState with a custom JjOpsPort for tests that need to
/// control/observe jj behavior (e.g., programmed rebase outcomes).
#[cfg(test)]
pub fn test_state_with_jj_ops(jj_ops: Arc<dyn gyre_ports::JjOpsPort>) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    let mut state = test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides::default(),
    );
    let s = Arc::get_mut(&mut state).expect("test state is uniquely owned");
    s.jj_ops = jj_ops;
    state
}

/// Build a test AppState backed by a real `StoragePort` (e.g. a temp-file
/// SQLite instance) so infrastructure health probes evaluate real storage.
#[cfg(test)]
pub fn test_state_with_storage(
    storage: Arc<dyn gyre_ports::storage::StoragePort>,
) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides {
            storage: Some(storage),
            ..Default::default()
        },
    )
}

/// Build a test AppState whose search port is the given adapter — used to
/// exercise the reindex coordinator against a real SQLite FTS5 backend.
#[cfg(test)]
pub fn test_state_with_search(search: Arc<dyn gyre_ports::SearchPort>) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides {
            search: Some(search),
            ..Default::default()
        },
    )
}

/// Build a test AppState with custom JjOpsPort and GitOpsPort — used by
/// tests that drive the spawn path with a real git adapter and an
/// observing jj double (e.g. F9: branch creation before jj provisioning).
#[cfg(test)]
pub fn test_state_with_jj_and_git_ops(
    jj_ops: Arc<dyn gyre_ports::JjOpsPort>,
    git_ops: Arc<dyn gyre_ports::GitOpsPort>,
) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    let mut state = test_state_inner(
        git_ops,
        workspaces,
        policies,
        ws_store,
        TestStateOverrides::default(),
    );
    let s = Arc::get_mut(&mut state).expect("test state is uniquely owned");
    s.jj_ops = jj_ops;
    state
}

/// Build a test AppState with a custom `SecretRepository` — used to exercise
/// the spawn path's resolve-failure branch (task-097 F2) with a port that
/// always errors, everything else in-memory and real.
#[cfg(test)]
pub fn test_state_with_secrets(
    secrets: Arc<dyn gyre_ports::SecretRepository>,
) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides {
            secrets: Some(secrets),
            ..Default::default()
        },
    )
}

/// Build a test AppState with a custom KvJsonStore — used to exercise
/// kv read-failure paths (e.g. the rolling-rollout pending-state merge
/// must abort, not overwrite, when the stored state cannot be read).
#[cfg(test)]
pub fn test_state_with_kv_store(kv: Arc<dyn gyre_ports::KvJsonStore>) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides {
            kv: Some(kv),
            ..Default::default()
        },
    )
}

/// Build a test AppState whose workspace repo fails every `apply_trust_transition`,
/// used to exercise the `update_workspace`/`create_workspace` 409 error path.
#[cfg(test)]
pub fn test_state_failing_trust() -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(true);
    test_state_inner(
        Arc::new(NoopGitOps),
        workspaces,
        policies,
        ws_store,
        TestStateOverrides::default(),
    )
}

/// Build a test AppState whose policy repo fails every `create`, used to
/// exercise fail-closed built-in policy seeding (task-077 F6 class).
#[cfg(test)]
pub fn test_state_failing_policy_creates() -> Arc<crate::AppState> {
    let pol_store = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let policies = MemPolicyRepository::from_store(Arc::clone(&pol_store));
    policies.fail_creates();
    let ws_store = Arc::new(Mutex::new(HashMap::<String, Workspace>::new()));
    let workspaces = MemWorkspaceRepository::with_policy_store(pol_store)
        .with_workspace_store(Arc::clone(&ws_store));
    test_state_inner(
        Arc::new(NoopGitOps),
        Arc::new(workspaces),
        Arc::new(policies),
        ws_store,
        TestStateOverrides::default(),
    )
}

/// Build an AppState whose policy repo fails every `list`, used to exercise
/// the fail-closed supervised-trust merge gate (task-077 F9): a policy-store
/// read failure must HOLD the merge, never degrade to "no policies".
#[cfg(test)]
pub fn test_state_failing_policy_lists() -> Arc<crate::AppState> {
    let pol_store = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let policies = MemPolicyRepository::from_store(Arc::clone(&pol_store));
    policies.fail_lists();
    let ws_store = Arc::new(Mutex::new(HashMap::<String, Workspace>::new()));
    let workspaces = MemWorkspaceRepository::with_policy_store(pol_store)
        .with_workspace_store(Arc::clone(&ws_store));
    test_state_inner(
        Arc::new(NoopGitOps),
        Arc::new(workspaces),
        Arc::new(policies),
        ws_store,
        TestStateOverrides::default(),
    )
}

/// The paired workspace + policy repositories (and the shared workspace
/// store behind them) returned by `shared_workspace_policy_pair`. A named
/// alias keeps the tuple's use sites readable and under clippy's
/// `type_complexity` threshold.
#[cfg(test)]
type SharedWorkspacePolicyPair = (
    Arc<dyn WorkspaceRepository>,
    Arc<dyn gyre_ports::PolicyRepository>,
    Arc<Mutex<HashMap<String, Workspace>>>,
);

/// Construct a paired workspace + policy repo that share a single in-memory
/// policy store so `apply_trust_transition` writes are visible through
/// `state.policies`. When `fail_trust` is set, the workspace repo rejects every
/// trust transition (simulating a DB rollback). Also returns the shared
/// workspace store so the compute-target repo can pair with it (real
/// `has_workspace_references` in tests).
#[cfg(test)]
fn shared_workspace_policy_pair(fail_trust: bool) -> SharedWorkspacePolicyPair {
    let pol_store = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let policies = Arc::new(MemPolicyRepository::from_store(Arc::clone(&pol_store)));
    let ws_store = Arc::new(Mutex::new(HashMap::<String, Workspace>::new()));
    let workspaces = MemWorkspaceRepository::with_policy_store(pol_store)
        .with_workspace_store(Arc::clone(&ws_store));
    if fail_trust {
        workspaces.fail_trust_transitions();
    }
    (Arc::new(workspaces), policies, ws_store)
}

/// Build an AppState with a custom GitOpsPort for tests that need to control
/// git operation behavior (e.g., configurable `can_merge` results).
#[cfg(test)]
pub fn test_state_with_git_ops(git_ops: Arc<dyn gyre_ports::GitOpsPort>) -> Arc<crate::AppState> {
    let (workspaces, policies, ws_store) = shared_workspace_policy_pair(false);
    test_state_inner(
        git_ops,
        workspaces,
        policies,
        ws_store,
        TestStateOverrides::default(),
    )
}
/// Optional port overrides for `test_state_inner`: each field replaces the
/// default in-memory implementation when `Some`. Bundling them in one struct
/// keeps the shared builder under clippy's `too_many_arguments` threshold.
#[cfg(test)]
#[derive(Default)]
struct TestStateOverrides {
    storage: Option<Arc<dyn gyre_ports::storage::StoragePort>>,
    search: Option<Arc<dyn gyre_ports::SearchPort>>,
    secrets: Option<Arc<dyn gyre_ports::SecretRepository>>,
    kv: Option<Arc<dyn gyre_ports::KvJsonStore>>,
}

/// Shared builder for all in-memory test states. Callers supply the git ops
/// adapter plus a paired workspace/policy repo (see
/// `shared_workspace_policy_pair`); `overrides` replaces individual default
/// in-memory ports (storage/search/secrets/kv) when set.
#[cfg(test)]
fn test_state_inner(
    git_ops: Arc<dyn gyre_ports::GitOpsPort>,
    workspaces: Arc<dyn WorkspaceRepository>,
    policies: Arc<dyn gyre_ports::PolicyRepository>,
    ws_store: Arc<Mutex<HashMap<String, Workspace>>>,
    overrides: TestStateOverrides,
) -> Arc<crate::AppState> {
    let spec_link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
        Arc::new(MemSpecLinkRepository::default());
    let spec_links_store = crate::spec_registry::SpecLinksStore::default();
    crate::load_spec_links_into_store(&spec_link_repo, &spec_links_store);
    use std::collections::HashMap;
    use tokio::sync::{broadcast, Mutex};

    let TestStateOverrides {
        storage,
        search,
        secrets,
        kv,
    } = overrides;

    // Shared binding store: the meta-spec delete guard must see bindings
    // written through `meta_spec_bindings` (same wiring as lib.rs).
    let meta_spec_binding_store = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    // Mirror production wiring (lib.rs): entity repositories are wrapped in
    // search-sync decorators so tests exercise the same index-on-write
    // behavior as every real write path. The search port is wrapped in the
    // tenant-resolution decorator (search_scope) exactly like production.
    let search = crate::search_scope::with_tenant_resolution(
        search.unwrap_or_else(|| {
            Arc::new(gyre_adapters::MemSearchAdapter::new()) as Arc<dyn gyre_ports::SearchPort>
        }),
        Arc::clone(&workspaces),
    );
    let agents = crate::search_sync::wrap_agents(
        Arc::new(MemAgentRepository::default()),
        Arc::clone(&search),
    );
    let tasks =
        crate::search_sync::wrap_tasks(Arc::new(MemTaskRepository::default()), Arc::clone(&search));
    let merge_requests =
        crate::search_sync::wrap_mrs(Arc::new(MemMrRepository::default()), Arc::clone(&search));
    let reviews = crate::search_sync::wrap_reviews(
        Arc::new(MemReviewRepository::default()),
        Arc::clone(&merge_requests),
        Arc::clone(&search),
    );
    let personas = crate::search_sync::wrap_personas(
        Arc::new(MemPersonaRepository::default()),
        Arc::clone(&search),
    );
    let analytics = crate::search_sync::wrap_analytics(
        Arc::new(MemAnalyticsRepository::default()),
        Arc::clone(&search),
    );
    let audit = crate::search_sync::wrap_audit(
        Arc::new(MemAuditRepository::default()),
        Arc::clone(&search),
    );

    Arc::new(crate::AppState {
        auth_token: "test-token".to_string(),
        base_url: "http://localhost:3000".to_string(),
        repos: Arc::new(MemRepoRepository::default()),
        agents,
        tasks,
        merge_requests,
        reviews,
        merge_queue: Arc::new(MemMergeQueueRepository::default()),
        git_ops,
        jj_ops: Arc::new(NoopJjOps),
        agent_commits: Arc::new(MemAgentCommitRepository::default()),
        worktrees: Arc::new(MemWorktreeRepository::default()),
        telemetry_buffer: Arc::new(gyre_common::message::TelemetryBuffer::new(1_000, 10)),
        message_broadcast_tx: broadcast::channel(16).0,
        kv_store: kv.unwrap_or_else(|| Arc::new(MemKvStore::default())),
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
        explorer_sessions: Arc::new(crate::explorer_ws::ExplorerSessionRegistry::new()),
        retention_store: crate::retention::RetentionStore::new(),
        job_registry: Arc::new(crate::jobs::JobRegistry::new()),
        budget_grace_in_progress: Arc::new(tokio::sync::Mutex::new(
            std::collections::HashSet::new(),
        )),
        analytics,
        costs: Arc::new(MemCostRepository::default()),
        audit,
        siem_store: crate::siem::SiemStore::new(),
        audit_broadcast_tx: broadcast::channel(64).0,
        notification_broadcast_tx: broadcast::channel(64).0,
        network_peers: Arc::new(MemNetworkPeerRepository::default()),
        dependencies: Arc::new(MemDependencyRepository::default()),
        breaking_changes: Arc::new(MemBreakingChangeRepository::default()),
        dependency_policies: Arc::new(MemDependencyPolicyRepository::default()),
        rate_limiter: crate::rate_limit::RateLimiter::new(1000),
        process_registry: Arc::new(Mutex::new(HashMap::new())),
        spawned_backends: Arc::new(Mutex::new(HashMap::new())),
        agent_logs: Arc::new(crate::log_compression::AgentLogBuffer::new()),
        agent_log_tx: Arc::new(Mutex::new(HashMap::new())),
        quality_gates: Arc::new(MemQualityGateRepository::default()),
        gate_results: Arc::new(MemGateResultRepository::default()),
        spec_lifecycle_configs: Arc::new(MemSpecLifecycleRepository::default()),
        push_gate_registry: Arc::new(crate::pre_accept::builtin_gates()),
        repo_push_gates: Arc::new(MemPushGateRepository::default()),
        speculative_results: Arc::new(Mutex::new(HashMap::new())),
        spawn_log: Arc::new(MemSpawnLogRepository::default()),
        db_storage: None,
        storage,
        spec_approvals: Arc::new(MemSpecApprovalRepository::default()),
        spec_policies: Arc::new(MemSpecPolicyRepository::default()),
        attestation_store: Arc::new(MemAttestationRepository::default()),
        chain_attestations: Arc::new(MemChainAttestationRepository::default()),
        key_bindings: Arc::new(MemKeyBindingRepository::default()),
        trusted_issuers: vec![],
        commit_signatures: Arc::new(MemCommitSignatureRepository::default()),
        signing_config: crate::commit_signatures::SigningConfig::default(),
        remote_jwks_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        trusted_proxies: crate::trusted_proxy::TrustedProxies::default(),
        tunnel_store: Arc::new(Mutex::new(HashMap::new())),
        container_audits: Arc::new(MemContainerAuditRepository::default()),
        spec_ledger: Arc::new(MemSpecLedgerRepository::default()),
        spec_approval_history: Arc::new(MemSpecApprovalEventRepository::default()),
        spec_link_repo: spec_link_repo.clone(),
        spec_assertion_results: Arc::new(MemSpecAssertionResultRepository::default()),
        trust_anchors: Arc::new(MemTrustAnchorRepository::default()),
        spec_links_store,
        budget_configs: Arc::new(MemBudgetConfigRepository::default()),
        budget_usages: Arc::new(MemBudgetUsageRepository::default()),
        budget_calls: Arc::new(MemBudgetCallRepository::default()),
        search,
        tenants: Arc::new(MemTenantRepository::default()),
        workspaces,
        personas,
        policies,
        workspace_memberships: Arc::new(MemWorkspaceMembershipRepository::default()),
        repo_access_overrides: Arc::new(MemRepoAccessOverrideRepository::default()),
        teams: Arc::new(MemTeamRepository::default()),
        notifications: Arc::new(MemNotificationRepository::default()),
        graph_store: Arc::new(gyre_adapters::MemGraphStore::new()),
        saved_views: Arc::new(gyre_adapters::MemSavedViewRepository::default()),
        wg_config: crate::WireGuardConfig::from_env(),
        agent_inbox_max: 1000,
        user_workspace_state: Arc::new(MemUserWorkspaceStateRepository::default()),
        rollout_policies: Arc::new(MemRolloutPolicyRepository::default()),
        messages: Arc::new(MemMessageRepository::default()),
        message_dispatch_tx: {
            let (tx, rx) = tokio::sync::mpsc::channel(256);
            tokio::spawn(async move {
                let mut rx = rx;
                while rx.recv().await.is_some() {}
            });
            tx
        },
        meta_specs: Arc::new(MemMetaSpecRepository::with_binding_store(Arc::clone(
            &meta_spec_binding_store,
        ))),
        meta_spec_bindings: Arc::new(MemMetaSpecBindingRepository::with_store(Arc::clone(
            &meta_spec_binding_store,
        ))),
        meta_spec_sets: Arc::new(MemMetaSpecSetRepository::default()),
        tenant_meta_spec_sets: Arc::new(MemTenantMetaSpecSetRepository::default()),
        last_seen_debounce: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        llm_rate_limiter: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_configs: Arc::new(MemLlmConfigRepository::default()),
        presence: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        ws_connections: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        ws_connection_counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        ws_connection_workspaces: Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        ws_connection_users: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        traces: Arc::new(MemTraceRepository::default()),
        otlp_config: crate::otlp_receiver::OtlpServerConfig {
            enabled: false,
            grpc_port: 4317,
            max_spans_per_trace: 10_000,
        },
        conversations: Arc::new(MemConversationRepository::default()),
        repos_root: format!("/tmp/gyre-unit-test-repos-{}", std::process::id()),
        prompt_templates: Arc::new(MemPromptRepository::default()),
        compute_targets: Arc::new(MemComputeTargetRepository::with_workspace_store(ws_store)),
        llm: Some(Arc::new(gyre_adapters::MockLlmPortFactory::echo())),
        user_notification_prefs: Arc::new(MemUserNotificationPreferenceRepository::default()),
        user_channel_prefs: Arc::new(MemUserChannelPreferenceRepository::default()),
        user_tokens: Arc::new(MemUserTokenRepository::default()),
        judgment_ledger: Arc::new(MemJudgmentLedgerRepository),
        secrets: secrets.unwrap_or_else(|| Arc::new(MemSecretRepository::default())),
        orchestrator_registry: crate::signal_chain::OrchestratorRegistry::new(),
        ws_tickets: crate::auth::WsTicketStore::new(),
    })
}

// ── In-memory SecretRepository ──────────────────────────────────────────────

#[derive(Default)]
pub struct MemSecretRepository {
    /// (tenant_id, Secret, encrypted-at-rest-equivalent value).
    /// Plaintext is held only here in the adapter-equivalent layer, never in
    /// the Secret metadata.
    store: Arc<Mutex<Vec<(String, gyre_common::Secret, Vec<u8>)>>>,
}

fn mem_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl gyre_ports::SecretRepository for MemSecretRepository {
    async fn create(&self, secret: &gyre_common::Secret, value: &[u8]) -> Result<()> {
        let mut store = self.store.lock().await;
        // Mirror the SQLite UNIQUE(id) + UNIQUE(tenant, scope, scope_id, name)
        // failure mode: reject duplicates instead of silently shadowing them
        // (get_value/rotate only ever find the first entry).
        if store.iter().any(|(tid, s, _)| {
            tid == &secret.tenant_id
                && (s.id == secret.id
                    || (s.scope == secret.scope
                        && s.scope_id == secret.scope_id
                        && s.name == secret.name))
        }) {
            anyhow::bail!(
                "secret {} already exists in tenant {} (duplicate id or scope/name)",
                secret.id,
                secret.tenant_id
            );
        }
        store.push((secret.tenant_id.clone(), secret.clone(), value.to_vec()));
        Ok(())
    }

    async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|(tid, s, _)| tid == tenant_id && s.id == *id)
            .map(|(_, _, v)| v.clone()))
    }

    async fn find_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<gyre_common::Secret>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|(tid, s, _)| tid == tenant_id && s.id == *id)
            .map(|(_, s, _)| s.clone()))
    }

    async fn list_by_scope(
        &self,
        scope: gyre_common::SecretScope,
        scope_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<gyre_common::Secret>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|(tid, s, _)| tid == tenant_id && s.scope == scope && s.scope_id == scope_id)
            .map(|(_, s, _)| s.clone())
            .collect())
    }

    async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .retain(|(tid, s, _)| !(tid == tenant_id && s.id == *id));
        Ok(())
    }

    async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()> {
        let mut store = self.store.lock().await;
        let entry = store
            .iter_mut()
            .find(|(tid, s, _)| tid == tenant_id && s.id == *id);
        match entry {
            Some((_, s, v)) => {
                *v = new_value.to_vec();
                s.last_rotated_at = Some(mem_now_secs());
                Ok(())
            }
            None => anyhow::bail!("secret {} not found in tenant {}", id, tenant_id),
        }
    }

    async fn resolve_for_agent(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let now = mem_now_secs();
        let store = self.store.lock().await;
        // Coarse-to-fine: later entries overwrite same-name finer-scope wins.
        let mut targets: Vec<(&str, &str)> = vec![
            (gyre_common::SecretScope::Tenant.as_str(), tenant_id),
            (gyre_common::SecretScope::Workspace.as_str(), workspace_id),
            (gyre_common::SecretScope::Repo.as_str(), repo_id),
        ];
        if let Some(tid) = task_id {
            targets.push((gyre_common::SecretScope::Task.as_str(), tid));
        }
        let mut by_name: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        for (scope, sid) in targets {
            for (tid, s, v) in store.iter() {
                if tid == tenant_id
                    && s.scope.as_str() == scope
                    && s.scope_id == sid
                    && !s.is_expired(now)
                {
                    by_name.insert(s.name.clone(), v.clone());
                }
            }
        }
        let mut resolved: Vec<(String, Vec<u8>)> = by_name.into_iter().collect();
        resolved.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(resolved)
    }
}

// ── In-memory TraceRepository ────────────────────────────────────────────────

/// Mirror of `sqlite/trace.rs` / `postgres/trace.rs` `MAX_SUMMARY_BYTES`:
/// stored (column-side) span summaries are capped at 4KB. The full values
/// live in the payload rows (mirroring the SQLite blob columns).
const MAX_SUMMARY_BYTES: usize = 4096;

/// Mirror of the adapters' `truncate_summary`: cap at 4KB on a char boundary
/// (same clamped-`end` idiom as `sqlite/trace.rs:122-133`).
fn truncate_summary(s: &str) -> String {
    if s.len() <= MAX_SUMMARY_BYTES {
        return s.to_string();
    }
    let mut end = MAX_SUMMARY_BYTES;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[derive(Default)]
pub struct MemTraceRepository {
    store: Arc<Mutex<HashMap<String, gyre_common::GateTrace>>>,
    payloads: Arc<Mutex<HashMap<(String, String), gyre_ports::trace::SpanPayload>>>,
    /// MR IDs whose traces were promoted to attestation - mirrors the SQLite
    /// adapter's `permanent=1` flag: promoted traces survive `delete_by_mr`.
    permanent: Arc<Mutex<std::collections::HashSet<String>>>,
}

#[async_trait]
impl gyre_ports::TraceRepository for MemTraceRepository {
    async fn store(&self, trace: &gyre_common::GateTrace) -> Result<()> {
        let mut guard = self.store.lock().await;
        // Replace any existing trace for same MR (capped at most recent).
        // A fresh capture is non-permanent (SQLite inserts permanent=0).
        self.permanent.lock().await.remove(trace.mr_id.as_str());
        // Drop the replaced trace's payload rows too (SQLite deletes them
        // via the PK replace in the same transaction).
        let replaced_gate_run_ids: Vec<String> = guard
            .values()
            .filter(|v| v.mr_id == trace.mr_id)
            .map(|v| v.gate_run_id.as_str().to_string())
            .collect();
        guard.retain(|_, v| v.mr_id != trace.mr_id);
        {
            let mut payloads = self.payloads.lock().await;
            for grid in replaced_gate_run_ids {
                payloads.retain(|(g, _), _| g != &grid);
            }
            // Mirror sqlite/trace.rs build_payload_blob: a payload row exists
            // whenever either summary side is Some (SQLite builds a blob from
            // Some("") too), carries the RAW (untruncated) summaries, and an
            // empty side decodes back to None. A span with neither side
            // captured stores no payload row (SQLite blob is None → 404).
            for span in &trace.spans {
                if span.input_summary.is_none() && span.output_summary.is_none() {
                    continue;
                }
                let payload_side = |s: &Option<String>| {
                    s.as_deref()
                        .filter(|v| !v.is_empty())
                        .map(|v| v.as_bytes().to_vec())
                };
                payloads.insert(
                    (trace.gate_run_id.as_str().to_string(), span.span_id.clone()),
                    gyre_ports::trace::SpanPayload {
                        input: payload_side(&span.input_summary),
                        output: payload_side(&span.output_summary),
                    },
                );
            }
        }
        // SQLite stores the truncated summary in the columns (full value only
        // in the payload blob). Without mirroring here, in-memory mode would
        // return >4KB summaries from GET /merge-requests/:id/trace while the
        // SQL adapters return 4KB.
        let mut stored = trace.clone();
        for span in &mut stored.spans {
            span.input_summary = span.input_summary.as_deref().map(truncate_summary);
            span.output_summary = span.output_summary.as_deref().map(truncate_summary);
        }
        guard.insert(trace.mr_id.as_str().to_string(), stored);
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
        // decode_payload_blob parity (sqlite/trace.rs): empty input/output
        // byte sections come back as None, not Some(vec![]).
        let empty_to_none = |v: &Option<Vec<u8>>| -> Option<Vec<u8>> {
            match v {
                Some(b) if b.is_empty() => None,
                other => other.clone(),
            }
        };
        Ok(guard.get(&key).map(|p| gyre_ports::trace::SpanPayload {
            input: empty_to_none(&p.input),
            output: empty_to_none(&p.output),
        }))
    }

    async fn promote_to_attestation(&self, mr_id: &Id) -> Result<()> {
        self.permanent
            .lock()
            .await
            .insert(mr_id.as_str().to_string());
        Ok(())
    }

    async fn delete_by_mr(&self, mr_id: &Id) -> Result<()> {
        // Promoted traces are permanent (SQLite: `permanent=1` rows survive).
        let mut store = self.store.lock().await;
        if self.permanent.lock().await.contains(mr_id.as_str()) {
            return Ok(());
        }
        // Drop the trace's payload rows with it (SQLite: ON DELETE CASCADE).
        if let Some(trace) = store.remove(mr_id.as_str()) {
            let gate_run_id = trace.gate_run_id.as_str().to_string();
            self.payloads
                .lock()
                .await
                .retain(|(g, _), _| g != &gate_run_id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod trace_contract_tests {
    use super::*;
    use gyre_ports::TraceRepository;

    fn span(span_id: &str, input: Option<&str>, output: Option<&str>) -> gyre_common::TraceSpan {
        gyre_common::TraceSpan {
            span_id: span_id.to_string(),
            parent_span_id: None,
            operation_name: "op".to_string(),
            service_name: "svc".to_string(),
            kind: gyre_common::SpanKind::Server,
            start_time: 1,
            duration_us: 2,
            attributes: Default::default(),
            input_summary: input.map(|s| s.to_string()),
            output_summary: output.map(|s| s.to_string()),
            status: gyre_common::SpanStatus::Ok,
            graph_node_id: None,
        }
    }

    fn trace(
        mr: &str,
        gate_run: &str,
        spans: Vec<gyre_common::TraceSpan>,
    ) -> gyre_common::GateTrace {
        gyre_common::GateTrace {
            id: Id::new(format!("{mr}-trace")),
            mr_id: Id::new(mr),
            gate_run_id: Id::new(gate_run),
            commit_sha: "0".repeat(40),
            spans,
            captured_at: 1,
        }
    }

    /// Mirrors sqlite's `promote_to_attestation_preserves_trace`: the mem
    /// adapter must enforce the same lifecycle contract (spec §3a storage
    /// lifecycle) — promoted traces survive `delete_by_mr`, non-promoted
    /// traces and their payload rows are removed.
    #[tokio::test]
    async fn promoted_trace_survives_delete_by_mr() {
        let repo = MemTraceRepository::default();
        TraceRepository::store(
            &repo,
            &trace("mr-plain", "gr-1", vec![span("s1", Some("in"), None)]),
        )
        .await
        .unwrap();
        TraceRepository::delete_by_mr(&repo, &Id::new("mr-plain"))
            .await
            .unwrap();
        assert!(TraceRepository::get_by_mr(&repo, &Id::new("mr-plain"))
            .await
            .unwrap()
            .is_none());
        assert!(
            TraceRepository::get_span_payload(&repo, &Id::new("gr-1"), "s1")
                .await
                .unwrap()
                .is_none()
        );

        TraceRepository::store(
            &repo,
            &trace(
                "mr-promoted",
                "gr-2",
                vec![span("s2", Some("in"), Some("out"))],
            ),
        )
        .await
        .unwrap();
        TraceRepository::promote_to_attestation(&repo, &Id::new("mr-promoted"))
            .await
            .unwrap();
        TraceRepository::delete_by_mr(&repo, &Id::new("mr-promoted"))
            .await
            .unwrap();
        assert!(TraceRepository::get_by_mr(&repo, &Id::new("mr-promoted"))
            .await
            .unwrap()
            .is_some());
    }

    /// F2 regression: `store()` must populate the payload rows (pre-fix the
    /// mem `payloads` map was never written, so every payload lookup was
    /// None). Also pins the SQLite-mirrored keying/cascade semantics: a span
    /// with neither summary side captured stores no payload row, and a
    /// replacement gate run drops the old run's payload rows.
    #[tokio::test]
    async fn store_populates_payloads_and_replacement_cascades() {
        let repo = MemTraceRepository::default();
        TraceRepository::store(
            &repo,
            &trace(
                "mr-1",
                "gr-old",
                vec![
                    span("s-old", Some("payload-in"), Some("payload-out")),
                    span("s-nopayload", None, None),
                ],
            ),
        )
        .await
        .unwrap();
        let p = TraceRepository::get_span_payload(&repo, &Id::new("gr-old"), "s-old")
            .await
            .unwrap()
            .expect("store() must create a payload row for a span with summaries");
        assert_eq!(p.input, Some(b"payload-in".to_vec()));
        assert_eq!(p.output, Some(b"payload-out".to_vec()));
        assert!(
            TraceRepository::get_span_payload(&repo, &Id::new("gr-old"), "s-nopayload")
                .await
                .unwrap()
                .is_none(),
            "span with neither summary side captured must store no payload row"
        );

        // Replacement gate run for the same MR (SQLite: PK replace in the
        // same transaction) — old run's payload rows must not linger.
        TraceRepository::store(
            &repo,
            &trace("mr-1", "gr-new", vec![span("s-new", Some("new-in"), None)]),
        )
        .await
        .unwrap();
        assert!(
            TraceRepository::get_span_payload(&repo, &Id::new("gr-old"), "s-old")
                .await
                .unwrap()
                .is_none(),
            "replaced trace's payload rows must be dropped"
        );
        let p = TraceRepository::get_span_payload(&repo, &Id::new("gr-new"), "s-new")
            .await
            .unwrap()
            .expect("replacement trace payload");
        assert_eq!(p.input, Some(b"new-in".to_vec()));
        assert_eq!(p.output, None);
    }

    /// The stored (trace-side) summaries must be capped at 4KB like the SQL
    /// adapters' columns, while the payload rows keep the full value.
    /// Pre-fix, mem stored raw summaries and only the adapters truncated.
    #[tokio::test]
    async fn store_truncates_stored_summaries_but_keeps_full_payload() {
        let repo = MemTraceRepository::default();
        let big = "x".repeat(5000);
        TraceRepository::store(
            &repo,
            &trace("mr-t", "gr-t", vec![span("s-t", Some(&big), Some(&big))]),
        )
        .await
        .unwrap();
        let got = TraceRepository::get_by_mr(&repo, &Id::new("mr-t"))
            .await
            .unwrap()
            .expect("trace stored");
        let s = &got.spans[0];
        assert_eq!(
            s.input_summary.as_deref().map(str::len),
            Some(MAX_SUMMARY_BYTES)
        );
        assert_eq!(
            s.output_summary.as_deref().map(str::len),
            Some(MAX_SUMMARY_BYTES)
        );
        let p = TraceRepository::get_span_payload(&repo, &Id::new("gr-t"), "s-t")
            .await
            .unwrap()
            .expect("payload row exists");
        assert_eq!(p.input.as_deref().map(<[u8]>::len), Some(5000));
        assert_eq!(p.output.as_deref().map(<[u8]>::len), Some(5000));
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

pub struct MemComputeTargetRepository {
    store: Arc<tokio::sync::RwLock<Vec<gyre_domain::ComputeTargetEntity>>>,
    /// Workspace store shared with the paired `MemWorkspaceRepository` so
    /// `has_workspace_references` performs a real scan instead of a
    /// hardcoded false (agent-runtime.md §3: DELETE must fail with 409
    /// when workspaces reference the target). Defaults to an independent
    /// empty store, so standalone use (no pairing) behaves like before.
    workspaces: Arc<Mutex<HashMap<String, Workspace>>>,
}

impl Default for MemComputeTargetRepository {
    fn default() -> Self {
        Self {
            store: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            workspaces: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl MemComputeTargetRepository {
    /// Construct a compute-target repo that shares the workspace store of a
    /// paired `MemWorkspaceRepository`, giving `has_workspace_references`
    /// real cross-repo visibility.
    pub fn with_workspace_store(workspaces: Arc<Mutex<HashMap<String, Workspace>>>) -> Self {
        Self {
            store: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            workspaces,
        }
    }
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

    async fn has_workspace_references(&self, id: &Id) -> Result<bool> {
        // Real scan of the shared workspace store (same contract as the
        // SQLite adapter's join). Unpaired standalone use sees an empty
        // workspace set, which is the honest result for that mode.
        Ok(self
            .workspaces
            .lock()
            .await
            .values()
            .any(|ws| ws.compute_target_id.as_ref() == Some(id)))
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

// ─── MemUserChannelPreferenceRepository ──────────────────────────────────────

#[derive(Default)]
pub struct MemUserChannelPreferenceRepository {
    channels: Arc<tokio::sync::RwLock<Vec<(Id, gyre_domain::NotificationChannels)>>>,
}

#[async_trait]
impl gyre_ports::UserChannelPreferenceRepository for MemUserChannelPreferenceRepository {
    async fn find(&self, user_id: &Id) -> Result<Option<gyre_domain::NotificationChannels>> {
        let guard = self.channels.read().await;
        Ok(guard
            .iter()
            .find(|(uid, _)| uid == user_id)
            .map(|(_, ch)| ch.clone()))
    }

    async fn upsert(
        &self,
        user_id: &Id,
        channels: &gyre_domain::NotificationChannels,
    ) -> Result<()> {
        let mut guard = self.channels.write().await;
        if let Some(existing) = guard.iter_mut().find(|(uid, _)| uid == user_id) {
            existing.1 = channels.clone();
        } else {
            guard.push((user_id.clone(), channels.clone()));
        }
        Ok(())
    }
}

// ── In-memory MetaSpecRepository ─────────────────────────────────────────────

#[derive(Default)]
pub struct MemMetaSpecRepository {
    store: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpec>>>,
    versions: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecVersion>>>,
    /// Binding rows, shared with `MemMetaSpecBindingRepository` when both are
    /// wired through `with_binding_store`/`with_store`. Mirrors the
    /// `mem_policy_store` pattern: `delete` must enforce the port contract
    /// ("Returns an error if bindings reference it") against the same rows
    /// the binding repo writes — separate default stores would make the
    /// guard blind to production bindings. In DB mode both ports are
    /// implemented by one storage struct, so only the mem mode needs this.
    bindings: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecBinding>>>,
}

impl MemMetaSpecRepository {
    /// Construct with a binding store shared with the binding repository
    /// (see `MemMetaSpecBindingRepository::with_store`).
    pub fn with_binding_store(
        bindings: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecBinding>>>,
    ) -> Self {
        Self {
            bindings,
            ..Default::default()
        }
    }
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
            // Archive only when the content version changed — a same-version
            // update (approval of unchanged content, required-flag toggle)
            // must not create a duplicate history row (same contract as the
            // SQLite/Postgres adapters).
            if existing.version != meta_spec.version {
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
        }
        store.retain(|m| m.id != meta_spec.id);
        store.push(meta_spec.clone());
        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        // Port contract (MetaSpecRepository::delete): "Returns an error if
        // bindings reference it." The SQLite/Postgres adapters enforce this
        // with a foreign-key count; the mem adapter guards in code against
        // the shared binding store. Error text mirrors the SQLite adapter.
        let referencing = self
            .bindings
            .read()
            .await
            .iter()
            .filter(|b| &b.meta_spec_id == id)
            .count();
        if referencing > 0 {
            let id_str = id.as_str();
            anyhow::bail!(
                "cannot delete meta_spec '{id_str}': {referencing} binding(s) reference it"
            );
        }
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
        _user_id: &str,
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

impl MemMetaSpecBindingRepository {
    /// Construct with an externally owned store so rows written here are
    /// visible to a `MemMetaSpecRepository` sharing the same Arc (pair via
    /// `MemMetaSpecRepository::with_binding_store`).
    pub fn with_store(store: Arc<tokio::sync::RwLock<Vec<gyre_domain::MetaSpecBinding>>>) -> Self {
        Self { store }
    }
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

    async fn list_all(&self) -> Result<Vec<gyre_domain::MetaSpecBinding>> {
        Ok(self.store.read().await.clone())
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

/// Port-contract test for the mem meta-spec delete guard: `delete` must
/// fail when bindings reference the meta-spec (mirrors the SQLite adapter's
/// `delete_with_binding_fails`), and the guard must see bindings written
/// through the shared binding store — the wiring `lib.rs` production state
/// and `test_state` both use.
#[cfg(test)]
mod meta_spec_binding_contract_tests {
    use super::*;
    use gyre_ports::{MetaSpecBindingRepository, MetaSpecRepository};

    fn sample_meta_spec(id: &str) -> gyre_domain::MetaSpec {
        gyre_domain::MetaSpec {
            id: Id::new(id),
            kind: gyre_domain::meta_spec::MetaSpecKind::Standard,
            name: "mem-guard-std".to_string(),
            scope: gyre_domain::meta_spec::MetaSpecScope::Global,
            scope_id: None,
            prompt: "You are a diligent worker.".to_string(),
            version: 1,
            content_hash: "hash-v1".to_string(),
            required: false,
            approval_status: gyre_domain::meta_spec::MetaSpecApprovalStatus::Approved,
            approved_by: None,
            approved_content_hash: None,
            approved_at: None,
            created_by: "system".to_string(),
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[tokio::test]
    async fn delete_fails_when_binding_references_meta_spec() {
        let binding_store = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let ms_repo = MemMetaSpecRepository::with_binding_store(Arc::clone(&binding_store));
        let binding_repo = MemMetaSpecBindingRepository::with_store(binding_store);

        let ms = sample_meta_spec("ms-guarded");
        ms_repo.create(&ms).await.unwrap();

        // No bindings yet → delete succeeds.
        ms_repo.delete(&ms.id).await.unwrap();
        assert!(
            ms_repo.get_by_id(&ms.id).await.unwrap().is_none(),
            "delete must remove the meta-spec when no bindings reference it"
        );

        // Re-create, bind, then delete → error, row retained.
        ms_repo.create(&ms).await.unwrap();
        binding_repo
            .create(&gyre_domain::MetaSpecBinding {
                id: Id::new("bind-1"),
                spec_id: "system/guard.md".to_string(),
                meta_spec_id: ms.id.clone(),
                pinned_version: 1,
                created_at: 1000,
            })
            .await
            .unwrap();
        let err = ms_repo.delete(&ms.id).await;
        assert!(
            err.is_err(),
            "delete must fail while a binding references the meta-spec (port contract)"
        );
        assert!(
            err.unwrap_err().to_string().contains("binding"),
            "error must name the binding constraint"
        );
        assert!(
            ms_repo.get_by_id(&ms.id).await.unwrap().is_some(),
            "failed delete must retain the meta-spec row"
        );
    }
}

// ── In-memory ChainAttestationRepository ────────────────────────────────────

#[derive(Default)]
pub struct MemChainAttestationRepository {
    store: Arc<Mutex<Vec<gyre_common::Attestation>>>,
}

#[async_trait]
impl gyre_ports::ChainAttestationRepository for MemChainAttestationRepository {
    async fn save(&self, attestation: &gyre_common::Attestation) -> Result<()> {
        let mut store = self.store.lock().await;
        store.retain(|a| a.id != attestation.id);
        store.push(attestation.clone());
        Ok(())
    }

    async fn find_by_id(&self, id: &str) -> Result<Option<gyre_common::Attestation>> {
        Ok(self.store.lock().await.iter().find(|a| a.id == id).cloned())
    }

    async fn load_chain(&self, leaf_id: &str) -> Result<Vec<gyre_common::Attestation>> {
        let store = self.store.lock().await;
        let mut chain = Vec::new();
        let mut current_id = Some(leaf_id.to_string());
        while let Some(id) = current_id.take() {
            if let Some(att) = store.iter().find(|a| a.id == id) {
                current_id = match &att.input {
                    gyre_common::AttestationInput::Derived(d) => Some(hex::encode(&d.parent_ref)),
                    _ => None,
                };
                chain.push(att.clone());
            }
        }
        chain.reverse(); // root to leaf
        Ok(chain)
    }

    async fn find_by_task(&self, task_id: &str) -> Result<Vec<gyre_common::Attestation>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|a| a.metadata.task_id == task_id)
            .cloned()
            .collect())
    }

    async fn find_by_commit(&self, commit_sha: &str) -> Result<Option<gyre_common::Attestation>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|a| a.output.commit_sha == commit_sha)
            .cloned())
    }

    async fn find_by_repo(
        &self,
        repo_id: &str,
        since: u64,
        until: u64,
    ) -> Result<Vec<gyre_common::Attestation>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|a| {
                a.metadata.repo_id == repo_id
                    && a.metadata.created_at >= since
                    && a.metadata.created_at <= until
            })
            .cloned()
            .collect())
    }
}

// ── In-memory KeyBindingRepository ──────────────────────────────────────────

#[derive(Default)]
pub struct MemKeyBindingRepository {
    store: Arc<Mutex<Vec<(String, gyre_common::KeyBinding, bool)>>>, // (tenant_id, binding, invalidated)
}

#[async_trait]
impl gyre_ports::KeyBindingRepository for MemKeyBindingRepository {
    async fn store(&self, tenant_id: &str, binding: &gyre_common::KeyBinding) -> Result<()> {
        self.store
            .lock()
            .await
            .push((tenant_id.to_string(), binding.clone(), false));
        Ok(())
    }

    async fn find_by_public_key(
        &self,
        tenant_id: &str,
        public_key: &[u8],
    ) -> Result<Option<gyre_common::KeyBinding>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|(tid, kb, inv)| tid == tenant_id && kb.public_key == public_key && !inv)
            .map(|(_, kb, _)| kb.clone()))
    }

    async fn find_active_by_identity(
        &self,
        tenant_id: &str,
        user_identity: &str,
    ) -> Result<Vec<gyre_common::KeyBinding>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|(tid, kb, inv)| {
                tid == tenant_id && kb.user_identity == user_identity && !inv && !kb.is_expired(now)
            })
            .map(|(_, kb, _)| kb.clone())
            .collect())
    }

    async fn invalidate(&self, tenant_id: &str, public_key: &[u8]) -> Result<()> {
        for entry in self.store.lock().await.iter_mut() {
            if entry.0 == tenant_id && entry.1.public_key == public_key {
                entry.2 = true;
            }
        }
        Ok(())
    }

    async fn invalidate_all_for_identity(
        &self,
        tenant_id: &str,
        user_identity: &str,
    ) -> Result<()> {
        for entry in self.store.lock().await.iter_mut() {
            if entry.0 == tenant_id && entry.1.user_identity == user_identity {
                entry.2 = true;
            }
        }
        Ok(())
    }
}

// ── In-memory TrustAnchorRepository ─────────────────────────────────────────

#[derive(Default)]
pub struct MemTrustAnchorRepository {
    store: Arc<Mutex<Vec<(String, gyre_common::TrustAnchor)>>>, // (tenant_id, anchor)
}

#[async_trait]
impl gyre_ports::TrustAnchorRepository for MemTrustAnchorRepository {
    async fn create(&self, tenant_id: &str, anchor: &gyre_common::TrustAnchor) -> Result<()> {
        self.store
            .lock()
            .await
            .push((tenant_id.to_string(), anchor.clone()));
        Ok(())
    }

    async fn find_by_id(
        &self,
        tenant_id: &str,
        anchor_id: &str,
    ) -> Result<Option<gyre_common::TrustAnchor>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|(tid, a)| tid == tenant_id && a.id == anchor_id)
            .map(|(_, a)| a.clone()))
    }

    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<gyre_common::TrustAnchor>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .filter(|(tid, _)| tid == tenant_id)
            .map(|(_, a)| a.clone())
            .collect())
    }

    async fn update(&self, tenant_id: &str, anchor: &gyre_common::TrustAnchor) -> Result<()> {
        let mut store = self.store.lock().await;
        if let Some(entry) = store
            .iter_mut()
            .find(|(tid, a)| tid == tenant_id && a.id == anchor.id)
        {
            entry.1 = anchor.clone();
        }
        Ok(())
    }

    async fn delete(&self, tenant_id: &str, anchor_id: &str) -> Result<()> {
        self.store
            .lock()
            .await
            .retain(|(tid, a)| !(tid == tenant_id && a.id == anchor_id));
        Ok(())
    }
}

// ── In-memory CommitSignatureRepository (identity-security.md §Layer 3) ──────

/// In-memory `CommitSignatureRepository` for pure-mem mode and tests.
/// Mirrors the SQLite adapter's `(repo_id, commit_sha)` keying.
#[derive(Default)]
pub struct MemCommitSignatureRepository {
    store: Arc<Mutex<Vec<gyre_ports::commit_signature_repo::CommitSignature>>>,
}

#[async_trait]
impl gyre_ports::commit_signature_repo::CommitSignatureRepository for MemCommitSignatureRepository {
    async fn save(
        &self,
        record: &gyre_ports::commit_signature_repo::CommitSignature,
    ) -> Result<()> {
        let mut store = self.store.lock().await;
        match store
            .iter_mut()
            .find(|r| r.repo_id == record.repo_id && r.commit_sha == record.commit_sha)
        {
            Some(existing) => *existing = record.clone(),
            None => store.push(record.clone()),
        }
        Ok(())
    }

    async fn find(
        &self,
        repo_id: &str,
        commit_sha: &str,
    ) -> Result<Option<gyre_ports::commit_signature_repo::CommitSignature>> {
        Ok(self
            .store
            .lock()
            .await
            .iter()
            .find(|r| r.repo_id == repo_id && r.commit_sha == commit_sha)
            .cloned())
    }
}

#[cfg(test)]
mod budget_call_tests {
    use super::*;
    use gyre_ports::BudgetCallRepository;

    fn record(id: &str) -> gyre_domain::BudgetCallRecord {
        gyre_domain::BudgetCallRecord {
            id: Id::new(id),
            tenant_id: Id::new("tenant-1"),
            workspace_id: Id::new("ws-1"),
            repo_id: None,
            agent_id: None,
            task_id: None,
            usage_type: "llm_query".to_string(),
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: 0.001,
            model: "test-model".to_string(),
            prompt_template_sha: None,
            timestamp: 1000,
        }
    }

    #[tokio::test]
    async fn save_rejects_duplicate_id() {
        // Port contract: "Insert-only; the record id is unique." The SQL
        // adapters enforce this via the PRIMARY KEY; the mem adapter must
        // reject a duplicate identically or SQLite-only suites would pass
        // while the in-memory backend silently accepts rewrites.
        let repo = MemBudgetCallRepository::default();
        repo.save(&record("dup-1")).await.unwrap();
        let err = repo.save(&record("dup-1")).await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
        // Distinct ids still append.
        repo.save(&record("dup-2")).await.unwrap();
        let listed = repo.list_by_workspace("ws-1", 0, 10).await.unwrap();
        assert_eq!(listed.len(), 2);
    }
}
#[cfg(test)]
mod secret_contract_tests {
    //! F1 (task-097): the mem adapter must enforce the SecretRepository port's
    //! duplicate-rejection contract in code — SQLite enforces it via UNIQUE
    //! constraints, so a mem-only guard is the only thing tests can catch.
    use super::*;
    use gyre_common::{Secret, SecretScope, SecretType};
    use gyre_ports::SecretRepository as _;

    fn sample_secret(
        id: &str,
        name: &str,
        scope: SecretScope,
        scope_id: &str,
        tenant: &str,
    ) -> Secret {
        Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: SecretType::Static,
            created_by: "test".to_string(),
            created_at: mem_now_secs(),
            expires_at: None,
            last_rotated_at: None,
            tenant_id: tenant.to_string(),
        }
    }

    #[tokio::test]
    async fn create_rejects_duplicate_id_same_tenant() {
        let repo = MemSecretRepository::default();
        let s = sample_secret(
            "sec-1",
            "ALPHA",
            SecretScope::Tenant,
            "tenant-a",
            "tenant-a",
        );
        repo.create(&s, b"v1").await.unwrap();

        // Same id, different name/scope — still a duplicate (SQLite PK).
        let dup = sample_secret("sec-1", "BETA", SecretScope::Repo, "repo-9", "tenant-a");
        let err = repo.create(&dup, b"v2").await;
        assert!(err.is_err(), "duplicate id in same tenant must be rejected");
    }

    #[tokio::test]
    async fn create_rejects_duplicate_scope_and_name_same_tenant() {
        let repo = MemSecretRepository::default();
        let s = sample_secret("sec-1", "ALPHA", SecretScope::Repo, "repo-1", "tenant-a");
        repo.create(&s, b"v1").await.unwrap();

        // Same (scope, scope_id, name) via a fresh id — the UNIQUE the port doc names.
        let dup = sample_secret("sec-2", "ALPHA", SecretScope::Repo, "repo-1", "tenant-a");
        let err = repo.create(&dup, b"v2").await;
        assert!(
            err.is_err(),
            "duplicate (scope, scope_id, name) in same tenant must be rejected"
        );
    }

    #[tokio::test]
    async fn create_allows_same_name_different_tenant() {
        let repo = MemSecretRepository::default();
        let a = sample_secret(
            "sec-1",
            "ALPHA",
            SecretScope::Tenant,
            "tenant-a",
            "tenant-a",
        );
        let b = sample_secret(
            "sec-2",
            "ALPHA",
            SecretScope::Tenant,
            "tenant-b",
            "tenant-b",
        );
        repo.create(&a, b"v1").await.unwrap();
        repo.create(&b, b"v2").await.unwrap();

        // Both tenants must independently resolve their own value.
        let va = repo.get_value(&a.id, "tenant-a").await.unwrap();
        let vb = repo.get_value(&b.id, "tenant-b").await.unwrap();
        assert_eq!(va.as_deref(), Some(b"v1".as_slice()));
        assert_eq!(vb.as_deref(), Some(b"v2".as_slice()));
    }

    #[tokio::test]
    async fn create_allows_different_name_same_scope() {
        let repo = MemSecretRepository::default();
        let a = sample_secret("sec-1", "ALPHA", SecretScope::Workspace, "ws-1", "tenant-a");
        let b = sample_secret("sec-2", "BETA", SecretScope::Workspace, "ws-1", "tenant-a");
        repo.create(&a, b"v1").await.unwrap();
        repo.create(&b, b"v2").await.unwrap();
    }
}

#[cfg(test)]
mod trace_payload_tests {
    //! Task-087 F2 / task-097 repair: MemTraceRepository.payloads must be
    //! populated by store() exactly like the SQLite adapter builds its
    //! payload blobs -- in-memory mode previously 404'd every span payload.
    //! These mirror sqlite/trace.rs's round-trip tests plus the
    //! replacement/cascade semantics the ON DELETE CASCADE gives SQLite.
    use super::*;
    use gyre_ports::TraceRepository as _;

    fn span(span_id: &str, input: Option<&str>, output: Option<&str>) -> gyre_common::TraceSpan {
        gyre_common::TraceSpan {
            span_id: span_id.to_string(),
            parent_span_id: None,
            operation_name: "GET /api/health".to_string(),
            service_name: "gyre-server".to_string(),
            kind: gyre_common::SpanKind::Server,
            start_time: 1_000_000,
            duration_us: 2_000,
            attributes: std::collections::HashMap::new(),
            input_summary: input.map(str::to_string),
            output_summary: output.map(str::to_string),
            status: gyre_common::SpanStatus::Ok,
            graph_node_id: None,
        }
    }

    fn trace(
        mr_id: &str,
        gate_run_id: &str,
        spans: Vec<gyre_common::TraceSpan>,
    ) -> gyre_common::GateTrace {
        gyre_common::GateTrace {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            mr_id: Id::new(mr_id),
            gate_run_id: Id::new(gate_run_id),
            commit_sha: "abc123".to_string(),
            spans,
            captured_at: 1_700_000_000,
        }
    }

    #[tokio::test]
    async fn get_span_payload_roundtrip() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", Some("{}"), Some(r#"{"ok":true}"#))],
        ))
        .await
        .unwrap();

        let payload = repo
            .get_span_payload(&Id::new("gate-run-1"), "span-1")
            .await
            .unwrap()
            .expect("stored span with summaries must have a payload");
        let input = payload.input.map(|b| String::from_utf8(b).unwrap());
        let output = payload.output.map(|b| String::from_utf8(b).unwrap());
        assert_eq!(input.as_deref(), Some("{}"));
        assert_eq!(output.as_deref(), Some(r#"{"ok":true}"#));
    }

    #[tokio::test]
    async fn span_with_no_payload_returns_none() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", None, None)],
        ))
        .await
        .unwrap();

        let payload = repo
            .get_span_payload(&Id::new("gate-run-1"), "span-1")
            .await
            .unwrap();
        assert!(
            payload.is_none(),
            "both-summaries-absent span has no payload"
        );
    }

    #[tokio::test]
    async fn unknown_span_or_run_returns_none() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", Some("i"), Some("o"))],
        ))
        .await
        .unwrap();

        // Span id is only unique within a trace: a different run's id must
        // not resolve this run's payload.
        assert!(repo
            .get_span_payload(&Id::new("gate-run-2"), "span-1")
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .get_span_payload(&Id::new("gate-run-1"), "span-2")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn store_replaces_previous_run_payloads() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", Some("old-input"), Some("old-output"))],
        ))
        .await
        .unwrap();
        repo.store(&trace(
            "mr-1",
            "gate-run-2",
            vec![span("span-1", Some("new-input"), None)],
        ))
        .await
        .unwrap();

        // Old run's payload went away with the replaced trace (SQLite:
        // delete + cascade to trace_spans, then re-insert).
        assert!(repo
            .get_span_payload(&Id::new("gate-run-1"), "span-1")
            .await
            .unwrap()
            .is_none());
        let payload = repo
            .get_span_payload(&Id::new("gate-run-2"), "span-1")
            .await
            .unwrap()
            .expect("new run's span payload must resolve");
        let input = payload.input.map(|b| String::from_utf8(b).unwrap());
        let output = payload.output.map(|b| String::from_utf8(b).unwrap());
        assert_eq!(input.as_deref(), Some("new-input"));
        assert_eq!(output.as_deref(), None, "None summary stays absent");
    }

    #[tokio::test]
    async fn delete_by_mr_removes_payloads() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", Some("i"), Some("o"))],
        ))
        .await
        .unwrap();

        repo.delete_by_mr(&Id::new("mr-1")).await.unwrap();

        assert!(repo
            .get_span_payload(&Id::new("gate-run-1"), "span-1")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn promote_to_attestation_preserves_payloads() {
        let repo = MemTraceRepository::default();
        repo.store(&trace(
            "mr-1",
            "gate-run-1",
            vec![span("span-1", Some("i"), Some("o"))],
        ))
        .await
        .unwrap();
        repo.promote_to_attestation(&Id::new("mr-1")).await.unwrap();

        // Promoted trace survives delete_by_mr (permanent=1 rows survive) --
        // its payloads must survive with it, mirroring SQLite.
        repo.delete_by_mr(&Id::new("mr-1")).await.unwrap();

        let payload = repo
            .get_span_payload(&Id::new("gate-run-1"), "span-1")
            .await
            .unwrap()
            .expect("promoted trace's payloads must survive delete_by_mr");
        let input = payload.input.map(|b| String::from_utf8(b).unwrap());
        assert_eq!(input.as_deref(), Some("i"));
    }
}

#[cfg(test)]
mod team_contract_tests {
    use super::*;
    use gyre_ports::TeamRepository;

    fn team(id: &str, ws: &str, slug: &str) -> Team {
        Team::new(
            Id::new(id),
            Id::new(ws),
            "Platform",
            slug,
            Id::new("u-lead"),
            1000,
        )
    }

    /// Mirrors sqlite's team contract tests: the mem adapter must enforce
    /// the port's duplicate constraints in code (it has no schema to lean
    /// on) — duplicate slug within a workspace on create and update, and
    /// duplicate team membership.
    #[tokio::test]
    async fn mem_team_enforces_duplicate_contracts() {
        let repo = MemTeamRepository::default();

        repo.create(&team("t-1", "ws-1", "platform")).await.unwrap();
        let err = repo
            .create(&team("t-2", "ws-1", "platform"))
            .await
            .expect_err("duplicate slug in workspace must fail");
        assert!(format!("{err:#}").contains("team slug already exists"));
        // Same slug in another workspace is fine.
        repo.create(&team("t-3", "ws-2", "platform")).await.unwrap();

        repo.create(&team("t-4", "ws-1", "sre")).await.unwrap();
        let mut t4 = repo.find_by_id(&Id::new("t-4")).await.unwrap().unwrap();
        t4.slug = "platform".to_string();
        let err = repo
            .update(&t4)
            .await
            .expect_err("renaming onto another team's slug must fail");
        assert!(format!("{err:#}").contains("team slug already exists"));

        repo.add_member(&TeamMembership::new(Id::new("t-1"), Id::new("u-1"), 2000))
            .await
            .unwrap();
        let err = repo
            .add_member(&TeamMembership::new(Id::new("t-1"), Id::new("u-1"), 3000))
            .await
            .expect_err("duplicate membership must fail");
        assert!(format!("{err:#}").contains("already a member"));

        // find_by_slug / list_for_user / delete-with-memberships.
        assert_eq!(
            repo.find_by_slug(&Id::new("ws-1"), "platform")
                .await
                .unwrap()
                .unwrap()
                .id
                .as_str(),
            "t-1"
        );
        let teams = repo
            .list_for_user(&Id::new("ws-1"), &Id::new("u-1"))
            .await
            .unwrap();
        assert_eq!(teams.len(), 1);
        repo.delete(&Id::new("t-1")).await.unwrap();
        assert_eq!(repo.list_members(&Id::new("t-1")).await.unwrap().len(), 0);
    }
}
