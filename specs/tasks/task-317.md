---
title: "Changelog entries carry the spec reference resolved from commit provenance"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / changelog-spec-reference-provenance / d1e5376399b8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §Changelog Generation, each changelog entry must include the spec reference — 'which spec it implements (from commit provenance)' — alongside the already-implemented agent attribution, task reference, and conventional-commit scope. (1) ChangelogEntry (crates/gyre-server/src/version_compute.rs) gains a spec_ref: Option<String> field. (2) release_prepare (crates/gyre-server/src/api/release.rs) resolves it per commit from real provenance state, preferring the direct record and falling back to the task join: AgentCommit.spec_ref when that field exists and is set (the field is delivered by the agent-gates plan's auto-mr-pinned-spec-ref task, planned but not yet published — consume it if landed; do NOT fork a second agent_commits spec field if it is absent), otherwise resolve via the existing provenance chain already loaded in the handler: agent_commits row's task_id -> state.tasks lookup -> task.spec_ref (the pinned 'path@sha'). Commits without provenance (human pushes with no agent_commits row, or tasks without a pinned spec_ref) render no spec reference — never a fabricated value. (3) render_changelog_markdown emits the spec reference on each entry that has one, in the spec's example form ('spec: system/identity-security.md@abc123'), in the BREAKING CHANGES / Features / Bug Fixes sections alike; the ReleasePrepareResponse sections carry the field so API consumers see it too. The resolution is a real query against the provenance and task stores on every prepare — not a cached or hardcoded mapping.

## Acceptance Criteria

- [ ] On a repo whose commits have provenance (an agent_commits row with a task_id whose task carries spec_ref 'system/foo.md@abc123'), POST /api/v1/release/prepare produces a changelog whose markdown for that commit contains 'spec: system/foo.md@abc123', and the response's ChangelogEntry carries spec_ref = Some('system/foo.md@abc123').
- [ ] A commit in the same range pushed by a human (no agent_commits row) yields an entry with spec_ref = None and renders without a spec reference — no fabricated or defaulted value (mirrors the fabricated-scope discipline).
- [ ] The rendered markdown keeps the existing agent attribution, task reference and scope on every entry (no regression), and the spec reference appears in all three sections (breaking/features/fixes) for entries that have one.

## Verification intent

Seed a repo with a tag, push a conventional commit attributed to an agent session whose task pins spec_ref 'system/foo.md@abc123' (agent_commits row with that task_id), run release prepare, and assert the generated markdown contains 'spec: system/foo.md@abc123' on that entry: pre-task the probe fails because ChangelogEntry has no spec_ref field, AgentCommit has no spec-reference field, and render_changelog_markdown emits only scope/agent/task — grep for 'spec:' in the generated changelog returns nothing (the real defect: the fourth required attribution cannot be produced by any code path). Control: a human-pushed commit in the same range must render WITHOUT any spec reference — an implementation that fabricates a default spec string fails this control.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "5e69a034fc0a01c1f8222f5e",
    "decision": "gap",
    "requirement_quote": "Each entry includes:\n- **Agent attribution** - which agent produced the change\n- **Task reference** - which task it was part of\n- **Spec reference** - which spec it implements (from commit provenance)\n- **Conventional commit scope** - which area of the codebase",
    "detail": "Changelog generation exists and is real: release_prepare renders a markdown changelog grouped into BREAKING CHANGES / Features / Bug Fixes, each entry carrying the conventional commit scope, the producing agent (name or id, resolved from the agent-commit provenance store) and the task reference. But the fourth required attribution — the spec reference ('which spec it implements (from commit provenance)') — is absent: the provenance record the changelog is built from (AgentCommit) has no spec-reference field (only task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level), and ChangelogEntry has no spec-reference field. There is no lookup anywhere in the release path that would map a commit to the spec it implements, so the generated changelogs cannot contain the specced spec references.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/version_compute.rs",
        "quote": "pub struct ChangelogEntry {\n    pub sha: String,\n    pub commit_type: String,\n    pub scope: Option<String>,\n    pub description: String,\n    pub is_breaking: bool,\n    pub agent_id: Option<String>,\n    pub agent_name: Option<String>,\n    pub task_id: Option<String>,\n}",
        "detail": "The changelog entry type carries scope (conventional commit scope), agent attribution (agent_id/agent_name) and task reference — but no spec-reference field. The spec explicitly requires each entry to include 'Spec reference - which spec it implements (from commit provenance)'; the rendered markdown in render_changelog_markdown correspondingly emits only scope, agent and task attribution."
      },
      {
        "file": "crates/gyre-domain/src/agent_tracking.rs",
        "quote": "pub struct AgentCommit {\n    pub id: Id,\n    pub agent_id: Id,\n    pub repository_id: Id,\n    pub commit_sha: String,\n    pub branch: String,\n    pub timestamp: u64,",
        "detail": "AgentCommit is the commit-provenance record the changelog attribution is resolved from (release_prepare builds prov_map from state.agent_commits.find_by_repo keyed by commit_sha). Its full field list was inspected: id, agent_id, repository_id, commit_sha, branch, timestamp, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level — there is no spec-reference field, so 'which spec it implements' cannot be derived from commit provenance as the spec requires."
      }
    ]
  }
]
