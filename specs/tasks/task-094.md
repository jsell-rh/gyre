---
title: "Platform Model Cross-Repo Spec Escalation Protocol"
spec_ref: "platform-model.md §3 Cross-Repo Spec Escalation Protocol"
depends_on:
  - task-093
progress: ready-for-review
coverage_sections:
  - "platform-model.md §3 Cross-Repo Spec Escalation Protocol"
commits: ["0bff43283532f309624a7ceec8f0363cfd034406"]
---

## Spec Excerpt

When a repo orchestrator needs something from another repo:

```
1. Repo A orchestrator: "I need trait UserPort from Repo B"

2. Repo A checks: does Repo B's spec describe UserPort?

   YES -> Repo A escalates to Workspace Orchestrator:
          "Repo B has UserPort in its spec but hasn't implemented it yet.
           I need it for TASK-042."
          Workspace Orchestrator:
          - Creates cross-repo task in Repo B
          - Creates MR dependency (Repo A's MR depends on Repo B's MR)
          - Notifies Repo B's orchestrator

   NO  -> Repo A escalates to Workspace Orchestrator:
          "Repo B does not have UserPort in its spec. I need a spec change."
          Workspace Orchestrator:
          - Drafts a spec change for Repo B (agent writes it)
          - Escalates to HUMAN for approval (spec change = design decision)
          - Human approves or rejects
          - If approved: spec lifecycle auto-creates task in Repo B
          - Workspace Orchestrator creates the MR dependency
```

**The human gate:** Cross-repo spec changes always require human approval.

## Implementation Plan

1. **Escalation message schema:**
   - Define `EscalationPayload` struct with `source_repo_id`, `target_repo_id`, `needed_artifact` (spec path or interface name), `reason`, `task_id`, `escalation_type` (ImplementationNeeded | SpecChangeNeeded)
   - Use existing `MessageKind::Escalation` with this structured payload

2. **MCP tool for escalation:**
   - Add `gyre_escalate_cross_repo` MCP tool for repo orchestrators
   - Input: target_repo, needed_artifact, reason, escalation_type
   - Sends Directed-tier Escalation message to workspace orchestrator

3. **Workspace orchestrator escalation handler:**
   - When workspace orchestrator receives Escalation via `message.poll`:
     - If ImplementationNeeded: create task in target repo, create MR dependency
     - If SpecChangeNeeded: draft spec change, create human notification (priority 2)
   - MR dependency: extend `MergeRequest` domain with `depends_on_mr_ids: Vec<Id>`

4. **MR dependency tracking:**
   - Add `mr_dependencies` table linking MRs across repos
   - Merge processor checks dependencies before merging: blocked MR cannot merge until dependency MR is merged
   - UI shows dependency chain on MR detail panel

5. **Human approval flow for cross-repo spec changes:**
   - Notification with "Approve spec change" / "Reject" inline actions
   - Approval triggers spec lifecycle auto-task creation
   - Rejection notifies workspace orchestrator to find alternative

## Acceptance Criteria

- [ ] EscalationPayload struct covers both escalation types
- [ ] `gyre_escalate_cross_repo` MCP tool available to repo orchestrators
- [ ] Workspace orchestrator can create cross-repo tasks
- [ ] MR dependency table and tracking implemented
- [ ] Merge processor blocks MRs with unmerged dependencies
- [ ] Human notification created for cross-repo spec changes
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §3 "Cross-Repo Spec Escalation Protocol". The Escalation MessageKind already exists in `gyre-common/src/message.rs`. MCP tools are defined in `gyre-server/src/mcp.rs`. The merge processor is in `gyre-server/src/merge_processor.rs`. MR dependencies are a new concept — check if any `depends_on` field already exists on the MR domain entity before creating one.

## Shipped

Shipped in `0bff4328` (all paths relative to repo root):

- **EscalationPayload** (`crates/gyre-common/src/message.rs`): struct with
  `source_repo_id`, `target_repo_id`, `needed_artifact`, `reason`, `task_id`,
  `source_mr_id`, `target_mr_id`, and `escalation_type`
  (`EscalationType::ImplementationNeeded | SpecChangeNeeded`, kebab-case wire
  strings, unknown values rejected — never defaulted, so the human gate
  cannot be silently skipped). The `MessageKind::Escalation` payload schema
  is extended with the cross-repo fields as OPTIONAL fields so generic
  escalations (`{reason, context}` from the stale-agent detector,
  spec-lifecycle patrol, merge processor) keep validating; `reason` stays
  the shared required field.
