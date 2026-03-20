//! Port traits defining the hexagonal architecture boundaries.
//!
//! Ports are pure interfaces. Implementations live in `gyre-adapters`.
//! Domain logic depends only on these traits, never on concrete adapters.

pub mod activity;
pub mod agent;
pub mod agent_tracking;
pub mod analytics;
pub mod audit;
pub mod compute;
pub mod git_ops;
pub mod jj_ops;
pub mod merge_queue;
pub mod merge_request;
pub mod project;
pub mod repository;
pub mod review;
pub mod storage;
pub mod task;
pub mod user;

pub use activity::{ActivityQuery, ActivityRepository};
pub use agent::AgentRepository;
pub use agent_tracking::{AgentCommitRepository, WorktreeRepository};
pub use analytics::{AnalyticsRepository, CostRepository};
pub use audit::AuditRepository;
pub use compute::{ComputeTarget, ProcessHandle, SpawnConfig};
pub use git_ops::GitOpsPort;
pub use jj_ops::{JjChange, JjOpsPort};
pub use merge_queue::MergeQueueRepository;
pub use merge_request::MergeRequestRepository;
pub use project::ProjectRepository;
pub use repository::RepoRepository;
pub use review::ReviewRepository;
pub use storage::StoragePort;
pub use task::TaskRepository;
pub use user::{ApiKeyRepository, UserRepository};
