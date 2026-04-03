//! LLM adapter implementations.
//!
//! - `mock`: In-memory mock adapter for tests and stub mode.
//! - `rig_vertexai`: Google Vertex AI adapter via the `rig` crate.

#[cfg(test)]
mod integration_test;
pub mod mock;
pub mod rig_vertexai;

pub use mock::{MockLlmAdapter, MockLlmPortFactory};
pub use rig_vertexai::RigVertexAiFactory;
