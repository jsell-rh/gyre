use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::{BranchInfo, CommitInfo, DiffResult, MergeResult};

#[async_trait]
pub trait GitOpsPort: Send + Sync {
    /// Initialize a new bare repository at the given path.
    async fn init_bare(&self, path: &str) -> Result<()>;

    /// List branches with their head commit SHA.
    async fn list_branches(&self, repo_path: &str) -> Result<Vec<BranchInfo>>;

    /// Get commit log for a branch (most recent first).
    async fn commit_log(
        &self,
        repo_path: &str,
        branch: &str,
        limit: usize,
    ) -> Result<Vec<CommitInfo>>;

    /// Get diff between two refs (commits, branches, tags).
    async fn diff(&self, repo_path: &str, from: &str, to: &str) -> Result<DiffResult>;

    /// Check if a path is a valid git repository.
    async fn is_repo(&self, path: &str) -> Result<bool>;

    /// Check if source branch can be merged into target without conflicts.
    async fn can_merge(&self, repo_path: &str, source: &str, target: &str) -> Result<bool>;

    /// Merge source branch into target branch. Returns success with merge commit SHA or conflict info.
    async fn merge_branches(
        &self,
        repo_path: &str,
        source: &str,
        target: &str,
    ) -> Result<MergeResult>;

    /// Add a git worktree at `worktree_path` checked out to `branch`.
    async fn create_worktree(
        &self,
        repo_path: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<()>;

    /// Add a git worktree at `worktree_path` checked out (detached) at `sha`.
    /// Used by post-merge validation to run gates against an exact commit
    /// (platform-model.md §6, task-095 R2-1).
    async fn create_detached_worktree(
        &self,
        repo_path: &str,
        worktree_path: &str,
        sha: &str,
    ) -> Result<()>;

    /// Remove a git worktree at `worktree_path`.
    async fn remove_worktree(&self, repo_path: &str, worktree_path: &str) -> Result<()>;

    /// Remove a git worktree at `worktree_path`, discarding uncommitted
    /// changes (the `--force` variant of `remove_worktree`).
    ///
    /// Used for throwaway workspaces — preview worktrees (meta-spec
    /// reconciliation §5) are expected to be dirty when deleted. An absent
    /// worktree path is not an error: the caller asked for it to be gone.
    async fn force_remove_worktree(&self, repo_path: &str, worktree_path: &str) -> Result<()>;

    /// List paths of all registered worktrees for the repository.
    async fn list_worktrees(&self, repo_path: &str) -> Result<Vec<String>>;

    /// Create an initial empty commit on `branch` in a freshly-initialized bare repo.
    /// Returns the commit SHA. Typically called right after `init_bare`.
    async fn create_initial_commit(&self, repo_path: &str, branch: &str) -> Result<String>;

    /// Clone a remote repository as a bare mirror into `path`.
    /// Equivalent to `git clone --mirror <url> <path>`.
    async fn clone_mirror(&self, url: &str, path: &str) -> Result<()>;

    /// Fetch all refs for a mirror repository.
    /// Equivalent to `git fetch --all` in the mirror repo directory.
    async fn fetch_mirror(&self, path: &str) -> Result<()>;

    /// Check if a branch exists in the repository.
    async fn branch_exists(&self, repo_path: &str, branch_name: &str) -> Result<bool>;

    /// Create a new branch from `from_ref` (a branch name, tag, or commit SHA).
    async fn create_branch(&self, repo_path: &str, branch_name: &str, from_ref: &str)
        -> Result<()>;

    /// Delete the local branch `branch_name`.
    ///
    /// An unknown branch is not an error (deletion is idempotent). The branch
    /// must not be checked out in a worktree — remove the worktree first.
    async fn delete_branch(&self, repo_path: &str, branch_name: &str) -> Result<()>;

    /// Write (or overwrite) a file at `file_path` on `branch`, creating a commit
    /// with the given `message`. Returns the new commit SHA.
    ///
    /// Works on bare repositories: builds a new tree from the branch tip,
    /// inserts/replaces the blob at `file_path`, and commits to `refs/heads/<branch>`.
    async fn write_file(
        &self,
        repo_path: &str,
        branch: &str,
        file_path: &str,
        content: &[u8],
        message: &str,
    ) -> Result<String>;

    /// Force-reset a branch to point at a specific commit SHA.
    ///
    /// Used for atomic group rollback: when a group member fails to merge,
    /// the target branch is reset to the pre-group HEAD to undo already-merged
    /// members.
    async fn reset_branch(&self, repo_path: &str, branch: &str, target_sha: &str) -> Result<()>;

    /// Read a file's content from the tip of a branch.
    ///
    /// Returns `Ok(None)` if the file does not exist at the given path on the
    /// branch. Returns `Ok(Some(bytes))` with the blob content if found.
    /// Works on bare repositories.
    async fn read_file(
        &self,
        repo_path: &str,
        branch: &str,
        file_path: &str,
    ) -> Result<Option<Vec<u8>>>;

    /// Create a revert commit on `branch` that undoes `sha_to_revert`.
    ///
    /// The revert commit's tree is the first parent's tree of `sha_to_revert`,
    /// and its parent is the current tip of `branch`. Returns the revert
    /// commit's SHA. Used by the post-merge recovery protocol
    /// (platform-model.md §6).
    async fn revert_commit(
        &self,
        repo_path: &str,
        branch: &str,
        sha_to_revert: &str,
    ) -> Result<String>;
}
