use crate::agent_tracking::LoopConfig;
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: AgentStatus, to: AgentStatus },
}

/// How an agent should behave when it detects the Gyre server is unreachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectedBehavior {
    /// Stop accepting new work and wait for reconnection (default).
    #[default]
    Pause,
    /// Continue working locally (git ops, local state) until reconnected.
    ContinueOffline,
    /// Abort immediately: mark self Dead, clean worktrees.
    Abort,
}

/// Agent status enum per agent-runtime.md §1 Phase 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// Agent is executing work.
    Active,
    /// Agent completed successfully.
    Idle,
    /// Max iterations reached, spawn failure, or non-recoverable error.
    Failed,
    /// Manually stopped, cascaded shutdown, or paused due to disconnection.
    Stopped,
    /// Detected by stale agent detector (heartbeat expired).
    Dead,
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AgentStatus::Active => "Active",
            AgentStatus::Idle => "Idle",
            AgentStatus::Failed => "Failed",
            AgentStatus::Stopped => "Stopped",
            AgentStatus::Dead => "Dead",
        };
        write!(f, "{s}")
    }
}

/// Token and cost usage reported by an agent for a work session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUsage {
    pub agent_id: Id,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub cost_usd: f64,
    /// Unix epoch seconds when this usage was reported.
    pub reported_at: u64,
}

/// Reference to a meta-spec that was consulted during an agent's work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaSpecUsed {
    pub id: Id,
    pub kind: String,
    pub content_hash: String,
    pub version: u32,
    pub required: bool,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Id,
    pub name: String,
    pub status: AgentStatus,
    pub parent_id: Option<Id>,
    pub current_task_id: Option<Id>,
    pub lifetime_budget_secs: Option<u64>,
    pub spawned_at: u64,
    pub last_heartbeat: Option<u64>,
    /// Identity of the agent or user who spawned this agent (M13.2).
    pub spawned_by: Option<String>,
    /// How the agent should behave when the server is unreachable (BCP graceful degradation).
    #[serde(default)]
    pub disconnected_behavior: DisconnectedBehavior,
    /// Workspace that governs this agent (ABAC boundary). Non-optional per M34 hierarchy enforcement.
    pub workspace_id: Id,
    /// Current session iteration count for the Ralph loop.
    #[serde(default)]
    pub iteration: u32,
    /// Ralph loop configuration (when present, server manages session cycle).
    pub loop_config: Option<LoopConfig>,
}

impl Agent {
    pub fn new(id: Id, name: impl Into<String>, spawned_at: u64) -> Self {
        Self {
            id,
            name: name.into(),
            status: AgentStatus::Idle,
            parent_id: None,
            current_task_id: None,
            lifetime_budget_secs: None,
            spawned_at,
            last_heartbeat: None,
            spawned_by: None,
            disconnected_behavior: DisconnectedBehavior::default(),
            workspace_id: Id::new("default"),
            iteration: 0,
            loop_config: None,
        }
    }

    /// Returns true if the agent has sent a heartbeat within `timeout_secs`.
    pub fn is_alive(&self, now: u64, timeout_secs: u64) -> bool {
        if self.status == AgentStatus::Dead {
            return false;
        }
        let last = self.last_heartbeat.unwrap_or(self.spawned_at);
        now.saturating_sub(last) <= timeout_secs
    }

    pub fn heartbeat(&mut self, now: u64) {
        self.last_heartbeat = Some(now);
    }

    pub fn assign_task(&mut self, task_id: Id) {
        self.current_task_id = Some(task_id);
    }

    /// Enforce valid status transitions per agent-runtime.md §1.
    pub fn transition_status(&mut self, new_status: AgentStatus) -> Result<(), AgentError> {
        let valid = matches!(
            (&self.status, &new_status),
            (AgentStatus::Idle, AgentStatus::Active)
                | (AgentStatus::Active, AgentStatus::Idle)
                | (AgentStatus::Active, AgentStatus::Failed)
                | (AgentStatus::Active, AgentStatus::Stopped)
                // Terminal transitions: any state can reach Dead, Failed, Stopped.
                | (_, AgentStatus::Dead)
                | (_, AgentStatus::Failed)
                | (_, AgentStatus::Stopped)
        );
        if valid {
            self.status = new_status;
            Ok(())
        } else {
            Err(AgentError::InvalidTransition {
                from: self.status.clone(),
                to: new_status,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent() -> Agent {
        Agent::new(Id::new("a1"), "test-agent", 1000)
    }

    #[test]
    fn test_new_agent_is_idle() {
        let agent = make_agent();
        assert_eq!(agent.status, AgentStatus::Idle);
        assert!(agent.last_heartbeat.is_none());
    }

    #[test]
    fn test_heartbeat_updates() {
        let mut agent = make_agent();
        agent.heartbeat(2000);
        assert_eq!(agent.last_heartbeat, Some(2000));
    }

    #[test]
    fn test_is_alive_within_timeout() {
        let mut agent = make_agent();
        agent.heartbeat(1000);
        assert!(agent.is_alive(1060, 60));
    }

    #[test]
    fn test_is_alive_past_timeout() {
        let mut agent = make_agent();
        agent.heartbeat(1000);
        assert!(!agent.is_alive(2000, 60));
    }

    #[test]
    fn test_dead_agent_not_alive() {
        let mut agent = make_agent();
        agent.status = AgentStatus::Dead;
        assert!(!agent.is_alive(1001, 60));
    }

    #[test]
    fn test_valid_transition_idle_to_active() {
        let mut agent = make_agent();
        assert!(agent.transition_status(AgentStatus::Active).is_ok());
        assert_eq!(agent.status, AgentStatus::Active);
    }

    #[test]
    fn test_any_to_dead() {
        let mut agent = make_agent();
        assert!(agent.transition_status(AgentStatus::Dead).is_ok());
    }

    #[test]
    fn test_active_to_failed() {
        let mut agent = make_agent();
        agent.transition_status(AgentStatus::Active).unwrap();
        assert!(agent.transition_status(AgentStatus::Failed).is_ok());
    }

    #[test]
    fn test_active_to_stopped() {
        let mut agent = make_agent();
        agent.transition_status(AgentStatus::Active).unwrap();
        assert!(agent.transition_status(AgentStatus::Stopped).is_ok());
    }

    #[test]
    fn test_assign_task() {
        let mut agent = make_agent();
        agent.assign_task(Id::new("task-1"));
        assert_eq!(agent.current_task_id, Some(Id::new("task-1")));
    }
}
