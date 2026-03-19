//! Pure domain logic for the Gyre autonomous development platform.
//!
//! # Hexagonal Architecture Invariant
//!
//! This crate MUST NOT import `gyre-adapters` or any infrastructure crate
//! (databases, HTTP clients, file I/O, etc.). Domain logic depends only on:
//! - `gyre-common` for shared types and errors
//!
//! Violations are caught by `scripts/check-arch.sh` and CI.

pub mod activity;
pub mod agent;
pub mod agent_tracking;
pub mod git_types;
pub mod merge_queue;
pub mod merge_request;
pub mod project;
pub mod repository;
pub mod review;
pub mod task;
pub mod user;

pub use activity::ActivityEvent;
pub use agent::{Agent, AgentError, AgentStatus};
pub use agent_tracking::{AgentCommit, AgentWorktree};
pub use git_types::{BranchInfo, CommitInfo, DiffResult, FileDiff, MergeResult};
pub use merge_queue::{MergeQueueEntry, MergeQueueEntryStatus};
pub use merge_request::{DiffStats, MergeRequest, MrError, MrStatus};
pub use project::Project;
pub use repository::Repository;
pub use review::{Review, ReviewComment, ReviewDecision};
pub use task::{Task, TaskError, TaskPriority, TaskStatus};
pub use user::{User, UserRole};
