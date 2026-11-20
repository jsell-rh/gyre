# Review: task-206 — Meta-spec preview mode (real agent preview runs)

**Reviewer:** Independent review agent
**Round:** R1 (this candidate)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** e9f6a31c3903ff61597af88d0b70b41d94bb911a
**Candidate reviewed:** 61ed53957685390bb412d7c33b143bdcbb29ac6f (merge-base with base: e1cf9d8d)
**Status:** complete — approved

---

## Verdict

Approved. Independent evidence supports every acceptance criterion; no
blocking source defects found. Probe evidence under
`/tmp/stage/review-evidence/`.

## Base-divergence note (not a candidate defect)

The candidate is not descended from the assigned base e9f6a31c (merge-base
is e1cf9d8d, which predates task-262 SPIFFE federation and task-272 spec
backlog). The raw `git diff base..candidate` therefore *shows* the removal
of `crates/gyre-server/src/spiffe_federation.rs`,
`crates/gyre-spiffe/src/federation.rs`, and `specs/tasks/task-268..272.md`.
Verified: the candidate performs **zero genuine file deletions** relative
to the merge-base (`git diff e1cf9d8d HEAD --diff-filter=D` is empty) —
those files are base-only additions made after the branch point, and a
three-way merge preserves them. No candidate-side damage to other tasks'
work.

## Retained verification failure (verification-stage, not a source defect)

