use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeResult {
    Success { merge_commit_sha: String },
    Conflict { message: String },
}

/// Result of creating a revert commit (git revert -m 1 semantics).
///
/// The revert is a three-way merge of the reverted commit's first-parent
/// tree against the current branch tip, so reverting a non-tip merge
/// undoes only that merge's changes and preserves every later merge
/// (task-095 R4-F1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RevertResult {
    Success { revert_commit_sha: String },
    /// The inverse patch conflicts with changes that landed after the
    /// reverted commit (same file modified both there and later). The
    /// branch is untouched; the caller decides how to proceed.
    Conflict { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub head_sha: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub patches: Vec<FileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub patch: Option<String>,
}
