//! Adapter implementations for the Gyre platform.
//!
//! Adapters implement the port traits defined in `gyre-ports`.
//! They are the only place where infrastructure concerns (databases, HTTP, etc.) live.
//!
//! # Hexagonal Architecture
//!
//! Adapters depend on ports, not the other way around.
//! Domain logic MUST NOT import this crate.

pub mod compute;
pub mod git2_ops;
pub mod jj_ops;
pub mod llm;
pub mod mem_graph;
pub mod mem_saved_view;
pub mod mem_search;
pub mod postgres;
pub mod schema;
pub mod sqlite;

pub use compute::{DockerTarget, LocalTarget, SshTarget};
pub use git2_ops::Git2OpsAdapter;
pub use jj_ops::JjOpsAdapter;
pub use llm::{MockLlmAdapter, MockLlmPortFactory, RigVertexAiFactory};
pub use mem_graph::MemGraphStore;
pub use mem_saved_view::MemSavedViewRepository;
pub use mem_search::MemSearchAdapter;
pub use postgres::PgStorage;
pub use sqlite::SqliteStorage;
