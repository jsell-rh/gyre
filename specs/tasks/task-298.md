---
title: "Remove orchestrator agent-spawning and carry out the spec amendment table"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / orchestrator-boundary-amendments / 4a3f940495a8"
depends_on: [task-302]
progress: ready-for-review
commits: ["071353fe866168d84e605b15c84108e083c2844f", "1167df20681ee579878dbf704587a97befec7633", "e822f5417041018c32c286a22e87664b47c175d4"]
---

## Required behavior

Per agent-runtime.md §1 Phase 3 ('The repo orchestrator only creates tasks. It never spawns agents.') and the Relationship to Existing Specs amendments table. (1) Remove the gyre_spawn_worker MCP tool entirely — catalog entry, tools/call dispatch, scope validation, prompt references (api/prompts.rs, api/spawn.rs) and tests — so no orchestrator principal can create agent records through any MCP surface; the mechanical scheduler is the only task→agent path. The existing decomposition machinery (Delegation detection, per-repo mutex one-live semantics, sub-tasks carrying spec_ref/parent_task_id/order/depends_on, LLM judgment with deterministic fallback) is preserved unchanged. The needs_write RBAC gate membership derives from handler effects, so removing the handler removes the membership (scripts/check-mcp-write-tools.sh stays green). (2) Remove the /api/v1/personas routes (CRUD, resolve, approve) — 'replaced entirely by /api/v1/meta-specs?kind=meta:persona'. The meta-spec registry persona kind must carry the Persona operational fields (model, temperature, max_tokens, budget) so they remain editable after the personas routes are removed, and spawn-time persona resolution/binding keeps working against registry-backed persona data; all consumers (web client, CLI, tests, ABAC/RBAC registrations for the removed routes) are updated, and the frozen exemption-file count only shrinks. (3) Add the 'archive' action to the ABAC action vocabulary and register the repo archive/unarchive routes with the middleware resolver (removing their entries from scripts/abac-route-registry-exemptions.txt) so archive/unarchive get real policy evaluation, keeping scripts/check-abac-route-registry.sh green. (4) Supersession cleanup per the spec's own directive: update every remaining live cross-reference to ralph-loop.md in specs/ and docs/ to reference agent-runtime.md §1 (e.g. specs/system/authorization-provenance.md) and retire specs/system/ralph-loop.md as fully superseded — carrying out the canonical spec's supersession statement, not altering any contract. (5) hierarchy-enforcement.md §4 amendment carry-out: add the compute-target and meta-spec CRUD routes to its route table with their auth-context notes (compute-target routes use tenant context from auth, no path param; meta-spec routes use scope_id from query param/body), add meta_spec_versions/meta_spec_bindings/compute_targets to its tenant-filter configuration, and correct the stale KvJsonStore note that still lists compute_targets among tenant-less KV namespaces (it is a tenant_id-bearing table since migration 000033). (6) The meta-spec-reconciliation.md row's registry-replacement clauses — retiring the workspace/tenant meta-spec-set endpoints, migrating their consumers to the DB-backed registry model, meta_specs_used-based drift detection, and {meta_spec_id, kind, content_hash, version} snapshot tuples — are delivered by bindings-registry-cascade, on which this task depends; this task's close-out verifies those clauses are carried out (its acceptance includes them), so the amendments-table gap closes only when every row is done.

## Acceptance Criteria

- [ ] The MCP tools catalog no longer lists gyre_spawn_worker and tools/call with that name returns an error; a repo-orchestrator JWT has no MCP tool whose handler creates agent records.
- [ ] GET/POST/PUT/DELETE /api/v1/personas and /api/v1/personas/resolve return 404; persona creation, editing (including operational fields and approval), and spawn-time persona resolution work via the meta-spec registry endpoints with kind=meta:persona.
- [ ] A Deny policy with action 'archive' on a repo resource blocks repo archive for the affected principal through the middleware (real policy evaluation), and the repo archive/unarchive routes are covered by RouteResourceMapping entries with no exemption-file entries.
- [ ] grep for 'ralph-loop.md' across specs/ and docs/ returns no live normative cross-references; specs/system/ralph-loop.md no longer exists as a live spec.
- [ ] hierarchy-enforcement.md's route table and tenant-filter configuration name the compute-target and meta-spec CRUD routes/tables with their auth-context notes, and its KvJsonStore note no longer misdescribes compute_targets storage.
- [ ] Carried from the dependency: PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set return 404/405 with no production writer to the meta_spec_sets/tenant_meta_spec_sets stores remaining, and merge-time drift enforcement compares merge-attestation meta_specs_used against current required meta-spec versions.

