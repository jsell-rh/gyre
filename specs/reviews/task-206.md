# Review — task-206: meta-spec preview mode (§5)

**Candidate:** 72982255228aca379cfd60f81cfe50fed8c77275
**Base:** f70cfac6f829847f994daf0cb96631503b1a1561 (merge-base 97062a09)
**Verdict:** needs-revision — one budget-accounting defect; the rest of the §5 contract
is genuinely implemented and independently probed.

## What was verified (independent evidence in /tmp/stage/review-evidence)

- **Real agent run path.** `POST /api/v1/meta-specs/preview` (global route, mod.rs:882)
  validates every target up front (repo exists → 404, repo ABAC + tenant containment →
  403, spec_path on default branch → 400), reserves budget per workspace
  (`check_spawn_budget_for`) or via the independent `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`
  cap, then provisions per target a real Active agent row, a `preview/{preview_id}/{slug}`
  branch pinned to the default-branch tip, a real git worktree, and a short-lived scoped
  JWT (default 1800 s). 202 response is the exact spec shape. Provision/persist failures
  roll back with correct slot accounting (the untaken-slot repair 8212fa5d is in place).
- **Draft reaches the agent.** `GYRE_META_SPEC_DRAFT_KIND/CONTENT` +
  `GYRE_TARGET_SPEC_PATH` env injection (merged before server-owned vars, cannot shadow
  identity/credential env); `agent-runner.mjs` builds the preview prompt with the draft as
  governing context over the target spec, withholds task-mutating MCP tools, pushes to the
  preview branch, exits without provenance upload or completion call. The hard test
  (`preview_spawns_real_agents_with_branches_worktrees_and_draft`) asserts the literal
  draft text in the process environment dump.
- **Skip-ceremony enforced server-side**, not just client-side: MCP dispatch refuses
  task/MR/complete/provenance tools for preview agents; 21 REST surfaces (task/MR/merge
  queue/complete/stop/fail/spec approve/compose/orchestrator spawn/agent registration/
  status/usage/provenance) refuse preview-agent callers, keyed on the array-scoped
  Gyre-agent-JWT discriminator + preview registry (the dead-guard repair 0513d409 is in
  place and unit-pinned).
- **Completion semantics.** Every compute-target monitor routes process exit through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree removed,
  slot released, row Stopped — never Idle; branch survives for diffing. `GET
  /api/v1/meta-specs/preview/{id}` reports per-agent state from the real agent rows and
  the produced diff (branch vs pinned base sha).
- **Cleanup.** `DELETE` (404 unknown) kills agents via the recorded spawn backend (not
  handle shape — repair c33a86e2), force-removes worktrees, deletes branches, closes
  dangling MRs, drops records. `meta_spec_preview_gc` job registered (jobs.rs:705,
  spawned in main.rs:151) with TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` (default 24 h) and
  an orphan pass; TTL boundary tested (at/past deleted, just-under retained).
- **Clean cutover.** Workspace-scoped preview routes, hollow blast-radius preview
  response, `compute_preview_blast_radius`/`StructuralImpact` all removed; ABAC resolver
  has mappings for the new global routes; the two removed exemption entries are
  legitimate (routes gone).
- **Docs.** api-reference.md (routes + full preview-mode section) and server-config.md
  (TTL/JWT/GC/budget knobs) updated.
- **Probes run here:** `cargo check -p gyre-server` exit 0 (7m28s);
  `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-inert-enforcement.sh`, `check-mem-port-contracts.sh` all green; web `npm ci` +
  full vitest suite 2572 passed / 41 skipped (MetaSpecs + EditorSplit preview-loop tests
  included). Gyre-server test compilation is blocked by this sandbox's 4 GiB wrapper —
  see deferred probes below.
- **Pipeline-script change d28e4a32** (verifier overlay restoring merged exemption data)
  was scrutinized as potential verification tampering: it is safe — dev-check.sh's
  exemption-growth gate compares `HEAD` vs `HEAD^1` directly via git and independently
  forbids any added entry, so the overlay cannot weaken the upstream phase; the change
  ships with regression tests and only affects files present in both trees.

## Finding (blocking)

**Preview budget slot double-release via generic terminal paths** —
`release_preview_slot` (meta_specs.rs:2126) keys the release-once guarantee solely on the
preview record's `released` flag, but the pre-existing generic terminal paths decrement
the same workspace/tenant `active_agents` counter for preview agents without setting that
flag: `POST /agents/{id}/stop` and `/fail` (spawn.rs:3522/3352 — neither kills the
process, so the exit monitor's `finish_preview_agent` later releases the same slot again),
admin kill (admin.rs:437 — consumes the registry handle, so the later DELETE/"Clear" or GC
sweep releases it again), and budget-enforcement stop (budget.rs:1494, same delayed
double-release). Each occurrence permanently biases the persistent counter down by one,
letting later spawns over-admit past `max_concurrent_agents` — the exact class the
candidate's own comments call out as blocking, and the mirror image of the untaken-slot
repair it already shipped. Full source trace and reproduction:
/tmp/stage/review-evidence/finding-budget-double-release.md.

## Deferred to the independent verifier (blocked by the 4 GiB build wrapper here)

- `cargo test -p gyre-server --lib preview_` (all preview tests, including
  `preview_spawns_real_agents_with_branches_worktrees_and_draft`,
  `preview_agent_finish_teardown_and_diff`, `preview_gc_ttl_boundary`,
  `preview_delete_tears_down_everything`, the REST/MCP refusal suites, the
  rollback/launch-failure suites, `caller_is_preview_agent_detects_array_scoped_jwt`).
- `cargo test -p gyre-server` (full package) and workspace-wide suite + Clippy per the
  standard verification phase.
- Suggested additional regression for the finding above: stop a preview agent via
  `POST /api/v1/agents/{id}/stop` in a workspace with another Active agent, let the
  process exit, and assert `active_agents` drops by exactly one (currently drops by two).
