# Review — task-206: meta-spec preview mode (§5)

**Candidate:** `a6ecd7a68d82dde10bd39ba89eb56d023e51a883` (base `f70cfac6f829847f994daf0cb96631503b1a1561`)
**Verdict:** approved — no blocking findings.
**Date:** 2026-12-17 (independent review round)

## Scope

`specs/system/meta-spec-reconciliation.md` §5 Preview Mode, per the task-206 contract:
real agent preview runs with branches, diffs, and cleanup; spec-conformant global routes;
skip-ceremony; budget; killed-on-completion; DELETE + TTL GC; clean cutover of the hollow
implementation; docs; hard tests.

## What was verified (independent evidence)

- **Routes / clean cutover:** `POST/GET/DELETE /api/v1/meta-specs/preview{,/:preview_id}`
  registered (mod.rs:882-888) with ABAC resolver entries; the workspace-scoped preview routes,
  `compute_preview_blast_radius`, `StructuralImpact`, and their stale exemption entries are gone
  (repo-wide grep clean). Frontend `api.js` cut over to the global routes.
- **Real spawn path:** per target — branch pinned to the default-branch tip, Active agent row
  scoped to the repo's workspace, real git worktree + row, short-lived preview JWT
  (`GYRE_PREVIEW_JWT_TTL_SECS`), budget slot incremented. Validation (repo exists, repo ABAC,
  tenant containment, spec-path on default branch) completes before anything is created;
  provision/persist failures roll back fully; launch verdict uses the dispatch `launched` flag
  (not handle shape). Response 202 is the spec shape exactly.
- **Draft injection:** `GYRE_META_SPEC_DRAFT_KIND/CONTENT`, `GYRE_TARGET_SPEC_PATH`,
  `GYRE_PREVIEW_ID`, no `GYRE_TASK_ID`; agent-runner.mjs builds the prompt with the draft
  meta-spec as governing context over the target spec and withholds task-mutating MCP tools.
- **Skip-ceremony:** MCP dispatch + REST guards (403) on task/MR/complete/provenance/status/
  merge-queue/compose/orchestrator/specs routes for preview-agent callers; no refs/agents/* or
  refs/tasks/* writes; never Idle — every monitor exit routes through `finish_preview_agent`.
- **Budget:** workspace budget with grouped per-workspace reservation (multi-target POST cannot
  over-admit) when `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` is unset; independent preview cap when
  set. Slot release is exactly-once across process-exit/DELETE/GC and every generic terminal
  path (stop/fail/admin kill/budget stop/stale detector) via a durable per-agent flag — the
  double-release bias class is regression-tested.
- **Status + diff:** per-agent state from the real agent rows; diff via `git_ops.diff(base_sha,
  branch)`; 404 unknown; scope containment re-checked on GET/DELETE.
- **Cleanup:** DELETE kills through backends recorded at spawn time (SshDocker/K8s-aware),
  removes worktrees/branches, closes dangling MRs, revokes tokens, drops kv records.
  `meta_spec_preview_gc` registered and spawned; TTL/GC-interval configurable; corrupt-record
  and orphan-agent passes; saturating age math.
- **Docs:** api-reference.md (routes + full §5 section) and server-config.md (4 knobs) updated.
  Coverage matrix row 9 flipped to implemented with an accurate description.

## Probes run here

- 9 mechanical gate scripts: exit 0 (ABAC registry/exempt-handlers, arch, mcp-write-tools,
  in-memory-state-stores, scope-literal-defaults, forged-scope-fields, inert-enforcement,
  migration-versions).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server`: exit 0.
- `cargo test -p gyre-adapters --lib`: 582 passed (incl. ssh `from_handle` round-trips, git2
  worktree/branch ops).
- `cargo test -p gyre-common -p gyre-domain -p gyre-ports`: passed.
- `web`: `npm ci` + `npm test`: 2572 passed / 41 skipped (127 files) — includes the §5 preview
  loop tests in MetaSpecs.test.js and EditorSplit.test.js.
- `cargo test -p gyre-server --lib meta_specs::tests::preview`: **blocked by the executor build
  wrapper** (4 GiB cap, exit 64) — sandbox restriction, not a code defect; recorded as a
  required verifier probe.

## Required verifier probes

1. `cargo test -p gyre-server --lib meta_specs::tests::preview` (the preview suite:
   spawn/branch/worktree/draft-injection, 202 shape, authz, skip-ceremony, DELETE teardown,
   GC TTL boundary, finish+diff, kill routing, SSH/K8s launch verdicts, budget caps, rollback,
   REST/MCP refusals, exactly-once slot release).
2. `cargo test -p gyre-server` (full regression: rbac preview guard, mcp ceremony guards,
   retargeted discover/constraint_check tests, jobs, stale_agents).
3. Full mechanical gate set on the final merged tree.

## Notes

- `scripts/verifier-script-overlay.py` change is pipeline infra and conservative: check logic is
  restored from base; only frozen `*-exemptions.txt` data follows the merge (entrywise subset of
  base — the two removed entries correspond to routes the candidate legitimately deleted;
  adding entries remains forbidden).
- `web/dist/` was not agent-rebuilt in this checkpoint; the deterministic verifier owns the
  rebuild. Generated-output observation only, non-blocking.
- Working-tree note: `specs/tasks/task-206.md` currently differs from HEAD (`progress:
  not-started`, `commits: []` vs the committed ready-for-review record) — that modification was
  present on entry to this review stage and was left untouched.
