//! Port traits defining the hexagonal architecture boundaries.
//!
//! Ports are pure interfaces. Implementations live in `gyre-adapters`.
//! Domain logic depends only on these traits, never on concrete adapters.

pub mod activity;
pub mod agent;
pub mod git_ops;
pub mod merge_request;
pub mod project;
pub mod repository;
pub mod review;
pub mod storage;
pub mod task;

pub use activity::{ActivityQuery, ActivityRepository};
pub use agent::AgentRepository;
pub use git_ops::GitOpsPort;
pub use merge_request::MergeRequestRepository;
pub use project::ProjectRepository;
pub use repository::RepoRepository;
pub use review::ReviewRepository;
pub use storage::StoragePort;
pub use task::TaskRepository;
