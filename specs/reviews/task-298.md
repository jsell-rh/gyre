# Review: task-298 — Remove orchestrator agent-spawning and carry out the spec amendment table

**Candidate:** `de92c2af72f344f7f7f2943bed3174a52dd54320`
**Assigned base:** `0b33cb3a4a586c587e2566ecbcd82c03da94d84b` (branch fork point `196f1b20`)
**Verdict:** **Approved** — independent evidence supports the task contract.

## What was reviewed

The candidate's own changes (`196f1b20..de92c2af`, 44 files) against the
task's six required behaviors, plus the carried dependency clauses
(task-302). Full probe log: `/tmp/stage/review-evidence/task-298-review.md`.

## Findings — none blocking

1. **gyre_spawn_worker fully removed.** Catalog entry, `validate_tool_scope`
   arm, tools/call dispatch, `needs_write` membership, preview-registry
   membership, prompt references (api/prompts.rs, api/spawn.rs), docs, and
   the old tests are gone; the handler is deleted. The remaining
   repo-orchestrator-reachable MCP tools cannot create agent records
   (`gyre_spawn_repo_orchestrator` requires a workspace-orchestrator token,
   mcp.rs:1487-1495); the decomposition machinery (signal_chain.rs) is
   untouched. New negative test `mcp_spawn_worker_removed_from_catalog_and_dispatch`
   uses a real repo-orchestrator JWT against the production router and
   asserts both catalog exclusion and the unknown-tool error.
   `check-mcp-write-tools.sh` stays green (7 gated tools).
2. **Personas routes removed; registry replacement is real.** Module
   deleted, zero route registrations (axum 404s structurally), resolver
   entries and the persona entity-attribute arm removed, exemption-file
   count only shrank. `MetaSpec` carries model/temperature/max_tokens/budget
   (migration 000072 — next sequence, SQLite+PG portable; both adapters
   round-trip, adapters tests pass), kind-gated with a real 400 on
   non-persona kinds. `sync_persona_from_meta_spec` materializes or adopts
   the Persona read model (same id, or slug+scope adoption of the
   startup-seeded built-ins), mirrors approval, never fabricates scope
   identities (scoped entry without scope_id → warn+skip). Spawn-time
   binding is proven end-to-end by `registry_meta_persona_is_spawn_bindable`
   (registry POST → Pending persona row → spawn refused → registry approval
   + operational-field edit mirrored → spawn binds by derived slug,
   persona_id == meta-spec id). CLI bootstrap migrated (223 CLI tests pass,
   incl. `registry_persona_names_derive_to_builtin_slugs` pinning the
   adoption invariant); web dead methods removed (2596 frontend tests pass).
   The round-2 repair (case-insensitive `approval_status` at the API edge)
   is unit-pinned and exercised by the e2e payload.
3. **`archive` action is real policy evaluation.** Added to the action
   vocabulary (PERMISSION_ACTIONS + the spec's action-attribute table),
   resolver mappings for both routes, exemption entries removed (38→36).
   `archive_action_deny_blocks_repo_archive_through_middleware` seeds a real
   Deny policy through the policy port and proves 403 through the
   middleware stack on both routes before any handler runs. Non-Admin
   default-deny is spec-correct: repo-lifecycle.md §270-271 requires
   `action_override: "archive"` (Admin); the per-handler tenant-containment
   checks remain as defense-in-depth. `check-abac-route-registry.sh` green.
4. **Ralph-loop supersession carried out.** specs/system/ralph-loop.md
   deleted; the only remaining `ralph-loop.md` hits are the canonical
   supersession statement itself, historical task/milestone/coverage
   records; docs/ has zero hits; authorization-provenance.md re-pointed to
   agent-runtime.md §1.
5. **hierarchy-enforcement.md amendments present** (§3 tenant-filter rows
   for meta_spec_versions/meta_spec_bindings/compute_targets with isolation
   notes; corrected KvJsonStore note; §4 route-table rows with the
   auth-context notes), matching the real resolver mappings and per-handler
   gates.
6. **Dependency clauses verified at this head**: no meta-spec-set route
   registrations, `retired_meta_spec_set_endpoints_return_404`, no
   production writer to the old KV stores, merge-time drift compares
   `meta_specs_used` (`mr_recorded_meta_specs_used`).

Mechanical guards: all green (arch, migration versions/portability,
mem-port contracts, exempt handlers, dead message kinds, byte-slice,
scope-literal, inert-enforcement, forged-scope, fabricated-scope,
lossy-secret, relative-path, fail-open-ref). `cargo check -p gyre-server`
passes. The commit-attribution guard fails only against the review stage's
working-tree reset of task-298.md; the committed candidate's frontmatter
lists all three product commits.

## Notes for the verifier / controller (non-blocking)

- **Integration must merge, not apply the base→candidate diff.** The branch
  forked from `196f1b20`; the assigned base additionally contains task-282
  (/api/v1/deps/* surface) and task-340 (task-333..340 backlog). The
  candidate's own commits never touched those files, so a 3-way merge
  preserves them; expect conflicts in api/mod.rs.
- gyre-server test binaries exceed the review sandbox budget — the exact
  test list to run is recorded in
  `/tmp/stage/review-evidence/task-298-review.md` (spawn-worker removal,
  archive Deny, registry-persona spawn binding, registry approval denial,
  retired meta-spec-set 404s, tenant persona seeding, approval-status
  parsing; then the full suite).
- Minor observations, none blocking: no dedicated 404 pin test for the
  removed /api/v1/personas routes (behavior is structural — unregistered
  paths 404 — and the replacement surface has a strong e2e test);
  `/api/v1/policies/effective`'s sample action list omits `archive`
  (cosmetic; the permissions://me vocabulary has it); a stale
  "route is ABAC-exempt" comment inside `list_meta_specs_registry`
  contradicts its own newer comment (route is resolver-mapped);
  `specs/development/ralph-loops.md` (a distinct pre-existing file already
  marked superseded in specs/index.md) remains — outside this task's
  retirement scope; registry personas support Global/Tenant/Workspace
  scopes only, so repo-scoped personas are no longer HTTP-creatable —
  inherent to the spec's "replaced entirely by /api/v1/meta-specs?kind=meta:persona"
  amendment, and the read model still resolves repo-scoped rows.
