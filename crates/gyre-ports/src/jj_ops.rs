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
    async fn jj_squash(&self, repo_path: &str) -> Result<()>;

    /// Create a bookmark (branch) pointing to a specific change.
    async fn jj_bookmark_create(&self, repo_path: &str, name: &str, change_id: &str) -> Result<()>;

    /// Undo the last jj operation.
    async fn jj_undo(&self, repo_path: &str) -> Result<()>;
}
