# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R1 (this candidate: 7394a13f, includes repair rounds 05ab61cb-era findings, the base-integration merge e8d8426f and the process commit recording branch commits)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** f70cfac6f829847f994daf0cb96631503b1a1561
**HEAD reviewed:** 7394a13f0d7d81f5ae53f1f34b64629dda6f4bf0
**Status:** complete

---

## Verdict

**Approved.** The preview mode is a real agent run path, not an estimate. No
blocking defects found. Evidence and per-criterion analysis below; probe
artifacts under `/tmp/stage/review-evidence`.

## What was verified independently

- `cargo check -p gyre-server` on the exact candidate: exit 0.
- `python3 scripts/check-rustfmt-diff.py f70cfac6`: changed lines clean
  (33 Rust files). The pre-repair verification failure (admin.rs 446-450,
  meta_specs.rs 2195/5205-5214) is fixed by d603b9f0.
- `scripts/check-abac-route-registry.sh` and `scripts/check-abac-exempt-handlers.sh`:
  exit 0 on the merged tree. The pre-repair stale-exemption failures
  (base's `/api/v1/workspaces/:id/meta-specs/preview{,/:preview_id}` exemption
  lines vs. removed routes) are resolved: the merged tree drops both exemption
  entries and registers the spec-conformant global routes with real
  `RouteResourceMapping` entries in `abac_middleware.rs`.
- Full web suite (`cd web && npm ci && npm test`): 127 files, 2572 passed,
  41 skipped — includes the rewritten `MetaSpecs.test.js` preview-loop tests
  (POST shape, per-agent tabs, running→complete, diff rendering, malformed
  response, stall timeout, reset generation counter).
- `scripts/check-task-commit-attribution.sh`: passes against the committed
  HEAD's `specs/tasks/task-206.md` (all 19 branch SHAs recorded; the working-tree
  failure observed during review is the review-stage pipeline reset of the task
  file, not candidate state — see review-evidence/03).

## Per-acceptance-criterion evidence

### A1 — POST spawns one real agent per target on `preview/{preview_id}/{slug}`

`post_meta_spec_preview` (meta_specs.rs:832) resolves/authorizes every target
first (repo exists → 404, `check_repo_abac` + tenant containment → 403,
spec_path on default branch → 400), then per target: pins `base_sha` via
`git_refs::resolve_ref` (no fail-open — an empty repo is a 400), creates the
branch, an `Active` `Agent` row scoped to the repo's real workspace (no
placeholder identity), a real git worktree (`create_worktree` checks out the
pre-created branch), an `AgentWorktree` row, and a short-lived EdDSA JWT, then
launches through the same `dispatch_agent_launch` every normal spawn uses.
`launch_preview_agent` requires `outcome.launched` — the dispatch flag, not a
pid-shape guess — so SshDocker/Kubernetes launches (pid: None by design) are
not misreported. Test `preview_spawns_real_agents_with_branches_worktrees_and_draft`
asserts real agent rows (Active, correct workspace, `current_task_id: None`),
real branches via `branch_exists`, real worktree directories on disk, and the
202 response shape with `preview/<id>/{search,identity}` branches.

### A2 — Draft content reaches the agent as governing context

