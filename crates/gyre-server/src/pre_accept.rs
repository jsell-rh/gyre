//! Built-in pre-accept push gates.
//!
//! Each gate implements [`gyre_ports::PreAcceptGate`] and is registered by
//! name in `AppState::push_gate_registry` at server startup.
//!
//! Available built-in gates:
//! - `conventional-commit` — reject commits that don't match `type(scope): description`
//! - `task-ref`            — reject feat/ and fix/ branches without a TASK-{id} reference
//! - `no-em-dash`          — reject commits whose message contains an em-dash (—)

use gyre_ports::{GateOutcome, PreAcceptGate, PushContext};

// ---------------------------------------------------------------------------
// ConventionalCommitGate
// ---------------------------------------------------------------------------

/// Validates that every commit message in the push follows the Conventional
/// Commits specification: `type(scope): description` or `type: description`.
///
/// Allowed types: feat, fix, docs, style, refactor, perf, test, build, ci,
/// chore, revert.
pub struct ConventionalCommitGate;

/// Return `true` if `msg` (first line only) matches the conventional-commit
/// pattern.
fn is_conventional(msg: &str) -> bool {
    let first_line = msg.lines().next().unwrap_or("").trim();
    // pattern: <type>[(scope)]: <description>
    // We require at least one character after the colon+space.
    let types = [
        "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
        "revert",
    ];

    let (type_part, rest) = match first_line.split_once(':') {
        Some(pair) => pair,
        None => return false,
    };

    // type_part is e.g. "feat" or "feat(scope)"
    let base_type = if let Some(paren) = type_part.find('(') {
        let close = match type_part.find(')') {
            Some(c) => c,
            None => return false, // unclosed paren
        };
        if close < paren {
            return false;
        }
        &type_part[..paren]
    } else {
        type_part
    };

    if !types.contains(&base_type) {
        return false;
    }

    // description must be non-empty (after ": ")
    let description = rest.trim_start_matches(' ');
    !description.is_empty()
}

impl PreAcceptGate for ConventionalCommitGate {
    fn name(&self) -> &str {
        "conventional-commit"
    }

