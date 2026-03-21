use serde::{Deserialize, Serialize};

/// Domain events broadcast over the event bus to all connected WebSocket clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum DomainEvent {
    AgentCreated {
        id: String,
    },
    AgentStatusChanged {
        id: String,
        status: String,
    },
    TaskCreated {
        id: String,
    },
    TaskTransitioned {
        id: String,
        status: String,
    },
    MrCreated {
        id: String,
    },
    MrStatusChanged {
        id: String,
        status: String,
    },
    ActivityRecorded {
        id: String,
        event_type: String,
    },
    QueueUpdated,
    DataSeeded,
    /// Emitted when a git push is rejected by pre-accept gates (M13.1).
    PushRejected {
        repo_id: String,
        branch: String,
        agent_id: String,
        reason: String,
    },
    /// Emitted when a git push is accepted (M13.3).
    PushAccepted {
        repo_id: String,
        branch: String,
        agent_id: String,
        commit_count: usize,
        task_id: Option<String>,
        ralph_step: Option<String>,
    },
    /// Emitted when a speculative merge detects a conflict between branches (M13.5).
    SpeculativeConflict {
        repo_id: String,
        branch: String,
        conflicting_files: Vec<String>,
    },
    /// Emitted when a speculative merge is clean (no conflicts) (M13.5).
    SpeculativeMergeClean {
        repo_id: String,
        branch: String,
    },
    /// Emitted when hot-files list changes (M13.4).
    HotFilesChanged {
        repo_id: String,
    },
    /// Emitted when a spec file changes and a lifecycle task is auto-created.
    SpecChanged {
        repo_id: String,
        spec_path: String,
        change_kind: String,
        task_id: String,
    },
    /// Emitted when a quality gate fails on an MR (M12.3).
    /// Allows the MR's author agent to react in the same Ralph loop cycle.
    GateFailure {
        mr_id: String,
        gate_name: String,
        gate_type: String,
        status: String,
        output: String,
        spec_ref: Option<String>,
        gate_agent_id: String,
    },
    /// Emitted when an MR references a spec_ref SHA that is not the current HEAD blob
    /// for that spec file (warn_stale_spec policy). The merge is not blocked.
    StaleSpecWarning {
        mr_id: String,
        repo_id: String,
        spec_path: String,
        spec_sha: String,
        current_sha: String,
    },
}
