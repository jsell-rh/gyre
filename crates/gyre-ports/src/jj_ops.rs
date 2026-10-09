use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A single jj change (analogous to a git commit, but mutable until bookmarked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JjChange {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    /// Unix epoch seconds (0 if not available).
    pub timestamp: u64,
    pub bookmarks: Vec<String>,
}

/// Port for jj (Jujutsu) VCS operations.
///
/// jj operates in colocated mode: both `.jj/` and `.git/` exist in the same directory.
/// All methods accept a `repo_path` pointing to the working directory.
/// Outcome of an automatic rebase of an agent's working change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JjRebaseOutcome {
    /// Rebase completed (possibly a no-op — "Nothing changed").
    Success { rebased_count: usize },
    /// Rebase completed with conflicts materialized as jj conflict state.
    /// `files` lists the conflicted paths.
    Conflict { rebased_count: usize, files: Vec<String> },
}

#[async_trait]
pub trait JjOpsPort: Send + Sync {
    /// Initialize jj in an existing git repo (colocated mode).
    async fn jj_init(&self, repo_path: &str) -> Result<()>;

    /// Create a new jj change (anonymous WIP commit). Returns the new change ID.
    async fn jj_new(&self, repo_path: &str, description: &str) -> Result<String>;

    /// Update the description of an existing change.
    async fn jj_describe(&self, repo_path: &str, change_id: &str, description: &str) -> Result<()>;

    /// List recent changes (operation log), most recent first.
    async fn jj_log(&self, repo_path: &str, limit: usize) -> Result<Vec<JjChange>>;

    /// Squash the working copy into its parent change.
    ///
    /// Returns the commit SHA of the resulting (parent) commit after squash.
    async fn jj_squash(&self, repo_path: &str) -> Result<String>;

    /// Create a bookmark (branch) pointing to a specific change.
    async fn jj_bookmark_create(&self, repo_path: &str, name: &str, change_id: &str) -> Result<()>;

    /// Undo the last jj operation.
    async fn jj_undo(&self, repo_path: &str) -> Result<()>;

    /// Rebase the branch containing `revision` (the whole in-flight stack,
    /// per source-control.md §4: "the agent's in-progress work") onto
    /// `destination`. Runs `jj rebase -b <revision> -d <destination>`.
    ///
    /// Conflicts are surfaced as state, not errors: a conflicted rebase
    /// returns `JjRebaseOutcome::Conflict` with the affected file list
    /// (jj's conflict-as-state model — the agent keeps working on
    /// non-conflicting files).
    async fn jj_rebase(
        &self,
        workspace_path: &str,
        revision: &str,
        destination: &str,
    ) -> Result<JjRebaseOutcome>;

    /// Ensure the repo's shared jj checkout exists at `main_checkout_path`,
    /// backed by the git repo at `git_repo_path` (absolute paths required —
    /// the checkout's store records the git path at creation time).
    ///
    /// Idempotent: succeeds without re-initializing when the checkout already
    /// contains a `.jj` directory.
    async fn jj_main_checkout_init(
        &self,
        main_checkout_path: &str,
        git_repo_path: &str,
    ) -> Result<()>;

    /// Create an agent workspace named `name` at `workspace_path`, a working
    /// copy on top of `revision`, registered in the shared checkout at
    /// `main_checkout_path`.
    ///
    /// Imports the git repo's refs first (branches created after the checkout
    /// was initialized must be visible before resolving `revision` by name),
    /// and cleans up (forgets) a half-created workspace when creation fails,
    /// so the caller's fallback paths stay reachable.
    async fn jj_workspace_add(
        &self,
        main_checkout_path: &str,
        workspace_path: &str,
        name: &str,
        revision: &str,
        description: &str,
    ) -> Result<()>;

    /// Forget the named workspace from the shared checkout. Idempotent:
    /// succeeds when the workspace is already absent.
    async fn jj_workspace_forget(
        &self,
        main_checkout_path: &str,
        name: &str,
    ) -> Result<()>;

    /// Export bookmarks from the shared jj repo into its backing git repo.
    /// Makes agent work visible to git-backed consumers (diff, can_merge,
    /// merge). Idempotent.
    async fn jj_git_export(&self, workspace_path: &str) -> Result<()>;
}
