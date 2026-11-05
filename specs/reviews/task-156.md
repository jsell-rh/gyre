# Review — task-156 (Reconciliation Controller and Conformance Sweep)

Spec: `specs/system/meta-spec-reconciliation.md` §6 (Reconciliation: The Slow Rollout), §10 (Conformance Sweeps), §11 (Observability). Coverage rows 10, 14, 15.
Candidate: `85673368` on branch `pipeline/task-156/d1275b4c46e9411689a35c4019e836ed-1`, base `3c39ab1f`. Diff: 20 files, all task-relevant; no task-170 surface differs from base (the shipped byte-identical restoration claim verified via `git diff --name-only`).
Verdict: **approved**.

## Evidence (all at the candidate commit, clean tree; full logs under /tmp/stage/review-evidence/)

- `python3 scripts/check-rustfmt-diff.py 3c39ab1f` → changed lines clean (15 files).
- Clippy, diff-scoped via the check-clippy-diff.py JSON pipeline replicated per crate (the executor wrapper rejects workspace-wide invocations): `gyre-server --lib` and `--tests` both → 0 warnings on changed lines, 0 errors. The only error-level clippy lint in the build (`never_loop`, gyre-domain rust_extractor.rs:1071) is byte-identical at base — pre-existing, toolchain-specific, untouched by this diff.
- 19 mechanical invariant checks + task-commit-attribution → all PASS. Migration `2026-10-10-000056_meta_spec_approved_content_hash` uses the next unused sequence number and portable SQL.
- Focused tests: reconciliation suite 21 passed; meta_spec server 22 passed; adapters 10 passed; full `gyre-server --lib` 1265 passed, 0 failed.
- Mutation probes (restored afterwards, `git diff HEAD` verified empty):
  1. Disabling the approval trigger in `update_meta_spec_registry` → `two_step_edit_then_approve_triggers_reconciliation` FAILS — the trigger test pins real production behavior.
  2. Disabling the drift comparison in `run_conformance_sweep` → `sweep_detects_drift_and_creates_deduped_task` FAILS — the sweep test pins real production behavior.

## Verified against the acceptance criteria

- **Approval triggers reconciliation for affected repos (§6):** `PUT /workspaces/{id}/meta-spec-set` diffs old→new pinned SHAs (`diff_meta_spec_set`) and runs `run_reconciliation`; `PUT /meta-specs-registry/{id}` reconciles every workspace whose set binds a spec approved on changed content, keyed on the server-stamped `approved_content_hash` (new migration column, stamped at approval, never caller-supplied) so the two-step edit-then-approve flow the shipped UI performs triggers exactly like the combined flow, and re-approving unchanged content is a no-op. The double-trigger defect from the prior round is genuinely fixed: the Approved-only gate in `run_reconciliation` (a Pending bind is not a rollout) plus the content-only version bump collapse one logical rollout into one wave — pinned by `two_step_edit_then_approve_triggers_reconciliation` (mutation-verified above).
- **Task shape:** title `Align code with updated {kind} {name} v{version}`, labels `meta-spec-reconciliation` + `auto-created`, priority Medium, task_type Delegation, spec_path set — asserted in `reconciliation_creates_task_for_bound_repo`; per-repo task creation and cross-workspace non-suppression pinned by dedicated tests.
- **Sweep on schedule (§10):** job `meta_spec_conformance_sweep` registered in `jobs.rs` (default 86 400 s, `GYRE_META_SPEC_SWEEP_INTERVAL_SECS` override) and spawned in `main.rs`. DB-queries-only (attestation `meta_spec_set_sha` vs active set SHA over a 30-day window), no agent spawns — matches §10's "the sweep is cheap".
- **Drift-review tasks:** created per workspace+active-set-SHA, deduplicated against open tasks; clean-workspace, empty-SHA, and current-SHA negative paths all tested.
- **ReconciliationCompleted (§11):** `maybe_emit_reconciliation_completed` fires when the last open reconciliation task goes terminal (hooked in REST transitions, MCP update_task, repo archive), deduplicated against persisted wave identity (max(created_at) + task count) — the second-wave regression is covered by `second_wave_emits_its_own_completion`, idempotency by re-check assertions.
- **MetaSpecDrift notifications:** priority-6 notifications to Admin/Developer/Owner members from both the sweep (`notify_drift_members`) and wave completion (`emit_reconciliation_completed`) — same recipient pattern as the pre-existing infrastructure the task instructions designated; asserted end-to-end in tests.
- **Deduplication:** (workspace, repo, title) for reconciliation waves, (workspace, title) for drift review; identical set re-PUT is a no-op, pinned including the BTreeMap canonical-serialization invariant (`identical_reput_keeps_set_sha_stable` also checks request key order and that a changed pin does move the SHA).
- **Observability (§11):** `reconciliation_started`, `meta_spec_changed`, `meta_spec_set_updated`, `meta_spec_drift_detected`, `meta_spec_drift_resolved` events plus the existing `ReconciliationCompleted` MessageKind; Prometheus `gyre_meta_spec_drift_total{workspace,repo}`, `gyre_reconciliation_tasks_total{workspace,status}`, `gyre_reconciliation_duration_seconds{workspace}` registered and render-tested.
- **Authorization hardening (beyond the task minimum):** registry create/update/delete are Admin-only per-handler (the §1 human_only approval rule — a non-Admin approval would forge the §6 trigger), Workspace-scoped list enforces tenant containment + membership; the three stale `abac-exempt-handlers-exemptions.txt` lines are removed per that file's own no-growth instruction, and `check-abac-exempt-handlers.sh` passes.
- **Adapter contract parity:** `update()` archives a version-history row only when the version changed in SQLite, Postgres, and mem (cross-adapter invariant test in SQLite plus the mem-port-contracts check).

## Minor observations (not blocking, recorded for completeness)

- The sweep's drift-review task anchors `repo_id` to the first drifted repo; the task itself is workspace-scoped (title embeds the active set SHA). Cosmetic scoping artifact, not a spec violation (§10: "ensure a reconciliation task exists").
- `find_meta_spec_by_name` resolves set entries by registry name — the same join key the pre-existing blast-radius endpoint uses; the registry has no name-uniqueness constraint (pre-existing), so a Workspace-scoped name collision across workspaces could in principle resolve the wrong record. No new names are introduced by this task's flows that would collide in practice.
- Coverage rows 10/14/15 remain `task-assigned` pending the independent audit stage's status update — correct division of labor; the shipped code satisfies the sections.

## Deferred to verification

Workspace-wide single-invocation `--all-targets` clippy (wrapper-restricted here; per-crate lib+tests probes clean on changed lines), full workspace suites, GitHub CI. No web/ files changed in this diff.