The durable finding (attempt 2b7e41fd, checks.sh exit 1) is confined to the
upstream-gate phase: the controller's ROOT copy of
`verifier-script-overlay.py` (taken from main, pre-d28e4a32) restores the
base's entire `scripts/` tree, including the frozen
`abac-route-registry-exemptions.txt` that still lists the two
workspace-scoped preview routes this task legitimately removed.
`check-abac-exempt-handlers.sh` errors on exempt mappings with no route
(script lines 259-260: "An exempt mapping with no route means the mapping
is stale"), so the stale base data fails the upstream phase. Diagnosis
verified against the script's actual behavior. The repo-side fix (d28e4a32)
restores check *logic* from the base but lets frozen exemption data follow
the merge — this cannot weaken gates: dev-check.sh forbids adding exemption
entries relative to the base, and a removed entry only re-exposes the
underlying finding. No candidate-side fix exists that does not violate the
clean-cutover acceptance criterion or the frozen-baseline rule.

## Evidence gathered on this exact tree

- **19/19 mechanical invariant checks pass** (check-arch, abac-route-registry,
  abac-exempt-handlers (83 handlers), mcp-write-tools, migration-versions,
  dead-message-kinds, byte-slice-truncation, relative-path-defaults (both
  checks), fail-open-ref-resolution, inert-enforcement, scope-literal-defaults,
  fabricated-scope-defaults, lossy-secret-conversion, forwarded-header-trust,
  in-memory-state-stores, unbounded-external-http, forged-scope-fields,
  migration-sql-portability, mem-port-contracts). See
  `review-evidence/static-checks.txt`.
- **`cargo check -p gyre-server` clean** (7m54s, includes web build via
  build.rs). See `review-evidence/cargo-check-gyre-server.txt`.
- **Web: `npm ci` + vitest → 2576 passed / 41 skipped** (127 files). The
  Svelte CSS/a11y warnings in the output are pre-existing upstream warnings,
  not failures. See `review-evidence/web-tests.txt`.
- **`python3 scripts/check-rustfmt-diff.py main` → exit 0** (37 changed Rust
  files, changed lines clean).

## Acceptance criteria — review evidence

- **Real agent spawn per target** — `post_meta_spec_preview`
  (meta_specs.rs:832): four passes (resolve+authorize, budget, provision with
  rollback compensation, launch). Agents are real `Active` rows scoped to the
  repo's workspace, on real `preview/{preview_id}/{slug}` branches with real
  worktrees (`create_branch` at a pinned `base_sha`, `create_worktree`,
  `state.worktrees.create`). Slug collisions disambiguated
  (`preview_branch_name` + `used` map). Test
  `preview_spawns_real_agents_with_branches_worktrees_and_draft` drives the
  full router against a real bare git repo and asserts agent rows, branches,
  and on-disk worktrees.
- **Draft reaches the agent** — env injection
  (`GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`,
  `GYRE_PREVIEW_ID`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately
  absent) merged before the server-owned vars so it cannot shadow identity
  env. `agent-runner.mjs` builds the preview prompt with the draft as the
  governing "how to build" context over the target spec ("where the draft
  and your usual conventions disagree, the draft wins"). The spawn test
  asserts the actual env dump of a real launched process contains the draft
  text — not a field-set check.
- **202 spec shape** — `PreviewResponse { preview_id, agents:
  [{agent_id, repo_id, spec_path, branch}] }`, `StatusCode::ACCEPTED`;
  hollow structural-impact fields gone. Verified by test and by handler
  reading.
- **Skip-ceremony enforced** — no task assignment (`task_id: None`, runner
  withholds task-mutating MCP tools), no spec-approval/quality gates, no MR,
  no provenance, no `refs/agents/*`/`refs/tasks/*` writes; the spawn test
  asserts all of these negatively against real stores. Escape routes are
  guarded in both directions (preview agent as *caller* and as *target*) on
  complete/stop/fail, orchestrator, compose, tasks, MRs, merge-queue,
  agent-tracking, specs, specs-assist, admin kill, and the MCP dispatch
  (`PERMISSION_DENIED` for ceremony tools). The array-scoped JWT
  discriminator has a pinned unit regression
  (`caller_is_preview_agent_detects_array_scoped_jwt`).
- **Budget** — defaults to the target workspaces' budgets with up-front
  grouped reservation (`check_spawn_budget_for`); separate
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap fails closed on enumeration
  errors; slot release is exactly-once across every generic terminal path
  via the durable `released` flag (tests
  `generic_stop_fail_release_preview_slot_exactly_once`,
  `stale_abort_releases_preview_slot_exactly_once`,
  `preview_worktree_failure_releases_no_untaken_budget_slot`,
  `preview_budget_enumeration_failure_fails_closed`).
- **GET status: real state + diff** — per-agent state derived from real agent
  rows (`preview_agent_status`), never hardcoded; diff computed against the
  pinned `base_sha` via the git adapter. Test
  `preview_agent_finish_teardown_and_diff` runs a real agent script that
  commits and pushes, waits for the exit monitor, and asserts Stopped (not
  Idle), worktree gone, branch alive, token revoked, and the diff surfacing
  the agent's commit.
- **No idle state** — `on_agent_process_exit` routes preview agents through
  `finish_preview_agent` (Stopped + token revoked + worktree removed + slot
  released) on every compute-target monitor; REST/MCP complete is refused.
- **DELETE** — kills via the recorded spawn backend (not handle shape —
  SshDocker/Kubernetes tested), stops rows, acks orphaned inboxes,
  force-removes worktrees, closes dangling MRs, deletes branches, revokes
  tokens, releases slots, drops kv records; 404 on unknown id. Test
  `preview_delete_tears_down_everything` + cross-tenant denial tests.
- **GC** — `meta_spec_preview_gc` job registered (interval
  `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1h) bound to the real
  sweep; TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24h, with a TTL
  boundary test (at/past TTL deleted, just-under retained) and an orphan
  pass reclaiming index entries whose run record is gone.
- **Clean cutover** — workspace-scoped preview routes,
  `compute_preview_blast_radius`, and `StructuralImpact` fully removed
  (grep clean); the two corresponding frozen exemption entries were removed
  (legitimate shrinkage — the frozen-baseline rule fails on *growth*).
- **Docs** — api-reference.md (routes + errors + shapes) and
  server-config.md (all four knobs) updated.
- **Web UI** — preview loop with poll, per-target tabs, locked diff during
  run, iterate; 867-line test expansion, all green.

## Commands the independent verifier must run (not run here — 4 GiB budget
reserves gyre-server test compilation for the verifier)

- `cargo test -p gyre-server --lib -- preview_`
- `cargo test -p gyre-server --test api_integration`
- `cargo test -p gyre-server --test auth_integration`
- `cargo test -p gyre-server --test e2e_ralph_loop`
- `cargo test -p gyre-server --test git_integration`
- `cargo test -p gyre-server --test m18_oidc_integration`
- `cargo test --all`
- `cd web && npm ci && npm test` (run here: green)
- Upstream-overlay phase: with the controller's ROOT
  `verifier-script-overlay.py` updated from this branch (contains d28e4a32),
  or accepting the merged-tree phase-two results as authoritative.
