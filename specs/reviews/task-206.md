# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Review agent (independent)
**Round:** R1 (continued from an interrupted review of the same candidate; no prior
verdict carried forward — all evidence below re-validated or re-run at this candidate)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 9c5474ba62dae1838cae01cadbd0af1c04ecb9be
**Candidate reviewed:** 6cf37fd3d7e173f199209576b9accc3ad45b8bfd (branch HEAD)
**Status:** complete

---

## Verdict

**Approved.** The hollow structural-impact preview implementation is gone,
replaced by a real agent run path that satisfies every §5 requirement and every
acceptance criterion in the task. No blocking findings.

## What was reviewed

Diff base→candidate scoped to task-206's own work (the raw diff also contains
merged-in base updates from other tasks — task-252 ws/activity gating,
task-249/252 message-kind changes, task-263 MR-scope, task-268/269 — which are
outside this task's review scope and arrived via sanctioned base merges).

### A1 — POST /api/v1/meta-specs/preview spawns one real agent per target

- Global routes registered (`api/mod.rs:942-948`): `POST
  /api/v1/meta-specs/preview`, `GET|DELETE
  /api/v1/meta-specs/preview/:preview_id`. ABAC resolver mappings added
  (`abac_middleware.rs:794-799`).
- `resolve_preview_targets` (meta_specs.rs:918) validates every target before
  creating anything: repo exists (404), workspace exists, repo ABAC
  (`check_repo_abac`) + tenant containment (403), spec_path exists on the
  default branch (400). Scope is derived from repo/workspace records, never
  caller-supplied — matches the AGENTS.md forged-scope invariant.
- `provision_preview_agent` (meta_specs.rs:1783): branch
  `preview/{preview_id}/{slug}` pinned to the default-branch tip via
  `git_refs::resolve_ref` (fail-closed, no empty-SHA default), real `Active`
  agent row scoped to the repo's real workspace, real git worktree +
  `AgentWorktree` row, short-lived JWT minted through
  `agent_signing_key.mint` with the preview TTL. Failure paths roll back
  branch/rows/token with a documented no-untaken-slot rule.
- Launch goes through `launch_taskless_agent_process` (spawn.rs:2894) — the
  same `dispatch_agent_launch` used by task-driven spawns, including SVID
  issuance (identity-security Layer 1) so heartbeats don't fail attestation.
  Launch failure is judged by the dispatch's `launched` flag, not handle
  shape (correct for SshDocker/K8s handles with `pid: None`).
- 202 response shape matches the spec exactly: `{preview_id, agents:
  [{agent_id, repo_id, spec_path, branch}]}`.

### A2 — Draft reaches the agent as governing context

- Env injection: `GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID`
  deliberately absent (spawn.rs merge order prevents extra_env from shadowing
  server-owned vars).
- `docker/gyre-agent/agent-runner.mjs` builds the preview prompt with the
  draft meta-spec inline as the governing "how to build" context over the
  target spec, withholds task-mutating MCP tools, no completion/provenance
  upload. Node test `preview run with configured gates: draft in prompt,
  gates never executed, no upload, no completion` proves it (re-run by this
  review, 12/12 pass).
- Server test `preview_spawns_real_agents_with_branches_worktrees_and_draft`
  uses a real bare git repo + real Git2OpsAdapter + a real spawned process
  dumping its environment, and asserts the draft text is in the process env.

### A3 — Skip-ceremony enforced, not just omitted

- No task row, no task assignment (`current_task_id` asserted None).
- MCP dispatch refuses task/MR/provenance mutations and cross-agent targeting
  for preview agents (mcp.rs:5147, 5171).
- REST guards (`caller_is_preview_agent`) on complete, spawn, task/agent
  status mutations, MR create/review, merge-queue, compose, orchestrator,
  messages, spec approve, assist — 21 call sites verified present. The
  JWT-scope discriminator handles both array-scoped and legacy
  string-scoped agent JWTs (unit-pinned by
  `caller_is_preview_agent_detects_array_scoped_jwt`), and the registry
  lookup fails closed on a store outage.
- Tests assert no MRs, no tasks, no provenance kv entries, no
  `refs/agents/*` / `refs/tasks/*` writes.

### A4 — Budget

- Default: per-workspace `check_spawn_budget_for` with N-target up-front
  reservation grouped per workspace (a single POST with N targets cannot
  over-admit a workspace with `max_concurrent_agents < N`).
