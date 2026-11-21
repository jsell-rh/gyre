---
title: "Breaking change detection: API-contract-change route and MCP notification of the dependent repo's orchestrator"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / breaking-change-contract-route / ed9f34316476"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

The conventional-commit route and the entire downstream cascade (BreakingChange records, per-dependent High-priority tasks with the specced title/labels, Block policy failing the merge queue, member notifications, workspace events) are real and unchanged. This task closes the two normative shortfalls. (1) API-CONTRACT-CHANGE ROUTE (spec: "or API contract change"): in detect_breaking_changes_on_push, in addition to detect_breaking_commits, detect incompatible API contract changes: when a push modifies any openapi.yaml/openapi.yml/swagger.json file in the tree, parse the contract at old_sha and at new_sha (read_git_file at both) and diff the operation set — an operation (method + path) present in the old contract and absent in the new one (removed or renamed endpoint) is an API contract change; it feeds the SAME process_breaking_changes cascade with a description naming the concrete change (e.g. "API contract change: removed DELETE /widgets from api/openapi.yaml"), gated on existing dependents exactly like the conventional-commit route. Additive-only contract changes (new operations, no removals) are NOT breaking. YAML and JSON contract forms must both parse; an unparseable contract on either side logs a warning and skips that route for the push (no panic, no fail-open cascade). (2) MCP ORCHESTRATOR NOTIFICATION (spec step 3b: "Notify the dependent repo's orchestrator via MCP"): alongside the existing notify_rich member notifications and the workspace-bus broadcast, the forge delivers the notification via MCP: resolve the live repo orchestrator agent for the dependent repo (the same agent source gyre_list_repo_orchestrators / list_repo_orchestrators_core reads) and send a Directed, persisted-until-acked message on the message bus addressed Destination::Agent(orchestrator_agent_id) carrying the breaking-change payload (repo, description, breaking-change id, dependent repo) — the bus the gyre_message_poll MCP tool serves, so the orchestrator receives it through its MCP polling loop. When no repo orchestrator agent exists for the dependent repo, the existing channels remain and a warning names the dependent repo and the missing orchestrator (no silent drop of the MCP arm). COORDINATION: shares the git_http.rs push path with dep-detection-extraction-forms (different functions; reuse tree-enumeration helpers rather than duplicating them).

## Acceptance Criteria

- [ ] Push to repo B (with dependent repo A) that removes an operation from B's openapi.yaml in a plain `fix:` commit (no `!` marker, no BREAKING CHANGE footer) creates a BreakingChange record for the A→B edge, sets the edge status Breaking, and files the High-priority task titled "Breaking change in {B}: API contract change: ..." with labels ["dependency-update", "breaking-change", "auto-created"].
- [ ] A Directed message addressed to A's live repo orchestrator agent is persisted on the message bus and is returned by gyre_message_poll for that orchestrator, carrying the breaking-change payload.
- [ ] With the workspace policy set to block, the merge queue entry for the change fails while the breaking change is unacknowledged (existing Block mechanism, now also reachable via the contract route); POST acknowledge clears it.
- [ ] A push that only ADDS an endpoint to the contract triggers no cascade.
- [ ] When the dependent repo has no live orchestrator agent, the server logs a warning naming the dependent repo, and member notifications still fire.

## Verification intent

Push to repo B a commit titled `fix: tidy contract` that removes `DELETE /widgets` from openapi.yaml, with repo A depending on B via a DependencyEdge: pre-fix detect_breaking_changes_on_push's ONLY detection input is detect_breaking_commits over conventional-commit markers, so an unmarked contract removal returns early — no BreakingChange record, no task, no edge status change — and the notify path has no MCP delivery at all (only notify_rich user notifications and a workspace-destination bus broadcast; grep of mcp.rs/message-bus writers for a breaking-change orchestrator notification finds nothing). The probe must fail until the contract-diff route feeds the cascade and the Directed MCP message to the dependent repo's orchestrator is delivered.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "c8dab08785afd0c6458ae06b",
    "decision": "gap",
    "requirement_quote": "When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):",
    "detail": "The conventional-commit route and the entire downstream cascade are real and code-verified: detect_breaking_commits parses the full push range with %B so both `feat!:` subjects and `BREAKING CHANGE:` footers are caught; process_breaking_changes queries list_dependents, persists BreakingChange records per dependent edge, sets edge status Breaking, creates High-priority tasks titled 'Breaking change in {repo}: {description}' with exactly the specced labels, notifies the dependent workspace's members, and broadcasts breaking_change_detected to the dependent workspace; the Block policy mechanically fails the merge until acknowledgment. Two normative shortfalls keep this section open: (1) the parenthetical names a second detection route — 'or API contract change' — which is not implemented anywhere: detect_breaking_changes_on_push's only detection input is detect_breaking_commits over conventional-commit markers; a push that removes an OpenAPI endpoint without a `!` marker triggers no cascade. The spec's own §Relationship (Versioning) bullet says conventional commits + semver 'are how the forge detects breaking changes', which designates the implemented route as the mechanism but does not amend away the 'or API contract change' arm — surfacing this tension rather than silently resolving it. (2) Step 3b says 'Notify the dependent repo's orchestrator via MCP'; the implementation notifies via user notifications (notify_rich) and a workspace-destination message-bus broadcast, with no MCP-tool notification path (grep of mcp.rs for breaking-change notify finds nothing).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        if let Some(parsed) = crate::version_compute::parse_conventional(sha, message) {\n            if parsed.is_breaking {",
        "detail": "detect_breaking_commits is the sole breaking-change detection input: conventional-commit parsing of `!` markers and BREAKING CHANGE: footers. No API-contract-diff route feeds process_breaking_changes."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "    let commit_log = String::from_utf8_lossy(&log_out);\n    let breaking_commits = detect_breaking_commits(&commit_log);",
        "detail": "detect_breaking_changes_on_push derives breaking commits exclusively from the git log message text; OpenAPI/proto contract changes pushed without conventional-commit markers produce no BreakingChange records."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
        "detail": "The specced per-dependent task creation is real: High-priority tasks with the exact title format and labels [dependency-update, breaking-change, auto-created], plus BreakingChange records, edge status updates, member notifications, and workspace broadcasts."
      }
    ]
  }
]
