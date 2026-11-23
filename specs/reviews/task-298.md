# Review — task-298 (Remove orchestrator agent-spawning and carry out the spec amendment table)

**Candidate:** `bf7e41faf2ab1c9b55a719950281ea617dc62f7b`
**Branch:** `pipeline/task-298/a0200e77adc548acb286cd3173e8825b-1`
**Assigned base:** `f3ca7db3bf7ee4b35cab550ad18011c13d8680c4` (merge-base `196f1b20`; the base is not an ancestor — the base→candidate tree diff additionally shows reversals of 34 base-side sibling-task commits, e.g. the /deps routes and task-333..342 spec files; those are divergence artifacts, not candidate changes. The candidate's own diff 196f1b20..bf7e41fa is cleanly scoped to task-298.)
**Verdict:** NOT APPROVED — one blocking finding.
**Date:** 2026-12-02

## What was verified (independently, on the candidate tree)

All six enumerated deliverables were inspected at source level and hold up:

- **gyre_spawn_worker removed** — catalog entry, tools/call dispatch, scope
  validation, prompt references and the old tests are gone from
  `crates/gyre-server/src/mcp.rs`; only the negative test
  `mcp_spawn_worker_removed_from_catalog_and_dispatch` references the name.
  `scripts/check-mcp-write-tools.sh` green (7 write tools gated, membership
  gone with the handler). Decomposition machinery (Delegation detection,
  per-repo mutex, ordered sub-tasks with spec_ref/parent_task_id/order/
  depends_on) untouched.
- **/api/v1/personas removed; registry carries personas** — routes and module
  deleted; `MetaSpec` carries model/temperature/max_tokens/budget
  (gyre-domain/src/meta_spec.rs:169-176, migration 000072, SQLite+PG adapters,
  portability check green); `sync_persona_from_meta_spec`
  (api/meta_specs.rs:8222) materializes/adopts the Persona read model with
  approval mirroring and operational-field sync, with fail-loud
  `reject_persona_fields_on_other_kinds`; web `api.js` persona methods removed
  (no component used them); CLI bootstrap migrated to
  `/api/v1/meta-specs?kind=meta:persona`.
- **archive ABAC action** — resolver mappings for repo archive/unarchive with
  action override `archive` (abac_middleware.rs:206-207), exemption-file
  entries removed (frozen count 38→36, only shrank), `archive` in
  PERMISSION_ACTIONS, Deny-through-middleware test present.
- **Ralph-loop supersession** — `specs/system/ralph-loop.md` deleted; remaining
  `ralph-loop.md` grep hits in specs/ and docs/ are the canonical supersession
  statement itself, references to the distinct `development/ralph-loops.md`
  spec, and historical task/milestone/coverage records — no live normative
  cross-references.
- **hierarchy-enforcement.md §3/§4** — compute-target and meta-spec CRUD route
  families in the route table with auth-context notes; the three tables in the
  tenant-filter configuration; KvJsonStore note corrected.
- **Dependency close-out (task-302 clauses)** — no workspace/tenant
  meta-spec-set routes; no production writer to the retired KV stores (only
  the distinct `agent_meta_spec_sets` prompt-set namespace remains);
  `mr_meta_spec_drift` compares merge-attestation `meta_specs_used`;
  `MetaSpecSnapshot` exists in the amended tuple shape.

Mechanical checks all green on the candidate: check-arch, check-abac-route-
registry, check-abac-exempt-handlers, check-mcp-write-tools, check-migration-
versions, check-migration-sql-portability, check-scope-literal-defaults,
check-mem-port-contracts. `cargo check -p gyre-server --lib` and
`cargo check -p gyre-cli` exit 0; `cargo test -p gyre-domain -p gyre-common`
passes. Full gyre-server suite cannot run in this sandbox (4 GiB build
budget) — the five round-4 repaired tests and the full suite are recorded as
required verifier commands in /tmp/stage/review-evidence/README.md.

## Blocking finding

**F1 (major): an orchestrator principal can still create agent records through
an MCP surface — `gyre_spawn_repo_orchestrator` remains live.**

Task-298's required behavior states the outcome of clause (1) as *"so no
orchestrator principal can create agent records through any MCP surface; the
mechanical scheduler is the only task→agent path"*, and the verification
intent repeats it: *"no agent record may be creatable by an orchestrator
principal through any MCP surface."* The candidate leaves
`gyre_spawn_repo_orchestrator` fully live:

- catalog entry (mcp.rs:545), tools/call dispatch (mcp.rs:5382-5384), scope
  validation admitting `workspace:{id}:spawn` tokens (mcp.rs:825);
- the handler (mcp.rs:1489) requires a workspace-orchestrator JWT and calls
  `spawn_repo_orchestrator_core` under the caller's own auth — creating the
  repo-orchestrator agent record and scoped token;
- the existing passing test `mcp_spawn_repo_orchestrator_happy_and_wrong_workspace`
  (mcp.rs:11866) proves the capability end-to-end through `/mcp` tools/call.

A workspace orchestrator is an orchestrator principal, so the required-
behavior outcome is not achieved. This contradicts the canonical spec the task
carries out — agent-runtime.md §1 Phase 2: *"The workspace orchestrator only
creates tasks (via `task.create` MCP tool). It never spawns agents, never
touches code, never interacts with compute targets"* — and Phase 3, which
assigns repo-orchestrator spawning to the task scheduler (signal_chain.rs),
plus the platform-model §4 amendments row: *"Remove `agent.spawn` MCP tool —
orchestrators do not spawn agents ... The system spawns agents mechanically."*

The candidate's shipped note acknowledges the tool remains but
mischaracterizes it as "the scheduler-side Phase-3 mechanism": it is a
caller-initiated MCP dispatch under the workspace orchestrator's JWT, not the
scheduler path. Consequence: coverage row 33's new claim that "every
amendments-table row [was] carried out" overstates the platform-model §4 row.

Scope fairness: the narrow acceptance criterion ("a repo-orchestrator JWT has
no MCP tool whose handler creates agent records") IS satisfied —
repo-orchestrator tokens carry only `repo:{name}:*` scopes and the handler's
tier check rejects them. The finding is specifically the workspace-orchestrator
tier. Fix: remove the `gyre_spawn_repo_orchestrator` MCP surface (catalog,
dispatch, scope validation, tests) exactly as `gyre_spawn_worker` was removed,
keeping `spawn_repo_orchestrator_core` for the scheduler and the
Admin/Developer-gated REST routes (human operators are not orchestrator
principals). If the project's intent is that workspace orchestrators keep this
capability, the remedy is amending agent-runtime.md §1 Phase 2 through the
spec lifecycle — not shipping the contradiction (goal rule 4).

## Notes for the verifier

- Required commands (recorded with rationale in
  `/tmp/stage/review-evidence/README.md`): full `cargo test --all`, plus the
  focused gyre-server lib tests `mcp_spawn_worker_removed_from_catalog_and_dispatch`,
  `archive_action_deny_blocks_repo_archive_through_middleware`,
  `persona_approval_via_registry_denies_agent`,
  `registry_meta_persona_is_spawn_bindable`,
  `permissions_resources_evaluate_real_policies_for_agent_jwt`,
  `mcp_spawn_repo_orchestrator_happy_and_wrong_workspace`, and `web: npm ci &&
  npm test`. The previous round's five failures and their repairs are
  documented in the candidate's task file and /tmp/stage/findings.log.
- Generated `web/dist/` was not evaluated (verifier-owned).
- No source files were modified during this review; all probes were read-only
  plus the mechanical check scripts and cargo check/test on smaller packages.
