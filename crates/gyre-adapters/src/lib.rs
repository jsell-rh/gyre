//! Adapter implementations for the Gyre platform.
//!
//! Adapters implement the port traits defined in `gyre-ports`.
//! They are the only place where infrastructure concerns (databases, HTTP, etc.) live.
//!
//! # Hexagonal Architecture
//!
//! Adapters depend on ports, not the other way around.
//! Domain logic MUST NOT import this crate.

pub mod git2_ops;
pub mod jj_ops;
pub mod sqlite;

pub use git2_ops::Git2OpsAdapter;
pub use jj_ops::JjOpsAdapter;
pub use sqlite::SqliteStorage;
