//! Port traits defining the hexagonal architecture boundaries.
//!
//! Ports are pure interfaces. Implementations live in `gyre-adapters`.
//! Domain logic depends only on these traits, never on concrete adapters.

pub mod storage;