## Verification intent

Mint a repo-orchestrator JWT and call MCP tools/call gyre_spawn_worker with a valid implementation task id — at the base this spawns a real worker agent through spawn_agent_core (worktree, scoped JWT, compute-target resolution), exactly the 'orchestrator spawns agents' capability the spec removes ('Remove agent.spawn MCP tool — orchestrators do not spawn agents'); after the change the tool must not exist and no agent record may be creatable by an orchestrator principal through any MCP surface. At the same base the amendments table is otherwise un-carried-out, all provable in one sweep: GET /api/v1/personas returns 200 (routes still registered at api/mod.rs:1002-1013), a Deny policy with action 'archive' never blocks repo archive because the archive/unarchive routes sit in the frozen exemption file (scripts/abac-route-registry-exemptions.txt:30,34) and the ABAC action vocabulary has no 'archive' action, hierarchy-enforcement.md contains zero compute-target/meta-spec route-table or tenant-filter entries while its KvJsonStore note still lists compute_targets as tenant-less, and specs/system/ralph-loop.md still exists with live cross-references (e.g. specs/system/authorization-provenance.md).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "15aec83b58b3494602d31ce8",
    "decision": "gap",
    "requirement_quote": "The repo orchestrator **only creates tasks**. It never spawns agents. The system handles the task→agent mapping mechanically.",
    "detail": "The decomposition machinery is implemented: the task scheduler detects Delegation tasks by task_type, spawns the repo orchestrator under a per-repo mutex (repo-orchestrator persona, repo-scoped JWT, one-live 409); decompose_delegation fails hard on an unresolvable spec_ref, reads the spec at the pinned SHA, uses LLM decomposition judgment when an LLM is configured (deterministic fallback otherwise), creates Implementation sub-tasks carrying spec_ref, parent_task_id, order and depends_on, and marks the delegation task Done. GAP: the 'never spawns agents' boundary is contradicted by a live production mechanism — the dispatched MCP tool gyre_spawn_worker lets any repo-orchestrator JWT spawn a worker agent directly through spawn_agent_core (worktree, scoped JWT, compute-target resolution). That is exactly the orchestrator-spawns-agents capability the spec removes (see also the amendments table: 'Remove agent.spawn MCP tool — orchestrators do not spawn agents'); the mechanical scheduler is not the only task→agent path available to an orchestrator. Secondary mechanism deviation: sub-tasks are created via the tasks port (state.tasks.create) rather than the task.create MCP tool.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"description\": \"Spawn a worker agent for an implementation task in the caller's repo. Requires a repo-orchestrator JWT. Delegates to the standard agent spawn flow (worktree, scoped JWT, compute target resolution).\",",
        "detail": "gyre_spawn_worker is advertised in the MCP tool catalog specifically to repo-orchestrator JWT holders — the repo orchestrator can spawn worker agents directly, contradicting 'It never spawns agents. The system handles the task→agent mapping mechanically.'"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
        "detail": "The tool is dispatched in the production tools/call path (not dead catalog text); handle_spawn_worker builds a SpawnAgentRequest and calls spawn_agent_core under the repo-orchestrator's own authentication."
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "quote": "Some(TaskType::Delegation) => decompose_delegation(state, repo, task).await?,",
        "detail": "Positive wiring: the scheduler routes Delegation tasks to the repo orchestrator's decomposition, which creates ordered Implementation sub-tasks and completes the delegation task — the specced Phase-3 flow itself is real."
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "quote": "sub.task_type = Some(TaskType::Implementation);",
        "detail": "Sub-tasks are created with the Implementation discriminator together with spec_ref, parent_task_id, order and depends_on (the same creation block sets sub.spec_ref/sub.parent_task_id/sub.order/sub.depends_on), via the tasks port rather than the task.create MCP tool — a mechanism deviation in how sub-tasks are created, with all specced fields present."
      }
    ]
  },
  {
    "id": "a40446f46dbb0536f3c30ed0",
    "decision": "gap",
    "requirement_quote": "Remove `agent.spawn` MCP tool — orchestrators do not spawn agents. They create tasks via `task.create`. The system spawns agents mechanically in response to new implementation tasks.",
    "detail": "The amendments table is dispositioned row by row. Implemented: platform-model.md §3 Task (task_type Implementation/Delegation/Coordination, order, depends_on, parent_task_id); message-bus.md MessageKind::SpecApproved (server-only Event tier, distinct from SpecChanged); platform-model.md §1 Workspace compute_target_id; the Agent entity amendment (AgentStatus enum + status field + task binding as current_task_id: Option<Id> — workers are bound mechanically by the scheduler; orchestrators are deliberately taskless, a documented Option deviation from the specced 'task_id: Id'); agent-gates.md (the approve handler emits SpecApproved after recording the ledger row; MergeAttestation gains meta_specs_used, covered by the EdDSA signature and populated from the spawn-time prompt-set record); abac-policy-engine.md meta_spec and compute_target resource types; human-system-interface.md §1 (the UI meta-specs surface fetches the registry API); and the spec-lifecycle.md stale-terminology replacement ('CEO agent' no longer occurs anywhere in the specs tree). Contradicted or missing: (1) 'Remove agent.spawn MCP tool — orchestrators do not spawn agents' — the live gyre_spawn_worker tool lets a repo-orchestrator JWT spawn worker agents directly (see Phase 3); (2) 'The /api/v1/personas endpoints ... are removed — replaced entirely by /api/v1/meta-specs?kind=meta:persona' — the full personas CRUD plus approve routes remain registered and the Persona entity is still bound to agents at spawn, coexisting with the meta-spec registry (whose CRUD lives at /api/v1/meta-specs-registry, not the specced path — see API); (3) 'Add archive to the action attribute table' — the ABAC action vocabulary contains no 'archive' action and the repo archive/unarchive routes run without middleware ABAC (frozen exemption-file entries with per-handler authorization only); (4) the meta-spec-reconciliation.md row's registry-replacement clauses are not carried out: the old path@sha-pinned meta-spec-set model is fully alive production code — PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set upsert MetaSpecSet{personas: BTreeMap<String, MetaSpecPinnedEntry{path, sha}>, principles, standards, process} into the meta_spec_sets/tenant_meta_spec_sets KV stores, trigger old-model reconciliation (diff_meta_spec_set → run_reconciliation, emit_meta_spec_set_updated) and feed resolve_effective_meta_spec_set, the §10 conformance sweep (run_conformance_sweep compares compute_meta_spec_set_sha of the path@sha set against the provenance-recorded meta_spec_set_sha) and merge-time drift enforcement (mr_meta_spec_drift compares the old-model set SHA, not meta_specs_used); MetaSpecSnapshot does not exist in the amended {meta_spec_id, kind, content_hash, version} form anywhere (grep across crates/: zero matches — the operative pin type remains MetaSpecPinnedEntry{path, sha}), and reconciliation.rs never references meta_specs_used — the specced replacement surface (required flags + spec-level bindings via the meta-spec API) coexists with the old endpoint instead of replacing it, the same amendment class as agent.spawn and /api/v1/personas; (5) the hierarchy-enforcement.md §4 row is not carried out in the amended document: it contains no compute-target or meta-spec CRUD route-table entries and no meta_spec_versions/meta_spec_bindings/compute_targets entries in its tenant-filter configuration, while its KvJsonStore note still lists compute_targets among tenant-less KV namespaces, contradicting the migrated tenant_id-bearing table (migration 000033) — the production substance largely exists, making the un-amended spec text stale rather than pending. Additionally the supersession directive that all cross-references to ralph-loop.md in other specs be updated to agent-runtime.md §1 is not carried out (e.g. specs/system/authorization-provenance.md still cites ralph-loop.md, and specs/system/ralph-loop.md still exists).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"gyre_spawn_worker\" => handle_spawn_worker(&state, &args, &auth).await,",
        "detail": "Amendment (a) contradicted: an agent-spawning MCP tool exists and is available to repo-orchestrator JWTs — orchestrators can spawn agents, which the amendment removes."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/personas\",",
        "detail": "Amendment (h) contradicted: the /api/v1/personas endpoints (create/list/resolve/get/put/delete/approve) are still routed rather than removed — both persona surfaces coexist."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"read\", \"write\", \"delete\", \"approve\", \"spawn\", \"push\", \"merge\", \"escalate\", \"generate\",",
        "detail": "Amendment (g) missing: the ABAC action vocabulary has no 'archive' action, and the archive/unarchive routes bypass middleware ABAC via the frozen exemption file, so the amendment's action-table addition is absent."
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "quote": "/api/v1/repos/:id/archive",
        "detail": "The archive route runs without middleware ABAC (exemption-file entry, per-handler authorization only) — the concrete state behind the missing 'archive' action."
      },
      {
        "file": "specs/system/authorization-provenance.md",
        "quote": "- **Ralph Loop** (`ralph-loop.md`): Re-spawn produces `DerivedInput` from same parent, preserving constraint chain (§4.2).",
        "detail": "The supersession directive 'All cross-references to ralph-loop.md in other specs ... should be updated to reference agent-runtime.md §1' is not carried out — other canonical specs still cite the superseded document."
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "SpecApproved,",
        "detail": "Positive wiring for the message-bus amendment: MessageKind::SpecApproved exists (server-only Event tier) with the specced payload, distinct from SpecChanged."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/workspaces/:id/meta-spec-set\",",
        "detail": "Amendment (4) — the row's 'PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API' is not enacted: the old PUT/GET meta-spec-set route (and the tenant twin '/api/v1/tenants/:id/meta-specs/set' at the adjacent registration) is still live production routing alongside the new registry/bindings surface."
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "pub struct MetaSpecPinnedEntry {",
        "detail": "Amendment (4) — 'The MetaSpecSnapshot from reconciliation spec uses content_hash ... struct fields become {meta_spec_id, kind, content_hash, version} tuples instead of path@sha strings': MetaSpecSnapshot does not exist in that form anywhere in crates/ (grep: zero matches); the operative pin type remains MetaSpecPinnedEntry{path, sha} keyed by git-spec path."
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "let summary = crate::reconciliation::run_reconciliation(",
        "detail": "Amendment (4) — a meta-spec-set PUT still drives the old-model reconciliation machinery (diff_meta_spec_set over path@sha pins → emit_meta_spec_set_updated → run_reconciliation), the exact flow the amendment replaces with required-flag changes plus reconciliation triggered on meta-spec approval."
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "quote": "let current_set_sha = crate::compute_meta_spec_set_sha(state, &mr.workspace_id).await;",
        "detail": "Amendment (4) — 'Drift detection compares the meta_specs_used array in merge attestations against current required meta-spec versions': merge-time drift (mr_meta_spec_drift) and the §10 conformance sweep compare the old-model set SHA (compute_meta_spec_set_sha vs the provenance-recorded meta_spec_set_sha), never the meta_specs_used array; reconciliation.rs contains no reference to meta_specs_used."
      },
      {
        "file": "specs/system/hierarchy-enforcement.md",
        "quote": "**KvJsonStore gap:** The generic `KvJsonStore` (used for agent_tokens, compute_targets, abac_policies, etc.)",
        "detail": "Amendment (5) — the hierarchy-enforcement.md §4 directives are absent from the amended document: its route table contains no compute-target or meta-spec CRUD entries (no 'compute-target'/'meta-spec' string occurs in the file outside this note), its §3 tenant-filter configuration lists no meta_spec_versions/meta_spec_bindings/compute_targets tables, and this KvJsonStore note still lists compute_targets among tenant-less KV namespaces — contradicting the migrated tenant_id-bearing table."
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-26-000033_compute_targets/up.sql",
        "quote": "CREATE TABLE compute_targets (",
        "detail": "Amendment (5) — the production substance exists: compute_targets is a real tenant_id-bearing table (UNIQUE(tenant_id, name), with workspaces.compute_target_id referencing it), so hierarchy-enforcement.md's un-updated route table and tenant-less-KV note are stale spec text, not pending work."
      }
    ]
  }
]