- `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently when
  set; enumeration of running preview agents fails closed (429/500 path, not
  open admission).
- Slot release is exactly-once across process-exit, DELETE, GC, stop/fail
  routes, admin kill, budget stop and the stale-agent detector, via a durable
  per-agent `released` flag re-checked before decrement (double decrement is
  unrecoverable under MAX(0, n-1)); `mark_preview_slot_released` is hooked
  into the generic terminal paths (stale_agents.rs). Tests cover each path.

### A5 — Status with real state and diff

- `preview_status_response` derives per-agent status from the real agent rows
  (`preview_agent_status_label`: running/complete/failed/stopped/dead/unknown),
  never a hardcoded "complete".
- `preview_diff` diffs the preview branch against the SHA it was created from
  (pinned at provisioning so a moving default branch can't pollute the diff),
  using the same git plumbing as the MR diff endpoint; `null` until the
  branch exists.
- `preview_agent_finish_teardown_and_diff` runs a real agent script that
  commits on the preview branch, pushes, and exits; asserts the monitor
  lands the agent in `Stopped` (never `Idle`), worktree removed, branch
  survives, token revoked, and the status reports `complete` with a diff
  containing the changed spec file.

### A6 — DELETE and GC

- `delete_meta_spec_preview`: role gate, scope containment (per-repo ABAC +
  tenant, with the recorded-workspace anchor surviving repo deletion), then
  full teardown: process kill routed through the recorded spawn backend,
  row Stopped, inbox bulk-ack, worktree force-removed, belt-and-braces MR
  close, branch deleted, token revoked, slot released, kv records removed.
  404 on unknown id. Tests assert branches/worktrees/kv gone and GET 404
  afterwards.
- `meta_spec_preview_gc` job registered in jobs.rs (interval
  `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1h) bound to
  `run_once`; TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24h. TTL
  boundary test: at/past TTL deleted, just-under retained, plus an orphan
  pass reclaiming index entries whose run record is gone. Job registration
  itself pinned by `meta_spec_preview_gc_job_registered` (triggers the real
  handler and asserts history).

### A7 — Clean cutover, docs, frontend

- `compute_preview_blast_radius` / `StructuralImpact` / the hollow response
  shape are gone; §4's blast-radius endpoint is untouched.
- `docs/api-reference.md` (routes + examples), `docs/server-config.md` (four
  new knobs), `docker/gyre-agent/README.md` updated.
- Frontend: `MetaSpecs.svelte` preview loop (POST → poll status → per-agent
  tabs with live diffs updating as each agent completes → complete state),
  `EditorSplit.svelte` locked-diff support. Web tests re-run by this review:
  MetaSpecs + EditorSplit, 184/184 pass.
- Coverage matrix row 9 flipped task-assigned → implemented with an accurate
  evidence summary.

## Independent evidence (this review)

Re-run at candidate 6cf37fd3 (logs under /tmp/stage/review-evidence):

| Probe | Result |
|---|---|
| `npx vitest run MetaSpecs.test.js EditorSplit.test.js` (after `npm ci`) | 184 passed, exit 0 |
| `node --test docker/gyre-agent/agent-runner.test.mjs` | 12 passed, exit 0 |
| `scripts/check-arch.sh` | exit 0 |
| `scripts/check-mcp-write-tools.sh` | exit 0 |
| `scripts/check-abac-exempt-handlers.sh` | exit 0 |
| `scripts/check-abac-route-registry.sh` | exit 0 |

Re-validated from the interrupted review of this exact candidate (same SHA,
checkpoint == candidate): `cargo check -p gyre-server` exit 0 (7m10s), and the
full static invariant battery (migration versions, dead message kinds, byte
slices, relative paths, fail-open refs, mem port contracts, fabricated scope
defaults, lossy secrets, scope literals, inert enforcement, SQL portability,
forged scope fields, forwarded headers, in-memory state, unbounded HTTP) —
all OK.

`scripts/check-task-commit-attribution.sh` fails **on the working tree only**:
the stage harness rewrote `specs/tasks/task-206.md` to the assignment state
(`progress: not-started`, `commits: []`). At the candidate commit itself every
task-labeled product-surface commit is recorded in the frontmatter (verified
via `git show HEAD:specs/tasks/task-206.md` — all 19 flagged SHAs present).
Not a candidate defect.

## Tests the verifier must execute (not run here — 4 GiB budget rule)

```
cargo test -p gyre-server            # esp. api::meta_specs::tests::preview_*,
                                     # jobs::tests::meta_spec_preview_gc_job_registered,
                                     # rbac/middleware preview guard tests
cargo clippy --all-targets           # repair commits 2dbb2775 etc. addressed findings
cd web && npm ci && npm test         # full frontend suite
```

## Non-blocking observations

1. **Workspace-scoped preview routes retained, not removed.** The plan said to
   remove them; they are instead re-registered as resolver-enforced twins of
   the global routes (exemption entries removed — the sanctioned retirement
   direction), with full per-handler workspace/tenant/repo-ABAC containment.
   This is stronger than removal and was forced by a verification finding
   (frozen base exemption data). Documented in api-reference.md. The coverage
   matrix's phrase "workspace-scoped routes removed" is slightly imprecise —
   the *hollow workspace-scoped implementation* was removed; the routes live
   on as authorized aliases. Cosmetic only.
2. **`scripts/verifier-script-overlay.py` modified** so frozen
   `*-exemptions.txt` data follows the merge instead of being overlaid from
   the base. Pipeline-infra change with a written safety argument (merged data
   is entrywise a subset of base because dev-check forbids growth) and two
   new tests pinning both directions (`test_exemption_data_follows_the_merge`,
   `test_candidate_stale_exemption_entry_still_fails`). The independent
   verifier should re-confirm the subset invariant holds on the merged tree.
3. **`is_preview_agent` fails closed on a kv outage**, which during an outage
   refuses ceremony-route calls from *normal* agent-JWT callers too (preview
   registry unreadable). Deliberate, documented, and the conservative
   direction for a refusal gate.