- **`gyre_escalate_cross_repo` MCP tool** (`crates/gyre-server/src/mcp.rs`):
  repo-orchestrator tier only. Validates the target repo exists, lives in
  the caller's workspace, and differs from the caller's own repo; validates
  optional `task_id`/`source_mr_id` against the caller's repo and
  `target_mr_id` against the target repo. In the `needs_write` RBAC gate
  and the pre-dispatch scope check (caller's own repo spawn right).
- **Escalation delivery + processing**
  (`crates/gyre-server/src/cross_repo_escalation.rs`): `escalate()` delivers
  the Directed-tier Escalation message to the workspace orchestrator's
  inbox (signed, persisted, ack-based — pollable via `message.poll`;
  workspace orchestrator spawned on demand via the task-093 core) and
  drives processing under the per-workspace registry lock, the same shape
  as the SpecApproved signal chain. `message.poll` by a workspace
  orchestrator additionally drives UNACKED structured escalations as the
  crash-recovery receipt path; generic escalations never parse as
  cross-repo payloads and stay for their real consumers.
- **ImplementationNeeded branch**: creates the cross-repo Implementation
  task in the TARGET repo (real task row, repo+workspace scoped,
  `cross-repo`/`escalation` labels), records the requesting task's
  `depends_on`, records the agent-declared MR dependency when both MRs are
  known (cycle-checked via the merge-deps helper), and notifies the target
  repo's orchestrator (spawned on demand) with a TaskAssignment inbox
  message.
- **SpecChangeNeeded branch (the human gate)**: drafts the spec change in
  the target repo — a `spec-edit/<slug>-<uuid>` branch off the default
  branch carrying the drafted spec content (real git write-through), with
  an open MR targeting the default branch authored by the workspace
  orchestrator — links the requesting MR onto the spec MR via the MR
  dependency, and fans out priority-2 `SpecApprovalRequested`
  notifications to the workspace's Admin/Developer/Owner members with the
  same inline-action body shape the specs/save flow uses
  (`spec_path`/`spec_sha`/`mr_id`/`branch`/`repo_id` plus the cross-repo
  context). Approval rides the existing `POST /specs/:path/approve` →
  `SpecApproved` signal chain (auto-creates the implementation task in the
  target repo); rejection rides the existing reject endpoint.
- **MR dependency table/tracking + merge blocking**: pre-existing
  (task-100/merge-dependencies.md) — `MergeRequest.depends_on` domain field
  persisted in `merge_requests.depends_on` (SQLite + mem),
  `merge_processor::dependencies_satisfied` blocks merging while
  dependencies are unmerged, PUT/GET/DELETE `/merge-requests/{id}/dependencies`
  API, and the MR detail panel renders the dependency chain. The escalation
  handler records dependencies through the same field the merge processor
  enforces (`dependencies_satisfied` is now `pub(crate)` and asserted
  directly in a test).

### Test evidence

Executed here: `cargo test -p gyre-common --lib` — 132 passed (incl. the
new payload/schema tests). `cargo check -p gyre-server --lib --tests` —
clean. rustfmt on all changed files — clean. All repo mechanical scripts
pass on the changed surface (arch, mcp-write-tools, byte-slice,
dead-message-kinds, inert-enforcement, fabricated/scope-literal defaults,
relative-path, lossy-secret, fail-open-ref, task-commit-attribution).

Server test EXECUTION is wrapper-blocked in the implement stage (4 GiB
budget); the verifier must run:

- `cargo test -p gyre-server --lib cross_repo_escalation` — end-to-end
  protocol tests incl. real-git2 write-through of the drafted spec change
  and the merge-processor dependency gate blocking the source MR.
- `cargo test -p gyre-server --lib mcp_escalate_cross_repo` — MCP-surface
  tier gating, cross-workspace denial, and the happy path.
- `cargo test -p gyre-server --lib` and `cargo test --all --quiet`
  (acceptance criterion).

Details: `/tmp/stage/review-evidence/task-094-implement.md`.
