# Review — task-156 (Reconciliation controller and conformance sweep — meta-spec-reconciliation.md §6, §10, §11)

Spec: `specs/system/meta-spec-reconciliation.md` §6 (Reconciliation: The Slow Rollout), §10 (Conformance Sweeps — Steady State), §11 (Observability).
Base: `6a9739e519ae473e66dd0d9d403f3108f6784431` · Candidate: `80baf240a35be1104ab9f824458660f2bcf852a3`.
Verdict: **complete** (approved — all three prior-round findings verified fixed with kill-tests, no new defects found).

## Probes (evidence: /tmp/stage/review-evidence/task-156-review-evidence.txt)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1236 passed, 0 failed** (18 reconciliation + 18 meta_specs tests included).
- `cargo test -p gyre-adapters --lib meta_spec` → **10 passed, 0 failed**.
- Mechanical gates all exit 0: check-arch, check-migration-versions, check-mem-port-contracts, check-dead-message-kinds, check-migration-sql-portability, check-abac-exempt-handlers, check-abac-route-registry, check-mcp-write-tools, check-task-commit-attribution.
- Mutation probes (restored; tree verified clean + md5-identical after): reverting the wave-identity fix to the original bug (min(created_at), no task_count) fails `second_wave_emits_its_own_completion`; reverting the approval trigger to `req.prompt.is_some()` fails `two_step_edit_then_approve_triggers_reconciliation`; disabling the Approved-only gate fails the same test. All three prior-round findings have real kill-tests.

## Verified working

- **§6 controller** (`crates/gyre-server/src/reconciliation.rs`): `run_reconciliation` resolves changed registry specs, gates on Approved (a Pending bind is not a rollout — the approval transition owns the wave), creates one deduplicated task per repo (title `Align code with updated {kind} {name} v{version}`, label `meta-spec-reconciliation`, priority Medium, task_type Delegation, spec_path set). Dedup is (workspace, repo, title) against non-terminal tasks; cross-workspace/repo suppression tested absent.
- **§6 triggers**: `PUT /workspaces/{id}/meta-spec-set` diffs old→new pins (new/changed SHA; removals not triggers; BTreeMap canonical serialization keeps re-PUT SHA stable — pinned by `identical_reput_keeps_set_sha_stable`). Registry approval on `PUT /meta-specs-registry/{id}` keys on `approved_content_hash` (stamped server-side at approval, migration 000056 portable SQL), so the two-step edit-then-approve UI flow triggers exactly like the combined flow, and re-approval of unchanged content is a no-op. Version bumps only on prompt change; all three adapters archive version-history rows only when the version changed (`same_version_update_does_not_duplicate_history`).
- **§10 sweep**: `run_conformance_sweep` registered as job `meta_spec_conformance_sweep` (daily default, `GYRE_META_SPEC_SWEEP_INTERVAL_SECS` override) and spawned in main.rs via `spawn_job`. Per workspace with a bound set it compares the active set SHA against `meta_spec_set_sha` in recent attestations (30-day window), creates one deduped drift-review task per workspace+set-sha, emits `meta_spec_drift_detected`, increments `gyre_meta_spec_drift_total`, and creates priority-6 `MetaSpecDrift` notifications for Admin/Developer/Owner members (same role set as the pre-existing `emit_reconciliation_completed` helper). DB queries only.
- **§11 observability**: `reconciliation_started` wave events; `ReconciliationCompleted` emitted by `maybe_emit_reconciliation_completed` when all reconciliation tasks for a workspace are terminal (hooked in REST task transitions, MCP `update_task`, repo-archive cancellation), deduplicated on wave identity (latest wave start + total task count) so second and later waves each emit exactly once; `meta_spec_drift_resolved`, `meta_spec_changed`, `meta_spec_set_updated` events; `gyre_reconciliation_tasks_total{workspace,status}` and `gyre_reconciliation_duration_seconds{workspace}` metrics registered and rendered.
- **Authorization**: registry create/update/delete are Admin-only per-handler (approval is human_only per §1; a non-Admin approval would forge the §6 trigger); workspace-scoped registry listing enforces tenant containment + membership for user tokens following the pre-existing explorer-views pattern. The three stale `abac-exempt-handlers-exemptions.txt` lines are deleted and backed by real enforcement (script green, 0 violations).
- **Prior-round regressions fixed and pinned**: `required` field persists (base behavior retained + regression test); task-170 surfaces byte-identical to base (0 files differ under web/, view_spec.rs, explorer_views.rs, .gitattributes).

## Notes (not blocking)

- `list_meta_specs_registry` skips the workspace-membership check for agent tokens (user_id None), enforcing tenant containment only — identical to the referenced pre-existing explorer-views pattern, not introduced by this candidate.
- Coverage rows for §6/§10/§11 remain `task-assigned` in `specs/coverage/system/meta-spec-reconciliation.md`; consistent with task-170 precedent where row transitions are owned by the audit process, and the repo goal states the matrix reflects reality rather than defining it.
- Wave-identity `min` vs `max` on created_at is equivalent for emission correctness given the task_count component; `max` additionally keeps the duration histogram per-wave accurate.
