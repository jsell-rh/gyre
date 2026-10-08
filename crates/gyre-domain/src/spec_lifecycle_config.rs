//! Per-repo spec lifecycle configuration — spec-lifecycle.md §Configuration.
//!
//! Controls which spec paths the post-receive lifecycle hook watches, which
//! it ignores, and the priorities of the tasks it auto-creates. Stored
//! per-repo; absent rows mean "defaults".

use serde::{Deserialize, Serialize};

use crate::task::TaskPriority;

/// Default watched path prefixes (spec-lifecycle.md §Configuration:
/// only `specs/system/` and `specs/development/` are implementation
/// contracts; milestones/prior-art/personas/prompts are informational).
pub const DEFAULT_WATCHED_PATHS: &[&str] = &["specs/system/", "specs/development/"];

/// Default ignored path prefixes.
pub const DEFAULT_IGNORED_PATHS: &[&str] = &[
    "specs/milestones/",
    "specs/prior-art/",
    "specs/personas/",
    "specs/prompts/",
];

/// Per-repo spec lifecycle configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecLifecycleConfig {
    /// Master switch: when false, the lifecycle hook skips processing entirely.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Path prefixes that trigger lifecycle task creation.
    #[serde(default = "default_watched_paths")]
    pub watched_paths: Vec<String>,
    /// Path prefixes excluded from task creation even when watched
    /// (takes precedence over `watched_paths`).
    #[serde(default = "default_ignored_paths")]
    pub ignored_paths: Vec<String>,
    /// Auto-invoke stale spec approvals when a watched spec changes.
    #[serde(default = "default_true")]
    pub auto_invalidate_approvals: bool,
    /// Skip task creation when a non-Done task already covers the change.
    #[serde(default = "default_true")]
    pub dedup_open_tasks: bool,
    /// Priority for tasks created on new specs.
    #[serde(default = "default_priority_new")]
    pub default_priority_new: TaskPriority,
    /// Priority for tasks created on modified specs.
    #[serde(default = "default_priority_modified")]
    pub default_priority_modified: TaskPriority,
    /// Priority for tasks created on deleted specs.
    #[serde(default = "default_priority_deleted")]
    pub default_priority_deleted: TaskPriority,
}

fn default_true() -> bool {
    true
}

fn default_watched_paths() -> Vec<String> {
    DEFAULT_WATCHED_PATHS.iter().map(|s| s.to_string()).collect()
}

fn default_ignored_paths() -> Vec<String> {
    DEFAULT_IGNORED_PATHS.iter().map(|s| s.to_string()).collect()
}

fn default_priority_new() -> TaskPriority {
    TaskPriority::Medium
}

fn default_priority_modified() -> TaskPriority {
    TaskPriority::High
}

fn default_priority_deleted() -> TaskPriority {
    TaskPriority::High
}

impl Default for SpecLifecycleConfig {
    fn default() -> Self {
        SpecLifecycleConfig {
            enabled: true,
            watched_paths: default_watched_paths(),
            ignored_paths: default_ignored_paths(),
            auto_invalidate_approvals: true,
            dedup_open_tasks: true,
            default_priority_new: TaskPriority::Medium,
            default_priority_modified: TaskPriority::High,
            default_priority_deleted: TaskPriority::High,
        }
    }
}

impl SpecLifecycleConfig {
    pub fn is_watched(&self, path: &str) -> bool {
        if self
            .ignored_paths
            .iter()
            .any(|p| path.starts_with(p.as_str()))
        {
            return false;
        }
        self.watched_paths.iter().any(|p| path.starts_with(p.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = SpecLifecycleConfig::default();
        assert!(c.enabled);
        assert_eq!(
            c.watched_paths,
            vec!["specs/system/".to_string(), "specs/development/".to_string()]
        );
        assert_eq!(
            c.ignored_paths,
            vec![
                "specs/milestones/".to_string(),
                "specs/prior-art/".to_string(),
                "specs/personas/".to_string(),
                "specs/prompts/".to_string(),
            ]
        );
        assert!(c.auto_invalidate_approvals);
        assert!(c.dedup_open_tasks);
        assert_eq!(c.default_priority_new, TaskPriority::Medium);
        assert_eq!(c.default_priority_modified, TaskPriority::High);
        assert_eq!(c.default_priority_deleted, TaskPriority::High);
    }

    #[test]
    fn watched_path_filtering() {
        let c = SpecLifecycleConfig::default();
        assert!(c.is_watched("specs/system/agent-gates.md"));
        assert!(c.is_watched("specs/development/architecture.md"));
        assert!(!c.is_watched("specs/milestones/m1.md"));
        assert!(!c.is_watched("src/main.rs"));
        assert!(!c.is_watched("specs/prompts/code-review.md"));
    }

    #[test]
    fn ignored_takes_precedence_over_watched() {
        // Watch everything under specs/ but ignore milestones.
        let c = SpecLifecycleConfig {
            watched_paths: vec!["specs/".to_string()],
            ignored_paths: vec!["specs/milestones/".to_string()],
            ..SpecLifecycleConfig::default()
        };
        assert!(c.is_watched("specs/system/x.md"));
        assert!(!c.is_watched("specs/milestones/x.md"));
    }

    #[test]
    fn deserializes_partial_json_with_defaults() {
        // Old/partial payloads must deserialize with spec defaults for
        // missing fields (serde defaults), so stored rows never lose the
        // default behavior silently.
        let c: SpecLifecycleConfig =
            serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!c.enabled);
        assert_eq!(
            c.watched_paths,
            vec!["specs/system/".to_string(), "specs/development/".to_string()]
        );
        assert_eq!(c.default_priority_new, TaskPriority::Medium);
    }

    #[test]
    fn deserializes_string_priorities() {
        // JSON payloads carry priorities as strings ("Medium"/"High"/...).
        let c: SpecLifecycleConfig = serde_json::from_str(
            r#"{"default_priority_new":"Low","default_priority_modified":"Critical"}"#,
        )
        .unwrap();
        assert_eq!(c.default_priority_new, TaskPriority::Low);
        assert_eq!(c.default_priority_modified, TaskPriority::Critical);
    }
}