    fn check(&self, ctx: &PushContext) -> GateOutcome {
        let bad: Vec<&str> = ctx
            .commit_messages
            .iter()
            .filter(|m| !is_conventional(m))
            .map(|m| m.as_str())
            .collect();

        if bad.is_empty() {
            GateOutcome::Passed
        } else {
            let sample = bad[0].lines().next().unwrap_or("").trim();
            let sample = if sample.len() > 72 {
                format!("{}...", &sample[..72])
            } else {
                sample.to_string()
            };
            GateOutcome::Failed(format!(
                "conventional-commit: {} commit(s) have non-conventional messages. \
                 First offender: \"{sample}\". \
                 Expected format: type(scope): description  \
                 (types: feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)",
                bad.len()
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// TaskRefGate
// ---------------------------------------------------------------------------

/// Requires that any push to a `feat/` or `fix/` branch references a known
/// task ID (`TASK-{digits}`) in at least one commit message.
pub struct TaskRefGate;

/// Return `true` if any message contains `TASK-` followed by at least one ASCII digit.
fn has_task_ref(messages: &[String]) -> bool {
    messages.iter().any(|m| {
        // Find each occurrence of "TASK-" and check that the next char is a digit.
        let mut search = m.as_str();
        while let Some(pos) = search.find("TASK-") {
            let after = &search[pos + 5..];
            if after.starts_with(|c: char| c.is_ascii_digit()) {
                return true;
            }
            search = &search[pos + 5..];
        }
        false
    })
}

impl PreAcceptGate for TaskRefGate {
    fn name(&self) -> &str {
        "task-ref"
    }

    fn check(&self, ctx: &PushContext) -> GateOutcome {
        let needs_ref = ctx.branch.starts_with("feat/") || ctx.branch.starts_with("fix/");
        if !needs_ref {
            return GateOutcome::Passed;
        }

        if ctx.commit_messages.is_empty() || has_task_ref(&ctx.commit_messages) {
            GateOutcome::Passed
        } else {
            GateOutcome::Failed(format!(
                "task-ref: branch '{}' is a feat/ or fix/ branch but no commit message \
                 references a task ID (TASK-{{id}}). Add a TASK-{{id}} reference to at \
                 least one commit message.",
                ctx.branch
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// NoEmDashGate
// ---------------------------------------------------------------------------

/// Rejects any commit whose message contains an em-dash character (U+2014 —).
/// Em-dashes are a known footgun: they often appear when copy-pasting from
/// word processors and cause downstream tooling issues.
pub struct NoEmDashGate;

const EM_DASH: char = '\u{2014}';

impl PreAcceptGate for NoEmDashGate {
    fn name(&self) -> &str {
        "no-em-dash"
    }

    fn check(&self, ctx: &PushContext) -> GateOutcome {
        let bad: Vec<&str> = ctx
            .commit_messages
            .iter()
            .filter(|m| m.contains(EM_DASH))
            .map(|m| m.as_str())
            .collect();

        if bad.is_empty() {
            GateOutcome::Passed
        } else {
            GateOutcome::Failed(format!(
                "no-em-dash: {} commit message(s) contain an em-dash character (—). \
                 Replace em-dashes with hyphens (-) or double-hyphens (--).",
                bad.len()
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// StackAttestationGate (M14.2)
// ---------------------------------------------------------------------------

/// Enforces agent stack attestation on push.
///
/// When a repo has a configured stack policy (a required fingerprint), only
/// agents whose registered stack fingerprint matches that policy are allowed
/// to push. If no policy is set for the repo the gate always passes (the
/// stack is recorded for informational purposes only).
pub struct StackAttestationGate;

impl PreAcceptGate for StackAttestationGate {
    fn name(&self) -> &str {
        "stack-attestation"
    }

    fn check(&self, ctx: &PushContext) -> GateOutcome {
        // No repo-level policy → attestation is informational; always pass.
        let required = match &ctx.required_fingerprint {
            Some(fp) => fp,
            None => return GateOutcome::Passed,
        };

        match &ctx.stack_fingerprint {
            Some(fp) if fp == required => GateOutcome::Passed,
            Some(fp) => GateOutcome::Failed(format!(
                "stack-attestation: agent stack fingerprint '{fp}' does not match \
                 the repo policy fingerprint '{required}'. \
                 Update your stack or ask an admin to update the policy."
            )),
            None => GateOutcome::Failed(
                "stack-attestation: this repo requires stack attestation but the \
                 pushing agent has no registered stack. \
                 POST /api/v1/agents/{id}/stack before pushing."
                    .to_string(),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Registry helpers
// ---------------------------------------------------------------------------

/// Build the full list of built-in pre-accept gates.
pub fn builtin_gates() -> Vec<Box<dyn PreAcceptGate>> {
    vec![
        Box::new(ConventionalCommitGate),
        Box::new(TaskRefGate),
        Box::new(NoEmDashGate),
        Box::new(StackAttestationGate),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_ports::PushContext;

    fn ctx(branch: &str, messages: Vec<&str>) -> PushContext {
        PushContext {
            repo_id: "repo-1".to_string(),
            refname: format!("refs/heads/{branch}"),
            branch: branch.to_string(),
            commit_messages: messages.into_iter().map(|s| s.to_string()).collect(),
            changed_files: vec![],
            agent_id: None,
            stack_fingerprint: None,
            required_fingerprint: None,
        }
    }

    // --- ConventionalCommitGate ---

    #[test]
    fn conventional_commit_passes_valid() {
        let gate = ConventionalCommitGate;
        let messages = vec![
            "feat: add login endpoint",
            "fix(auth): handle expired tokens",
            "chore(deps): bump serde to 1.0.200",
            "docs: update README",
        ];
        for msg in messages {
            let c = ctx("main", vec![msg]);
            assert!(
                matches!(gate.check(&c), GateOutcome::Passed),
                "should pass: {msg}"
            );
        }
    }

    #[test]
    fn conventional_commit_rejects_missing_type() {
        let gate = ConventionalCommitGate;
        let c = ctx("main", vec!["Add login endpoint"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn conventional_commit_rejects_bad_type() {
        let gate = ConventionalCommitGate;
        let c = ctx("main", vec!["wip: some work"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn conventional_commit_rejects_empty_description() {
        let gate = ConventionalCommitGate;
        let c = ctx("main", vec!["feat:"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn conventional_commit_rejects_unclosed_scope() {
        let gate = ConventionalCommitGate;
        let c = ctx("main", vec!["feat(scope: bad"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn conventional_commit_passes_multiline() {
        let gate = ConventionalCommitGate;
        let c = ctx("main", vec!["feat: add thing\n\nThis is the body."]);
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    // --- TaskRefGate ---

    #[test]
    fn task_ref_passes_non_feat_branch() {
        let gate = TaskRefGate;
        let c = ctx("main", vec!["chore: cleanup"]);
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    #[test]
    fn task_ref_passes_feat_branch_with_task_id() {
        let gate = TaskRefGate;
        let c = ctx(
            "feat/my-feature",
            vec!["feat(server): add endpoint TASK-42"],
        );
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    #[test]
    fn task_ref_fails_feat_branch_without_task_id() {
        let gate = TaskRefGate;
        let c = ctx("feat/my-feature", vec!["feat: add something"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn task_ref_fails_fix_branch_without_task_id() {
        let gate = TaskRefGate;
        let c = ctx("fix/null-ptr", vec!["fix: avoid null dereference"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn task_ref_passes_fix_branch_with_task_in_body() {
        let gate = TaskRefGate;
        let c = ctx(
            "fix/null-ptr",
            vec!["fix: avoid null dereference\n\nFixes TASK-007"],
        );
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    // --- NoEmDashGate ---

    #[test]
    fn no_em_dash_passes_clean_message() {
        let gate = NoEmDashGate;
        let c = ctx("main", vec!["feat: add thing -- with a dash"]);
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    #[test]
    fn no_em_dash_rejects_em_dash_in_message() {
        let gate = NoEmDashGate;
        let c = ctx("main", vec!["feat: add thing \u{2014} with em dash"]);
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    // --- builtin_gates ---

    #[test]
    fn builtin_gates_includes_all_four() {
        let gates = builtin_gates();
        let names: Vec<&str> = gates.iter().map(|g| g.name()).collect();
        assert!(names.contains(&"conventional-commit"));
        assert!(names.contains(&"task-ref"));
        assert!(names.contains(&"no-em-dash"));
        assert!(names.contains(&"stack-attestation"));
    }

    // --- StackAttestationGate ---

    fn ctx_with_stack(
        stack_fingerprint: Option<&str>,
        required_fingerprint: Option<&str>,
    ) -> PushContext {
        PushContext {
            repo_id: "repo-1".to_string(),
            refname: "refs/heads/main".to_string(),
            branch: "main".to_string(),
            commit_messages: vec!["feat: something".to_string()],
            changed_files: vec![],
            agent_id: Some("agent-1".to_string()),
            stack_fingerprint: stack_fingerprint.map(|s| s.to_string()),
            required_fingerprint: required_fingerprint.map(|s| s.to_string()),
        }
    }

    #[test]
    fn stack_attestation_passes_no_policy() {
        let gate = StackAttestationGate;
        let c = ctx_with_stack(None, None);
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    #[test]
    fn stack_attestation_passes_matching_fingerprint() {
        let gate = StackAttestationGate;
        let fp = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let c = ctx_with_stack(Some(fp), Some(fp));
        assert!(matches!(gate.check(&c), GateOutcome::Passed));
    }

    #[test]
    fn stack_attestation_fails_mismatched_fingerprint() {
        let gate = StackAttestationGate;
        let c = ctx_with_stack(Some("aaaa"), Some("bbbb"));
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }

    #[test]
    fn stack_attestation_fails_no_stack_with_policy() {
        let gate = StackAttestationGate;
        let c = ctx_with_stack(None, Some("required-fp"));
        assert!(matches!(gate.check(&c), GateOutcome::Failed(_)));
    }
}
