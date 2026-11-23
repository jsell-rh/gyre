---
title: "Carry out the remaining amendment-table rows, the Agent task-binding row, and the section close-out"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / amendment-table-docs-and-binding / e2624c4730e7"
depends_on: [task-298, task-302]
progress: not-started
commits: []
---

## Required behavior

New delta task, the single owner of the agent-runtime.md 'Relationship to Existing Specs' amendment-table gap. Every amendment row is owned exactly once: the code-side rows belong to existing task-298 'Remove orchestrator agent-spawning and carry out the spec amendment table' (this task's declared dependency — gyre_spawn_worker AND gyre_spawn_repo_orchestrator removal per that task's plan-carried extension, /api/v1/personas route removal with registry-carried operational fields, the ABAC 'archive' action, ralph-loop.md supersession cleanup, hierarchy-enforcement.md §4 route-table/tenant-filter amendments); the meta-spec-set registry-replacement clauses belong to satisfied task-302 (also a declared dependency — endpoint retirement, meta_specs_used-based drift detection, content_hash snapshot tuples); the rows the revision-4 audit dispositions satisfied need no work (TaskType/order/depends_on/parent_task_id; MessageKind::SpecApproved in code; Workspace.compute_target_id; AgentStatus enum; spec-lifecycle terminology; MergeAttestation.meta_specs_used population; meta_spec/compute_target resolver resources; the Persona required/prompt/content_hash fields via the unified MetaSpec registry). The Phase 2 and Phase 3 boundary violations (the two orchestrator-tier MCP spawn tools) are owned by task-298 in this plan, not by this task — no overlap. This task's own rows: (1) Supersede row 'docs/agent-protocol.md — consolidated into this spec': retire docs/agent-protocol.md — verify every requirement it documents is owned by the canonical spec (agent-runtime.md is the single lifecycle authority); relocate still-accurate operational detail that belongs to no canonical spec (exact container env vars, gate protocol mechanics, and the WS-transport and terminal-worktree semantics landed by the now-complete tasks task-310 (websocket-agent-transport) and task-269 (terminal-worktree-cleanup) — both already present in the file: the 'Worktree teardown is deferred to terminal completion' section and the 'WebSocket primary transport' section) to the appropriate existing docs (docs/server-config.md / docs/development.md), and update every reference (AGENTS.md Documentation Index, docs/api-reference.md cross-references, web/CLI help) with no dead links and no second lifecycle narrative; never edit agent-runtime.md's normative content. RETIREMENT SEQUENCING (correction of the revision-3 draft, which cited the misnumbered tasks task-311/task-272 as unfinished prerequisites): the two tasks formerly contracted to edit docs/agent-protocol.md are task-310 and task-269, and both are complete at this base with their content landed in the file — no unfinished plan edits it, so the retirement needs no external dependency beyond this plan's own task-298 and task-302; this task relocates the landed content rather than deleting content yet to be written. The assigned ci-docs-release plan's docs-site task (task-321) builds one rendered page per file currently in docs/ — its acceptance is source-dynamic by contract, so the retirement shrinks its page set by design; record the page-list change in this task's close-out so the task-321 verification is not surprised by the delta (no dependency edge — the docs-site task consumes docs/ dynamically and needs no change for the retirement). (2) message-bus.md MessageKind amendment: document SpecApproved in specs/system/message-bus.md — Event tier, server-only, payload {repo_id, spec_path, spec_sha, approved_by, approval_id}, fired by the spec approval handler, distinct from SpecChanged (which fires on push, before approval); the workspace orchestrator subscribes to SpecApproved — matching the existing code (gyre-common/src/message.rs). Doc-only; the active message-bus plan (task-255) owns record-activity-fail-closed and dead-inbox-sweep in different sections of that document — no overlap. (3) abac-policy-engine.md §Resource attributes: add meta_spec (attributes scope, scope_id, kind) and compute_target (attribute tenant_id) to the resource type list and add archive to the action attribute table, matching the resolver and the post-task-298 code state. (4) human-system-interface.md §5: amend the attestation bundle schema to include the meta_specs_used array with full content SHAs, matching the production MergeAttestation. (5) meta-spec-reconciliation.md: replace the retired PUT/GET /api/v1/workspaces/{id}/meta-spec-set endpoint text (and the tenant twin) with the registry model this spec mandates — required flags plus spec-level bindings via the meta-spec API, reconciliation triggered on required-meta-spec approval, MetaSpecSnapshot as {meta_spec_id, kind, content_hash, version} tuples — removing the doc's contradiction with the retired code (the endpoints 404 in production while the doc still specces them). Doc-only and limited to the retired-endpoint rows; the assigned meta-spec-reconciliation plan (task-332) owns the registry-parity/repo-scope/blast-radius/candidate-scoping/drift-policy/sweep/observability code surfaces of that document — no overlap with its deliverables. (6) Agent-entity task-binding row ('Add task_id: Id field — every agent is bound to exactly one task (the system creates the binding mechanically)'): make the binding non-optional and mechanically enforced for every task-bound agent kind — the spawn core fails closed (hard error, no agent record persisted) when creating a worker (bound to its Implementation task), repo orchestrator (bound to the Delegation/Coordination task being processed), gate reviewer (bound to the MR's source task), or interrogation agent (bound to the target agent's task) without a resolvable binding; schema migration at the next free 6-digit sequence number with the port contract enforced in every adapter including the mem adapter. The workspace orchestrator is taskless by §1 Phase 2's own definition (an inbox-processing agent that creates tasks and is bound to none), so the row's 'every agent' wording conflicts with the canonical spec's own Phase 2 — resolve that conflict through the spec lifecycle (propose the one-line amendment scoping the row to task-bound agent kinds, requiring human approval), never by fabricating a binding or silently shipping the contradiction; this proposal targets the amendment-table row, a different section from the §1 single-trigger amendment proposed by task-297 in this plan — the two proposals must be coordinated (one spec-edit branch or two non-conflicting branches) but do not overlap. (7) Section close-out, scoped exactly to this task's declared dependencies (task-298, task-302) and its own rows (1)-(6): verify every amendment-table row is carried out — the task-298 deliverables (gyre_spawn_worker and gyre_spawn_repo_orchestrator absent with no orchestrator-creatable agent records via MCP; /api/v1/personas 404 with persona CRUD/resolution via /api/v1/meta-specs?kind=meta:persona carrying the operational fields and required/prompt/content_hash/version; archive action live in the ABAC middleware with routes registered and exemption entries removed; ralph-loop.md retired with zero live cross-references; hierarchy-enforcement.md route-table/tenant-filter/storage-note amendments), the task-302 deliverables (workspace/tenant meta-spec-set endpoints 404/405 with no production writer; merge-time drift enforcement comparing meta_specs_used against current required versions), this task's rows (1)-(6), and the already-satisfied audit rows listed above — recording the task-321 docs-site page-list change as a close-out note.