The draft travels in the process env (`GYRE_PREVIEW_ID`,
`GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`,
`GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately absent — verified in
`launch_taskless_agent_process`: extra_env merges BEFORE the server-owned
identity vars so it can never shadow them). `agent-runner.mjs` builds a
preview prompt embedding the full draft text as the governing "how to build"
context over the target spec, withholds the task-mutating MCP tools, skips
conversation upload and `gyre_agent_complete`, and makes the push to the
preview branch the deliverable. The spawn test dumps the real agent process
env and asserts the draft body text is present (not a field-set check).

### A3 — 202 response matches the spec shape exactly

`PreviewResponse { preview_id, agents: [{agent_id, repo_id, spec_path, branch}] }`
— no structural-impact fields; blast radius stays on §4's endpoint.

### A4 — Skip-ceremony + budget

No task row, no gates, no MR, no merge queue, no provenance, no
`refs/agents/*`/`refs/tasks/*` writes (all asserted in the spawn test).
Escape routes are closed on BOTH surfaces: MCP dispatch refuses
`gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
`conversation_upload` for preview agents, and the REST surface refuses
preview-agent callers on tasks, MRs (create + status), merge-queue enqueue,
compose, spec approve/save, commits/record, agent registration, stop/fail
control routes and orchestrator spawns (all inspected — real 403 guards, not
bookkeeping). Budget: `check_spawn_budget_for` reserves all N slots per
workspace up front (no over-admit when `max_concurrent_agents < N`);
`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently when
set (429 on overflow — tested). Slot release is exactly-once across
process-exit, DELETE, GC, stop/fail routes, admin kill, budget stop and the
stale-agent detector via the durable `released` flag
(`mark_preview_slot_released` called from every generic terminal path;
regression tests `generic_stop_fail_release_preview_slot_exactly_once` and
`generic_kill_and_budget_stop_release_exactly_once_across_gc` encode the
review-round double-release bug).

### A5 — GET status reports real state + diff

Per-agent status is derived from the agent rows
(`preview_agent_status_label`: Active→running, Failed/Dead, Stopped+released→complete);
`diff` is the preview branch vs the pinned `base_sha` through the same
`state.git_ops.diff` plumbing the MR diff uses, `null` until the branch
exists. `preview_agent_finish_teardown_and_diff` runs a real agent script
that commits and pushes, waits for the monitor, and asserts Stopped (never
Idle), worktree removed, branch surviving, token revoked, and the diff
showing the agent's commit.

### A6 — No lingering Active/Idle

Every compute-target monitor routes through `on_agent_process_exit` →
`finish_preview_agent` (kv-indexed discriminator, one lookup per exit):
token revoked, worktree removed, slot released, row Stopped — the terminal
status flip is the LAST mutation so a terminal row implies a cleaned-up
agent. An unclassifiable/corrupt record still terminal-izes the agent rather
than handing it real work.

### A7 — DELETE kills and cleans

`delete_meta_spec_preview` (404 unknown id — tested) → `teardown_preview`:
kills through the backend recorded at spawn (`spawned_backends` — an
SshDocker remote container id or a Kubernetes Pod name is killed via its
real backend, not a silent local no-op; regression tests for the
recorded-backend routing), stops the row, removes the worktree before the
branch (git refuses deleting a checked-out branch), closes any open MR on a
preview branch (belt-and-braces), deletes the branch, revokes the token,
releases the slot, drops kv records. `preview_delete_tears_down_everything`
verifies branches/worktrees/records; GET afterwards 404s.

### A8 — Background GC with configurable TTL

`meta_spec_preview_gc` job registered in jobs.rs + spawned in main.rs,
interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS` (default 1h), TTL
`GYRE_META_SPEC_PREVIEW_TTL_HOURS` (default 24h). `run_once` expires runs at
`now - created_at >= ttl` (saturating against clock skew), and an orphan
pass reclaims agent index entries whose run record is gone (slot, token,
worktree, branch). `preview_gc_ttl_boundary` ages one run to exactly the
TTL and asserts deleted vs. a fresh run retained.

### A9 — Clean cutover

`compute_preview_blast_radius`/`StructuralImpact` and the workspace-scoped
preview routes are gone from mod.rs; no references remain anywhere in
crates/web/docs/docker (grep verified). The ABAC exemption file no longer
carries the removed routes. `api.js` `previewPersona*` replaced by the global
`previewMetaSpec*`.

### A10 — Docs

`docs/api-reference.md` gains the three routes plus a full "Meta-Spec
Preview Mode" section with the skip-ceremony table;
`docs/server-config.md` documents all four config knobs;
`docker/gyre-agent/README.md` documents the preview env vars.

### A11 — Tests prove the behaviors

38 preview tokio tests reviewed line-by-line: real bare git repos, real
processes via `GYRE_AGENT_COMMAND` scripts, real monitors, rollback
compensation tests (persist failure, worktree failure with no-slot-taken
budget assertion, agent-row failure orphan-branch deletion), launch
regressions for ssh-docker/kubernetes backends, authz matrices
(ReadOnly 403, unknown repo 404, cross-tenant/ABAC 403 on POST/status/DELETE),
JWT-scope discriminator test, and REST-ceremony refusal tests per route
family. No `#[ignore]`, no feature gates, no conditional skips.

## Non-blocking observations

1. **Preview-cap admission is fail-open on kv-store failure**
   (`count_running_preview_agents` returns 0 with a warning when
   `kv_list` errors). Only reachable when `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`
   is set; the same failing store then rejects the request at
   `persist_preview` (full rollback), so no over-admission persists, and the
   default workspace-budget path does not consult kv. Cosmetic robustness
   note, not a spec gap.
2. **An admin-stopped preview agent reports status `complete`** rather than
   `stopped` (the generic stop path sets the `released` flag, which the
   label maps to "complete"). The value is still derived from real ledger
   state; purely a labeling nuance for an operator-initiated kill.
3. **Generated output (recorded separately, not a finding):** running
   `cargo check` in this sandbox triggered build.rs to rebuild `web/dist`
   with new content hashes; the committed generated bundle was restored and
   the byproducts removed. `web/dist/` is verifier-owned.
4. The candidate modified `scripts/verifier-script-overlay.py` (verifier
   integration) so frozen `*-exemptions.txt` DATA follows the merge while
   check LOGIC is still restored from the base. Scrutinized: `dev-check.sh`
   independently forbids ADDING exemption entries (git-object comparison,
   immune to worktree overlays), so merged data is entrywise a subset of
   base data and can only make checks stricter; both properties are covered
   by new unit tests in `scripts/test_dev_checks.py`
   (`test_exemption_data_follows_the_merge`,
   `test_candidate_stale_exemption_entry_still_fails`). This is the correct
   fix for the legitimate clean-cutover failure this task triggered, not a
   weakening.

## Tests the independent verifier must execute

(Rust test compilation was deferred from the review sandbox per the 4 GiB
build budget; `cargo check` and the full static-gate set were run locally.)

- `cargo test -p gyre-server --lib` (covers the 38 preview tests in
  `api::meta_specs::tests` plus the updated `rbac.rs` / `spawn.rs` /
  `middleware.rs` test modules).
- `cargo test --all` (AGENTS.md definition of done).
- `python3 scripts/test_dev_checks.py` (new overlay-safety tests).
- `bash scripts/check-arch.sh`, `bash scripts/check-abac-route-registry.sh`,
  `bash scripts/check-abac-exempt-handlers.sh` on the integrated tree
  (already green in this review; re-run after final integration).
