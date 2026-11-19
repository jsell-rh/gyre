# Review — task-206 (Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup)

Spec: `specs/system/meta-spec-reconciliation.md` §5 "Preview Mode: The Fast Iteration Loop".
Candidate under review: `bb041cdd59bb5458ad489a182ab876588736c9ae` (base `0ea6a629`).
Verdict: **needs-revision** — the server-side §5 implementation is real, thorough and well-tested (verified below), but the candidate's rewrite of the production §9/§5 UI loop in `MetaSpecs.svelte` introduced a reproducible correctness regression plus poll-generation gaps.

## Probes run (independent, on the exact candidate)

- `cargo check -p gyre-server` (build wrapper, cold cache) → exit 0. Evidence: `/tmp/stage/review-evidence/cargo-check-gyre-server.log`.
- `cargo test -p gyre-adapters ssh` → 15 passed. Evidence: `cargo-test-gyre-adapters-ssh.log`.
- `cargo test -p gyre-adapters git2_ops` → 35 passed. Evidence: `cargo-test-gyre-adapters-git2ops.log`.
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh` → all exit 0. Evidence: `check-*.log`.
- `cd web && npm ci` → exit 0; `npx vitest run src/__tests__/MetaSpecs.test.js src/__tests__/EditorSplit.test.js` → 174/174 passed. Evidence: `vitest-metaspecs-editorsplit.log`.
- **Repro probe (temporary test, deleted after run; `web/dist` restored)**: switch meta-spec persona-1 → persona-2 (unmodified) then click Preview → the §9 State 2 locked editor shows a FABRICATED pending-changes diff (persona-2's prompt rendered as a `+` line against persona-1's baseline). Evidence: `vitest-d2-repro.log`.

Deferred to the independent verifier (sandbox build-wrapper constraint, standing for gyre-server): `cargo test -p gyre-server preview_`, `cargo test -p gyre-server mcp_preview_agent`, `cargo test -p gyre-server generic_stop_fail_release` / `generic_kill_and_budget_stop`, plus the full `cargo test --all` and `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets`.

## Verified working (no findings)

- **Real spawn path.** `POST /api/v1/meta-specs/preview` (global routes registered in mod.rs:891-906; ABAC resolver entries added) validates every target up front (repo exists, `check_repo_abac`, tenant containment, spec_path on the default branch), then provisions per target a real Active agent row, a `preview/{preview_id}/{slug}` branch pinned to the default-branch tip, a real worktree, and a short-lived JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800s). 202 response is the spec shape exactly. Rollback on provision/persist failure is compensated and tested (injected-failure stores).
- **Draft reaches the agent.** `GYRE_PREVIEW_ID`/`GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`/`GYRE_TARGET_SPEC_PATH` injected through `launch_taskless_agent_process`; `GYRE_TASK_ID` deliberately absent; agent-runner.mjs builds the prompt with the draft as governing context.
- **Skip-ceremony enforced, not just skipped.** Preview agents are refused on the complete/stop/fail/usage/status/task/MR/merge-queue/spec-approve/specs-save/commits-record/compose/orchestrator-spawn/agent-registration REST routes (caller-keyed via `caller_is_preview_agent`, target-keyed where relevant) and on the MCP ceremony tools; `finish_preview_agent` lands them Stopped (never Idle) with token revoked, worktree removed, budget slot released exactly-once across every generic terminal path (stop/fail/admin kill/budget stop/stale abort/DELETE/GC — durable `released` flag). Launch verdicts use the dispatch's `launched` flag, not handle shape; kills route through recorded backends.
- **Status/diff, DELETE, GC.** GET status derives per-agent state from real agent rows and computes the diff of the preview branch vs the pinned base SHA through the git adapter; DELETE kills agents, force-removes worktrees, deletes branches, closes dangling MRs, removes records (404 unknown); `meta_spec_preview_gc` job registered (jobs.rs) and spawned (main.rs), TTL default 24h, interval default 1h; orphan pass reclaims index entries whose run is gone.
- **Clean cutover.** Hollow `compute_preview_blast_radius`/`StructuralImpact` and the workspace-scoped preview routes are gone, including the two stale ABAC exemption lines. Docs (`api-reference.md`, `server-config.md`, `docker/gyre-agent/README.md`) updated accurately.
- **Server test quality** is genuinely high: router-driven tests with real processes, real bare repos, real worktrees, positive controls on every denial test, exact-count budget assertions, TTL boundary on the exact boundary. Weaknesses are minor and listed as findings (env-var name not pinned, jobs wiring and stale-detector leg untested).
- MCP spawn/decompose/cross-repo tools are tier-contained for preview agents (workspace/repo-orchestrator token required), so the unlisted MCP tools are not an escape.

## Findings (blocking)

- **F1 (code, regression, reproduced)** — `web/src/components/MetaSpecs.svelte` `onMsChange` (~line 422) dropped the `selectedMsBaseline = ms?.prompt || ''` refresh that base `0ea6a629` had (base line 389). Switching meta-specs then previewing fabricates the §9 State 2 pending-changes diff (draft vs the WRONG published prompt). Repro evidence: `vitest-d2-repro.log`. No existing test covers the switch→preview sequence.
- **F2 (code)** — poll-generation gaps: `onMsChange` does not bump `previewRun`, and the post-await stale guard in `pollPreview` (~line 520) calls `stopPreview()` which clears the CURRENT interval — a stale tick can resurrect an old completed preview over the editor, and after cancel→quick re-Preview it kills the new run's interval (stuck 'running', no polling, no stall notice).
- **F3 (code, minor)** — `MetaSpecs.svelte:894` diff renderer classifies `+++`/`---` header lines as add/remove (EditorSplit filters them).
- **F4 (code, low)** — `count_running_preview_agents` returns 0 on `kv_list` error; with `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` set (workspace-budget check skipped in that branch) the preview cap fails open under kv degradation.
- **F5 (test)** — no switch→preview UI test; `EditorSplit.test.js:151` comment falsely claims MetaSpecs mounts EditorSplit (the `context === 'meta-spec'` branch is production-unmounted on this candidate).
- **F6 (test, minor)** — draft-content assertion doesn't pin the `GYRE_META_SPEC_DRAFT_CONTENT` variable name; jobs.rs GC registration wiring and the stale-detector slot-release leg are untested.

Non-blocking observation: after "Iterate" the UI nulls `previewId`, so the previous run can no longer be cleaned up via the Clear/Clean Up action (spec §5 action bar) — branches linger until the 24h GC.

Recommended repair: in `MetaSpecs.svelte`, make `onMsChange` refresh the baseline and invalidate the run generation (e.g. route through `resetPreviewState()` + baseline update), make the post-await guard a bare `return`, port the `+++`/`---` filter into the inline diff renderer, and add the missing switch→preview test (the repro probe in the evidence log is a ready-made template). F4: fail the preview-cap check closed on enumeration error. F6: pin the env-var name in the happy-path assertion.