## Acceptance Criteria

- [ ] docs/agent-protocol.md no longer exists; no file in specs/ or docs/ duplicates the agent lifecycle narrative; every former reference (AGENTS.md docs index, docs/api-reference.md cross-references, web/CLI help) resolves to agent-runtime.md or the relocated operational doc (grep 'agent-protocol.md' returns no live references); the WS-transport content landed by task-310 and the terminal-worktree-cleanup semantics landed by task-269 are present in the relocated docs, not lost by the retirement (both tasks are complete at the base, so their content is in the file being relocated).
- [ ] specs/system/message-bus.md documents SpecApproved with tier/payload/origin matching gyre-common/src/message.rs; specs/system/abac-policy-engine.md names the meta_spec and compute_target resource types and the archive action; specs/system/human-system-interface.md §5 includes the meta_specs_used array; specs/system/meta-spec-reconciliation.md no longer specces the retired meta-spec-set endpoints.
- [ ] Creating an agent record for a task-bound kind (worker, repo orchestrator, gate reviewer, interrogation) without a resolvable task binding fails closed with no agent record persisted; interrogation agents carry the target agent's task binding; the workspace-orchestrator exception is recorded as a spec-lifecycle amendment pending human approval, not a silent deviation.
- [ ] The close-out checklist passes for every amendment-table row, each row traceable to task-298, task-302, this task's rows (1)-(6), or an already-satisfied audit row — no row unaccounted — and the close-out notes the task-321 docs-site page-list change caused by the retirement.

## Verification intent