## Shipped

Progress: `ready-for-review` — implementation complete on this branch
(checkpoints `071353fe`, `1167df20`; repair round `e822f541`). Independent
verification pending: gyre-server test binaries exceed this sandbox's 4 GiB
build budget; the exact command list is recorded in
/tmp/stage/review-evidence/task-298-repair-round2.md.

### Amendment-table carry-out (checkpoints 071353fe + 1167df20)

1. **gyre_spawn_worker removed**: MCP catalog entry, tools/call dispatch,
   scope validation, prompt references (api/prompts.rs, api/spawn.rs), and
   tests. No MCP tool handler available to a repo-orchestrator JWT creates
   agent records; the mechanical Phase-4 scheduler is the only task→agent
   path. Decomposition machinery untouched.
   Test: `mcp_spawn_worker_removed_from_catalog_and_dispatch` (mcp.rs).
2. **/api/v1/personas removed; registry carries personas**: routes/module
   gone (404); MetaSpec carries the Persona operational fields
   model/temperature/max_tokens/budget (migration 000072, SQLite+PG+mem
   adapters); `sync_persona_from_meta_spec` materializes/adopts the Persona
   read model spawn-time resolution consumes. Consumers migrated:
   - CLI bootstrap: find/create/approve via `/api/v1/meta-specs` with
     `kind=meta:persona`; `bootstrap::registry_persona_name` registers a
     name whose derived slug equals the built-in slug so the sync ADOPTS
     the startup-seeded persona (unit test
     `registry_persona_names_derive_to_builtin_slugs`; 25 CLI bootstrap
     tests pass).
   - Web client: dead `/personas` methods removed from api.js (MetaSpecs
     component already used the registry surface).
   - Server tests: `create_tenant_seeds_builtin_personas` verifies through
     the persona port; the abac live test whose fixture was the removed
     approve endpoint is replaced by
     `persona_approval_via_registry_denies_agent`.
   - New e2e test `registry_meta_persona_is_spawn_bindable` (spawn.rs):
     registry POST with operational fields → synced Pending persona →
     spawn refused while Pending → registry approval + operational-field
     edit mirrored to the read model → spawn binds the registry-created
     persona by derived slug.
