---
title: "Forge-level conventional-commit push enforcement: unconditional validation, scope rule, spec type table, sideband rejection contract"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / forge-level-conventional-push-enforcement / d1e5376399b8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §Conventional Commits: Forge-Enforced / §Pre-Accept Enforcement, commit-message validation is a git-protocol-level, unconditional pre-accept behavior of the forge, with a precise rejection contract. Four deviations to close, all in production push paths: (1) Forge-level default — the conventional-commit gate is evaluated for EVERY smart-HTTP receive-pack push regardless of repo_push_gates configuration (today check_pre_accept_gates returns Ok early when a repo has no gates configured, so a default repo accepts non-conforming pushes). Implement the default inside the shared gate-evaluation entry (check_pre_accept_gates in git_http.rs) so any other consumer of that entry (notably the source-control plan's jj-export parity task, whose contract predates this change) inherits the identical default and the two delivery routes cannot diverge; repos' configured named gates (task-ref, no-em-dash, etc.) continue to run in addition, and listing 'conventional-commit' via the push-gates admin API remains accepted (idempotent with the now-default behavior). (2) Scope rule — a present scope must be lowercase alphanumeric ([a-z0-9]+): 'feat(AUTH): x' and 'feat(My Scope): x' are rejected, 'feat(auth): x' and 'feat: x' are accepted. (3) Type whitelist matches the spec's ten-type table exactly (feat, fix, docs, style, refactor, perf, test, build, ci, chore) — drop 'revert', which is not in the spec's Types table, from both the whitelist and the error text ('unknown types are rejected' now means exactly the ten). (4) Error contract — the rejection is delivered in the git protocol so the pushing client sees it: per-offender detail (commit SHA + raw subject) must flow through the gate (extend PushContext / the PreAcceptGate port in gyre-ports to carry (sha, message) pairs, updating all gates and tests), and on rejection the receive-pack response reports the ref as rejected (report-status ng) with sideband 'remote:' lines (when the client negotiated side-band-64k) in the spec's exact shape: 'remote: [GYRE] Push rejected: commit {sha} has invalid message format', 'remote: [GYRE] Expected: <type>(<scope>): <description>', 'remote: [GYRE] Got: "{subject}"'; the HTTP error body carries the same message. Rejected pushes leave the ref at its pre-push position (existing undo_ref_updates path) and emit the GitPushRejected audit event. Every commit in the push range is still checked; empty descriptions still rejected. Do not change the gate's enforcement for the acceptance path, the other named gates, or the agent-workflow plan's separate agent-principal default-branch push rejection (a distinct check in the same handler — keep both).

## Acceptance Criteria

- [ ] A push of a commit with message 'fixed the thing' to a freshly created repo with NO gates configured is rejected: git exits nonzero, the client output contains 'remote: [GYRE] Push rejected: commit {sha} has invalid message format' plus the Expected/Got lines naming the offending commit, and the target ref is unchanged after the push; a GitPushRejected audit event records the rejection.
- [ ] Scope rule enforced on a default repo: 'feat(AUTH): x' is rejected with the same contract while 'feat(auth): x' on the same repo is accepted (and 'feat: x' without a scope is accepted).
- [ ] Type table enforced: 'revert: x' is rejected as an unknown type while 'chore: x' is accepted; the rejection text lists exactly the spec's ten types.
- [ ] Configured named gates (task-ref, no-em-dash) still evaluate as before on top of the forge-level default; pushes with conventional messages to default repos continue to succeed unchanged (no fail-closed regression).
- [ ] The PushContext/PreAcceptGate port change carries (sha, message) pairs and all existing gates and their tests are updated; no duplicated validation logic between the receive-pack path and the shared gate-evaluation entry.

## Verification intent

Create a fresh repo (no push gates configured), push a commit whose message is 'fixed the thing' via the git CLI over smart HTTP, and observe the push result: pre-task the push SUCCEEDS (the ref advances and the push is audited as accepted — the real defect: check_pre_accept_gates returns Ok early for unconfigured repos, so the spec's unconditional git-protocol-level enforcement does not exist), so the probe's assertion (nonzero exit + 'remote: [GYRE] Push rejected: commit {sha}...' in the output + unchanged ref) fails today. Controls that kill partial implementations: on the same fresh repo, 'feat(AUTH): x' must be rejected while 'feat(auth): x' succeeds (an implementation that rejects everything or skips the scope rule fails one arm), and 'revert: x' must be rejected while 'chore: x' succeeds (whitelist still broader than the spec's table fails this arm).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "0707840bfbb865b285a8c0a7",
    "decision": "gap",
    "requirement_quote": "The forge validates commit messages at push time:\n- Every commit in the push must match the conventional format\n- Unknown types are rejected\n- Empty descriptions are rejected\n- Scope is optional but must be lowercase alphanumeric if present",
    "detail": "Four concrete deviations from the specced enforcement contract. (1) The enforcement is opt-in per repository, not forge-level: check_pre_accept_gates returns Ok with no evaluations when a repo has no gates configured, and gates are only registered via the push-gates administration API (set_for_repo); a newly created repo has no conventional-commit gate, so pushes with non-conforming messages to such repos are accepted — the spec's unconditional 'Pushes with non-conforming commit messages are rejected before entering the repo' does not hold. (2) The scope rule 'must be lowercase alphanumeric if present' is not enforced at all: is_conventional only matches parentheses and never validates the scope's content or case ('feat(AUTH): x' or 'feat(My Scope): x' pass the gate). (3) The error contract is not met: the spec requires a clear error in git sideband output naming the offending commit ('remote: [GYRE] Push rejected: commit abc123 has invalid message format ... Got: ...'); the implementation returns an HTTP 403 plain-text body 'push rejected by gate ...' with no commit SHA and no sideband pkt-line framing. (4) The gate's type whitelist accepts 'revert', a type not in the spec's Types table, so the 'unknown types are rejected' rule is broader than the contract. Additionally, the gate runs after receive-pack has processed the packfile and undoes the ref updates on rejection (undo_ref_updates), so rejected commits do briefly enter the object database even though the ref never moves — weaker than 'before entering the repo'. What does work: every commit in the push range is checked, unknown (non-whitelisted) types are rejected, and empty descriptions are rejected.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "    if gate_names.is_empty() {\n        return Ok(Vec::new());\n    }",
        "detail": "check_pre_accept_gates skips all validation for repos with no configured gates. Gate names come from state.repo_push_gates.get_for_repo(repo_id); the only production setter is the push-gates administration API (api/push_gates.rs set_for_repo). Repo creation (api/repos.rs) registers no default gates, so a default repo enforces no commit-format validation — the spec's git-protocol-level unconditional enforcement is not implemented."
      },
      {
        "file": "crates/gyre-server/src/pre_accept.rs",
        "quote": "    let types = [\n        \"feat\", \"fix\", \"docs\", \"style\", \"refactor\", \"perf\", \"test\", \"build\", \"ci\", \"chore\",\n        \"revert\",\n    ];",
        "detail": "The gate's allowed-type whitelist. It rejects types outside the list (unknown types rejected) but includes 'revert', which is not one of the spec's ten types, so the whitelist does not match the spec's Types table."
      },
      {
        "file": "crates/gyre-server/src/pre_accept.rs",
        "quote": "    let description = rest.trim_start_matches(' ');\n    !description.is_empty()",
        "detail": "Empty descriptions are rejected (this part of the contract is met). However, the full is_conventional function was inspected: the only scope handling is locating '(' and ')' in the type part; the scope's content is never validated for lowercase-alphanumeric form, so 'feat(AUTH): x' and 'feat(Any Thing): x' pass — the 'Scope is optional but must be lowercase alphanumeric if present' rule is unenforced."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "                    format!(\n                        \"push rejected by gate '{}': {}\",\n                        rejection.gate_name, rejection.reason\n                    ),",
        "detail": "On gate rejection the handler returns an HTTP 403 with this plain-text body. The spec requires the error in git sideband output ('remote: [GYRE] Push rejected: commit abc123 has invalid message format / Expected: ... / Got: ...'), naming the offending commit SHA; the actual response carries no SHA and is not sideband-formatted (the sideband builder build_feedback_sideband is only used for the acceptance path)."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "    // Pre-accept gate checks: run after git accepts packfile, undo refs on failure.",
        "detail": "The gates run after run_git_stateless('receive-pack', ...) has executed; on rejection undo_ref_updates rolls the refs back. The ref never points at the rejected commits, but the packfile objects do enter the repository's object database before rejection — weaker than the spec's 'rejected before entering the repo'."
      }
    ]
  }
]