At the base every close-out assertion fails provably: grep SpecApproved specs/system/message-bus.md returns nothing (the kind exists only in code, gyre-common/src/message.rs) while the doc still lists only SpecChanged; docs/agent-protocol.md exists and documents the full lifecycle this spec claims to consolidate (304 lines, including the task-310 WS-transport and task-269 worktree-teardown content to be relocated); specs/system/human-system-interface.md §5 lacks meta_specs_used while the production MergeAttestation carries it; specs/system/meta-spec-reconciliation.md still specces PUT /api/v1/workspaces/{id}/meta-spec-set (line 246) while the endpoint 404s in production — the doc contradicts the deployed code. For the binding row: spawn an interrogation agent (POST /api/v1/agents/spawn, agent_type=interrogation) and read its record — current_task_id is None (gyre-domain/src/agent.rs:105, a Nullable column) and orchestrator.rs explicitly asserts task_id null for orchestrators, so no non-optional mechanical binding exists anywhere; the fail-closed taskless-spawn probe cannot pass at the base.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `0b33cb3a4a586c587e2566ecbcd82c03da94d84b`; plan: `da093bb0d16da84dcae7a1f8fc0ba4a14ecb9e8275c579ccae32fe282b48fdcc`.

[
  {
    "id": "a40446f46dbb0536f3c30ed0",
    "decision": "gap",
    "requirement_quote": "All cross-references to `ralph-loop.md` in other specs (e.g., `agent-gates.md`) should be updated to reference `agent-runtime.md` §1.",
    "detail": "The amendment table is partially carried out. SATISFIED rows: Task gains task_type (Implementation/Delegation/Coordination), order: Option<u32>, depends_on: Vec<Id>, parent_task_id: Option<Id>; MessageKind::SpecApproved exists in code (Event tier, server-only, specced payload) though message-bus.md was never amended; spec-lifecycle.md terminology cleaned (no 'CEO agent' remains; push-hook tasks informational); Workspace.compute_target_id added; the meta-spec-set endpoints retired in code (404, KV tables dropped by migration 000071) with MetaSpecSnapshot using content_hash tuples and drift detection comparing merge-attestation meta_specs_used against current required versions; MergeAttestation.meta_specs_used populated in production; the abac resolver carries meta_spec and compute_target resource mappings. UNMET rows (the open task-298 amendment-table gap, progress not-started): (1) the 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' row (platform-model.md §4) is NOT carried out: two orchestrator-tier spawn surfaces ship as live production code — gyre_spawn_worker (a repo-orchestrator JWT spawns a worker agent through the full spawn core; catalog, dispatch, scope validation and handler all present) and gyre_spawn_repo_orchestrator (a workspace-orchestrator JWT spawns the repo orchestrator agent directly via spawn_repo_orchestrator_core). task-298's current acceptance criterion names only the repo-orchestrator/worker surface ('a repo-orchestrator JWT has no MCP tool whose handler creates agent records'), so the workspace-orchestrator surface needs explicit ownership — an extension of task-298's contract or a separate assignment — recorded for the controller. (2) ralph-loop.md cross-references remain in canonical specs — authorization-provenance.md still cites ralph-loop.md for the Ralph Loop re-spawn semantics and meta-spec-reconciliation.md still carries a ralph-loop.md@sha example path (agent-gates.md itself was updated). (3) docs/agent-protocol.md still exists and is actively maintained ('consolidated into this spec' not carried out) — and the retirement is uncoordinated with active plans: the assigned task-311 (websocket-agent-transport) has a deliverable to update docs/agent-protocol.md, the assigned task-272 (terminal-worktree-cleanup) must update its worktree-cleanup claims, and task-321's docs site builds one page per docs/ file including agent-protocol, so deleting the file requires sequencing after those deliverables or explicitly retargeting them; recorded for the controller. (4) the /api/v1/personas endpoints were NOT removed — four routes remain live in api/mod.rs alongside the replacement registry. (5) the Persona struct gains no required: bool field and its prompt field is named system_prompt, not prompt (the required flag lives on the unified MetaSpec struct instead — see a00a6b5082295555260842b2). (6) message-bus.md contains no SpecApproved kind (the doc amendment was never applied). (7) the abac-policy-engine.md resource-attribute amendment is half-done in code — meta_spec and compute_target resources exist in the resolver, but the 'archive' action was never added (the archive/unarchive routes sit in the frozen ABAC exemption file with per-handler enforcement instead) and the abac-policy-engine.md doc has none of the three additions. (8) hierarchy-enforcement.md was never amended — no compute-target or meta-spec CRUD routes in its route table and meta_spec_versions/meta_spec_bindings/compute_targets are not in its tenant-filter configuration. (9) human-system-interface.md §5 was never amended to include meta_specs_used in the attestation bundle schema. (10) the Agent entity's task binding is current_task_id: Option<Id> rather than the specced non-optional task_id: Id. (11) meta-spec-reconciliation.md still specces the retired PUT /api/v1/workspaces/{id}/meta-spec-set endpoint, contradicting this spec's replacement of it.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"name\": \"gyre_spawn_worker\",",
        "detail": "Unmet amendment row (platform-model.md §4, 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'): the gyre_spawn_worker tool is live production code — a repo-orchestrator JWT can spawn a worker agent through the full spawn core (see 15aec83b58b3494602d31ce8)."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"name\": \"gyre_spawn_repo_orchestrator\",",
        "detail": "Unmet amendment row (same 'orchestrators do not spawn agents' rationale): a second live spawn surface lets a workspace-orchestrator JWT spawn the repo orchestrator agent directly (see 6d7302e3c6a23af8c4020c20) — not covered by task-298's current acceptance criterion, which scopes only the repo-orchestrator tier."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/personas\",",
        "detail": "Unmet amendment row (platform-model.md §2): the /api/v1/personas endpoints were NOT removed — the route block (list/create, resolve, :id CRUD, :id/approve) remains live alongside the replacement /api/v1/meta-specs?kind=meta:persona registry."
      },
      {
        "file": "specs/system/message-bus.md",
        "quote": "SpecChanged,",
        "detail": "Unmet amendment row (message-bus.md MessageKind): the canonical message-bus.md enumeration still lists SpecChanged but contains no SpecApproved anywhere (grep for SpecApproved/spec_approved returns nothing) — the kind exists only in code; the doc amendment was never applied."
      },
      {
        "file": "specs/system/authorization-provenance.md",
        "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
        "detail": "Unmet supersede row: a canonical spec still cross-references ralph-loop.md (meta-spec-reconciliation.md likewise carries a meta/process/ralph-loop.md@sha example) — 'all cross-references should be updated' was not carried out (agent-gates.md itself was updated)."
      },
      {
        "file": "docs/agent-protocol.md",
        "quote": "This document covers the full agent lifecycle: spawn, operation, and completion.",
        "detail": "Unmet supersede row: docs/agent-protocol.md still exists and documents the full agent lifecycle this spec claims to consolidate — never removed or folded in; the assigned task-311 and task-272 deliverables edit this exact file, so retirement needs sequencing or retargeting."
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "quote": "/api/v1/repos/:id/archive",
        "detail": "Unmet amendment row (abac-policy-engine.md): the 'archive' action was never added to the ABAC action set — the archive/unarchive routes sit in the frozen exemption file (per-handler enforcement) and 'archive' appears nowhere in abac_middleware.rs."
      },
      {
        "file": "crates/gyre-domain/src/agent.rs",
        "quote": "pub current_task_id: Option<Id>,",
        "detail": "Deviation from the amendment row (Agent entity): the spec says 'Add task_id: Id field — every agent is bound to exactly one task'; the code uses a nullable current_task_id: Option<Id> (assigned at spawn, but not the non-optional binding the row defines)."
      },
      {
        "file": "specs/system/hierarchy-enforcement.md",
        "quote": "### In-Memory meta_spec_sets",
        "detail": "Unmet amendment row (hierarchy-enforcement.md §4): the doc was never amended — it still describes the retired in-memory meta_spec_sets model and its route table / tenant-filter configuration contain no compute-target or meta-spec CRUD routes or the meta_spec_versions/meta_spec_bindings/compute_targets tables (the tables exist in the adapters)."
      },
      {
        "file": "specs/system/meta-spec-reconciliation.md",
        "quote": "PUT /api/v1/workspaces/{id}/meta-spec-set",
        "detail": "Contradiction left in place: meta-spec-reconciliation.md still specces the meta-spec-set endpoint that this spec's amendment table replaces (the code retired it — the endpoints 404 — but the canonical doc was never amended), and human-system-interface.md §5 likewise lacks the meta_specs_used array this spec says to add."
      },
      {
        "file": "crates/gyre-domain/src/task.rs",
        "quote": "pub enum TaskType {",
        "detail": "Satisfied amendment row (platform-model.md §3 Task): TaskType (Implementation/Delegation/Coordination) plus order, depends_on and parent_task_id all exist on the Task entity and drive the scheduler discriminators."
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "MessageKind::SpecApproved,",
        "detail": "Satisfied amendment row (code side): the SpecApproved kind exists with Event tier, server-only flag and the specced payload — only the message-bus.md doc amendment is missing."
      },
      {
        "file": "crates/gyre-domain/src/workspace.rs",
        "quote": "pub system_prompt: String,",
        "detail": "The Persona struct's prompt field is named system_prompt (not prompt) and the struct carries no required flag — the row's 'Persona struct gains required: bool' was implemented instead on the unified MetaSpec registry (see a00a6b5082295555260842b2)."
      }
    ]
  }
]
