# Review — task-156 (Reconciliation controller and conformance sweep)

Spec: `specs/system/meta-spec-reconciliation.md` §6 (Reconciliation), §10 (Conformance Sweeps), §11 (Observability).
Candidate: `77d76b8e` (branch `pipeline/task-156/749f3553a92b457ab2a397509b580bd8-7`), assigned base `1a00087c`.
Verdict: **needs-revision** (verdict.json written to /tmp/stage; evidence under /tmp/stage/review-evidence/).

## Prior durable findings — repair status

The three findings from the earlier review round (attempt-9 source `9be46de6`) were re-checked against this candidate:

- **`required` no-op on PUT** — FIXED. `update_meta_spec_registry` restores the `if let Some(required) = req.required` branch (meta_specs.rs:1197); `update_required_field_persists` passes.
- **Second wave never emits ReconciliationCompleted** — FIXED. Wave identity is now `(max(created_at), total task count)` over the workspace's reconciliation tasks, so terminal tasks from prior waves no longer drag the identity back; `second_wave_emits_its_own_completion` passes.
- **Two-step edit→approve never triggers §6** — the repair mechanism is real (`approved_content_hash` stamped server-side at approval time, new migration `2026-10-10-000056`, compared at approval; a live probe confirms an approval-only PUT triggers reconciliation), but the test that pins it fails at the candidate — see F1.

## Verified working (no findings)

- `run_reconciliation`: per-repo task creation, title `Align code with updated {kind} {name} v{version}`, label `meta-spec-reconciliation`, priority Medium, task_type Delegation, dedup scoped to (workspace, repo, title) against non-terminal tasks. 18 reconciliation tests pass (`cargo test -p gyre-server --lib reconciliation::`).
- `put_meta_spec_set` diffs old→new pins via BTreeMap canonical ordering and triggers only on changed/new pins; identical re-PUT is a no-op (`identical_reput_keeps_set_sha_stable`).
- Conformance sweep registered as job `meta_spec_conformance_sweep` (SWEEP_INTERVAL_SECS = 86400, `GYRE_META_SPEC_SWEEP_INTERVAL_SECS` override), spawned in main.rs; drift detection compares active set SHA against `meta_spec_set_sha` in 30-day-window attestations; deduped drift-review task per workspace+set-sha; `MetaSpecDrift` priority-6 notifications to Admin/Developer/Owner members; `gyre_meta_spec_drift_total` metric.
- Observability: `reconciliation_started` / `meta_spec_set_updated` / `meta_spec_drift_detected` / `meta_spec_drift_resolved` events, `ReconciliationCompleted` on wave completion, `gyre_reconciliation_tasks_total` / `gyre_reconciliation_duration_seconds`.
- Per-handler authorization on the registry routes is real (Admin-only writes, tenant containment + membership on scoped list); `registry_writes_reject_non_admin` passes.
- Mechanical checks at the candidate: check-abac-exempt-handlers (89 handlers, 0 violations), check-arch, check-dead-message-kinds, check-migration-versions — all OK. Migration 000056 is next in sequence and portable (bare `ALTER TABLE ... ADD COLUMN`).

## Findings

- [-] **F1 (major): the candidate's own test suite is red, and the double-trigger it exposes defeats the dedup acceptance criterion.** `cargo test -p gyre-server --lib api::meta_specs` → 17 passed, **1 failed**: `two_step_edit_then_approve_triggers_reconciliation` fails deterministically (reproduced twice, including in isolation) at its first assertion — first approval of never-approved content expects 1 reconciliation task, finds 2. Live probe (temporary test, tree restored clean; evidence `task-156-double-trigger-probe.txt`): create spec (v1, Pending) → PUT set binding the spec creates `Align code with updated meta:persona X v1` (set-diff trigger fires on first binding of still-unapproved content) → approval-only PUT bumps `ms.version` to 2 unconditionally and fires the approval trigger, whose title `...v2` can never match the v1 task in the (workspace, repo, title) dedup → second task. Two reconciliation tasks per repo for one logical rollout; the "Duplicate reconciliation tasks are deduplicated" acceptance criterion fails for adjacent triggers because every PUT (including no-op approvals) increments the version the title embeds. The shipped notes claim focused tests green citing `/tmp/stage/review-evidence/task-156-focused-tests.txt` — that file does not exist in this stage and the run fails at the exact candidate. Fix: either suppress the set-diff trigger for pins whose spec is not Approved (reconciliation to unapproved content is premature per §6 "approved and rolled out"), or stop bumping `version` on approval-only PUTs / dedup on (repo, spec identity) rather than the version-suffixed title — and make the shipped test pass.
- [-] **F2 (major): the candidate diff silently reverts task-170 (View Specification Grammar) — unrelated completed production code.** Deleted base→candidate: `web/src/lib/types/view-spec.ts` (298 lines), `web/src/lib/layoutRegistry.js`, `web/src/__tests__/view-spec.test.js` (231 lines), `web/src/__tests__/MoldableViewListView.test.js`, `.gitattributes`. Reverted: `crates/gyre-common/src/view_spec.rs` (kebab-case grammar → snake_case, `deny_unknown_fields` and side-by-side sub-view validation dropped), `crates/gyre-server/src/api/explorer_views.rs` (−421 lines), `MoldableView.svelte`, ExplorerCanvas test timeout overrides, and the committed web/dist bundle rebuilt without the feature. `specs/tasks/task-170.md` still says `progress: complete` with ui-layout.md §4 coverage rows — publishing this candidate makes those rows hollow (code and tests deleted). Cause: recovery checkpoint `f9a30460` (in the task's commits list) committed the interrupted attempt's older-main sandbox tree on top of `b1cdb0dd` (task-170's commit), and the final base merge `fd141db3` (merge-base `b1cdb0dd`) silently preserved the deletion. Evidence `task-156-view-spec-reversion.txt`. Fix: re-land the task-156 changes on base `1a00087c` without the snapshot reversion — the candidate tree must be identical to base on every task-170 surface (or restore them before publication).

## Evidence

- `/tmp/stage/review-evidence/task-156-meta-specs-tests.txt` — `cargo test -p gyre-server --lib api::meta_specs` at candidate: exit 101, 17 passed / 1 failed.
- `/tmp/stage/review-evidence/task-156-reconciliation-tests.txt` — `cargo test -p gyre-server --lib reconciliation::`: 18 passed.
- `/tmp/stage/review-evidence/task-156-double-trigger-probe.txt` — live probe: task titles after set-PUT and after approval-only PUT (v1 + v2).
- `/tmp/stage/review-evidence/task-156-view-spec-reversion.txt` — git evidence of the task-170 reversion and merge topology.
- `/tmp/stage/review-evidence/task-156-abac-exempt.txt`, `-arch.txt`, `-dead-kinds.txt`, `-migrations.txt` — mechanical checks OK.

— Review round, 2026-10-12 (candidate `77d76b8e`, working tree left clean at the candidate SHA)