3. **archive ABAC action**: action vocabulary + resolver mappings for repo
   archive/unarchive; exemption-file entries removed (frozen count
   38→36). Test: `archive_action_deny_blocks_repo_archive_through_middleware`.
4. **Ralph-loop supersession**: `specs/system/ralph-loop.md` and its
   coverage file deleted; authorization-provenance.md and the
   meta-spec-reconciliation.md example re-pointed to agent-runtime.md §1;
   remaining `ralph-loop.md` grep hits in specs/ and docs/ are only
   agent-runtime.md's own canonical supersession statement and historical
   task/coverage/milestone records.
5. **hierarchy-enforcement.md §3/§4 amendments**: compute-target and
   meta-spec CRUD route families added to the §4 route table with
   auth-context notes; meta_spec_versions/meta_spec_bindings/
   compute_targets added to the §3 tenant-filter configuration; KvJsonStore
   note corrected (compute_targets is a tenant_id-bearing table since
   migration 000033).
6. **Dependency close-out (task-302 clauses)**: no workspace/tenant
   meta-spec-set routes remain; no production writer to
   meta_spec_sets/tenant_meta_spec_sets; merge-time drift enforcement
   compares merge-attestation meta_specs_used against current required
   meta-spec versions (verified on this head; test
   `retired_meta_spec_set_endpoints_return_404`).

