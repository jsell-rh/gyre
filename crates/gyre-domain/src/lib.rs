//! Pure domain logic for the Gyre autonomous development platform.
//!
//! # Hexagonal Architecture Invariant
//!
//! This crate MUST NOT import `gyre-adapters` or any infrastructure crate
//! (databases, HTTP clients, file I/O, etc.). Domain logic depends only on:
//! - `gyre-common` for shared types and errors
//! - `gyre-ports` for repository/service trait definitions
//!
//! Violations are caught by `scripts/check-arch.sh` and CI.

pub mod agent;
pub mod project;
pub mod task;
