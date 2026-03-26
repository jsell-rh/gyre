//! Conversation-to-code provenance types (HSI §5).
//!
//! These types live in `gyre-common` (shared wire types, like `Message` and `Id`).
//! The `ConversationRepository` port lives in `gyre-ports`.

use crate::Id;
use serde::{Deserialize, Serialize};

/// Links one conversation turn to the commit(s) it produced.
///
/// Stored without `conversation_sha` initially (unknown at push time).
/// Back-filled when the conversation is uploaded via `conversation.upload`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnCommitLink {
    /// Unique record ID.
    pub id: Id,
    /// Agent that produced this turn.
    pub agent_id: Id,
    /// Which conversation turn this link represents.
    pub turn_number: u32,
    /// The commit SHA produced during/after this turn.
    pub commit_sha: String,
    /// Files modified in this commit.
    pub files_changed: Vec<String>,
    /// SHA-256 of the conversation blob — NULL until back-filled at upload time.
    pub conversation_sha: Option<String>,
    /// Unix epoch seconds.
    pub timestamp: u64,
    /// Tenant scope for multi-tenant isolation.
    pub tenant_id: Id,
}

/// Full conversation provenance record: links an agent conversation to the code it produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationProvenance {
    /// Agent that had the conversation.
    pub agent_id: Id,
    /// Task the agent was working on.
    pub task_id: Id,
    /// SHA-256 of the full compressed conversation blob.
    pub conversation_sha: String,
    /// Maps conversation turns to commits.
    pub turn_index: Vec<TurnCommitLink>,
}