### Repair round (e822f541) — review findings 1 and 2

`GyreClient::approve_persona` sent `{"approval_status": "approved"}`
(lowercase) while the server parsed approval_status case-sensitively, so
the admin-token CLI bootstrap aborted step 5 with 400 and never converged,
and the deferred e2e test `registry_meta_persona_is_spawn_bindable`
asserted OK on the same invalid payload. Fix:

- `parse_approval_status` (api/meta_specs.rs) now lowercases at the API
  edge — the same leniency `parse_kind`/`parse_scope` already apply; the
  domain `MetaSpecApprovalStatus::parse` stays case-sensitive for
  DB-stored canonical values. New unit test
  `approval_status_parse_accepts_any_case` pins the leniency.
- `approve_persona` (CLI client.rs) now sends the canonical `"Approved"`.
- The spawn.rs e2e payload stays lowercase deliberately, documented as
  pinning the edge case-insensitivity end-to-end.

### Test evidence (in-sandbox, via build wrapper)

- `cargo check -p gyre-server -p gyre-cli` — exit 0.
- `cargo check -p gyre-server --tests` — exit 0 (type-checks the new and
  edited tests without producing binaries).
- `cargo test -p gyre-cli` — 223 + 1 passed, 0 failed.
- `scripts/check-mcp-write-tools.sh`, `scripts/check-abac-route-registry.sh`,
  `scripts/check-abac-exempt-handlers.sh`, `scripts/check-arch.sh` — all OK.

Deferred to the independent verifier (exact list in
/tmp/stage/review-evidence/task-298-repair-round2.md):
`approval_status_parse_accepts_any_case`,
`registry_meta_persona_is_spawn_bindable`,
`mcp_spawn_worker_removed_from_catalog_and_dispatch`,
`archive_action_deny_blocks_repo_archive_through_middleware`,
`persona_approval_via_registry_denies_agent`,
`create_tenant_seeds_builtin_personas`,
`retired_meta_spec_set_endpoints_return_404`, the full `-p gyre-server`
suite, `--test git_integration` (`push_to_archived_repo_rejected`), and a
real-server `gyre bootstrap` twice (fresh + resume-with-Pending) to prove
step 5 convergence.
