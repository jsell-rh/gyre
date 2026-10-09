---
title: "Implement reconciliation controller and conformance sweep background job"
spec_ref: "meta-spec-reconciliation.md §6, §10"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §6 Reconciliation: The Slow Rollout"
  - "meta-spec-reconciliation.md §10 Conformance Sweeps (Steady State)"
  - "meta-spec-reconciliation.md §11 Observability"
commits: ["7aa240c0fdd773af954a446b0b0fabc76e3bcbd9", "9c051f3203235f84ef8ccf55947d89a05ea60ed2", "c399d1b44f1b74a42ea0d14ed7bc5ed1c1d8a571", "3ead34c467ba150dae67d8d2d9b34941ca9385c3", "86a98fb1fa7de027fcf0cc81666d3d7415e7900f", "170a785d9d059d4a54dae90664ce1c4e8c804aac", "2197283c0e82fd3d33293758008e1acad3676531", "0299db7cf0b96caea52261ff341097c24082afcd", "2ba1b5bd644071202c19a8642907850bdd9ff645", "cf16edcedb8bc0a68ef4e52bcfc7c5b4e86b49e7", "14798296c463663ccb5b21b8daa36946c3b5f2bb"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §6 — Reconciliation:

> When a meta-spec change is approved and rolled out, the reconciliation controller:
> 1. Identifies all repos in the affected workspace(s) that are bound to the changed meta-spec
> 2. For each repo, creates a reconciliation task: "Align code with updated meta-spec {name} v{N}"
> 3. Task is assigned to the repo orchestrator, which spawns an agent to assess compliance
> 4. Agent produces a diff showing what needs to change (if anything)

From §10 — Conformance Sweeps:

> A background job periodically (default: daily) sweeps all repos to verify their code conforms to the active meta-spec set. This catches drift that accumulates between explicit reconciliation runs.

## Implementation Plan

1. **Create reconciliation controller** (`crates/gyre-server/src/reconciliation.rs` — new file):
   - Function `run_reconciliation(state: &AppState, workspace_id: Id, meta_spec_path: &str, new_version: u32)`
   - Query all repos in the workspace whose meta-spec set references the changed meta-spec
   - For each affected repo, create a reconciliation task (title: "Align code with updated {kind} {name} v{version}")
   - Label: `meta-spec-reconciliation`, priority: Medium
   - Emit `ReconciliationCompleted` message (infrastructure already exists in lib.rs)

2. **Wire reconciliation trigger** into meta-spec update flow:
   - When a meta-spec is approved via `POST /api/v1/meta-specs/{id}/approve` or when a meta-spec set is updated via `PUT /api/v1/workspaces/{id}/meta-specs/set`, trigger `run_reconciliation()`
   - Only trigger when the meta-spec version changes (not on re-approval of same version)

3. **Add conformance sweep background job** (register in `crates/gyre-server/src/jobs.rs`):
   - Job name: `meta_spec_conformance_sweep`
   - Default interval: 24 hours (86400 seconds)
   - For each workspace, compare the active meta-spec set SHA against the meta_spec_set_sha in recent agent commits
   - If drift is detected (code was produced under an older meta-spec set), create a drift-review task
   - Emit `MetaSpecDrift` notifications (notification type already exists)

4. **Add observability**:
   - Emit structured log events for reconciliation start/complete
   - Use existing ReconciliationCompleted MessageKind for event bus notifications
   - Add MetaSpecDriftDetected to domain events if not present

5. **Tests**:
   - Unit test: reconciliation creates tasks for affected repos
   - Unit test: repos not referencing the changed meta-spec are not affected
   - Unit test: conformance sweep detects version drift
   - Unit test: duplicate reconciliation tasks are not created (deduplication)

## Acceptance Criteria

- [ ] Meta-spec approval triggers reconciliation task creation for affected repos
- [ ] Reconciliation tasks have correct title, labels, and priority
- [ ] Conformance sweep background job runs on schedule (configurable interval)
- [ ] Conformance sweep creates drift-review tasks when meta-spec version mismatch detected
- [ ] ReconciliationCompleted events emitted on completion
- [ ] MetaSpecDrift notifications sent to workspace members
- [ ] Duplicate reconciliation tasks are deduplicated
- [ ] Tests cover trigger, sweep, and deduplication

## Agent Instructions

- Read `crates/gyre-server/src/api/meta_specs.rs` for existing meta-spec approval and set-update handlers
- Read `crates/gyre-server/src/lib.rs` for `emit_reconciliation_completed()` — the notification infrastructure is ready
- Read `crates/gyre-server/src/jobs.rs` for how to register a new background job
- Read `crates/gyre-common/src/notification.rs` for MetaSpecDrift notification type
- Read `crates/gyre-common/src/message.rs` for ReconciliationCompleted MessageKind
- The task creation pattern is in `crates/gyre-server/src/git_http.rs` (spec lifecycle task creation) — follow the same deduplication pattern

## Shipped

**Reconciliation controller (§6)** — `crates/gyre-server/src/reconciliation.rs`:
`run_reconciliation(workspace, changed_paths)` resolves each changed registry
meta-spec to its kind/name/version, computes blast radius (every repo in the
workspace bound via the workspace meta-spec set), and creates one
deduplicated reconciliation task per repo: title `Align code with updated
{kind} {name} v{version}`, label `meta-spec-reconciliation`, priority Medium,
task_type Delegation (repo-orchestrator signal-chain), spec_path set. Dedup
is (workspace, repo, title) against non-terminal tasks; a pending task in
another repo/workspace never suppresses this one. `diff_meta_spec_set`
computes changed paths (new/changed SHA pins; removals are not triggers).

**Triggers wired (§6)** — `PUT /api/v1/workspaces/{id}/meta-spec-set`
diffs old→new set and runs reconciliation only on version change
(identical re-PUT is a no-op; pinned by tests incl. BTreeMap
canonical-serialization so the set SHA is stable across re-PUTs). Registry
content update + approval on `PUT /api/v1/meta-specs-registry/{id}`
reconciles every workspace whose set binds the changed spec.

**Conformance sweep (§10)** — `run_conformance_sweep` registered as job
`meta_spec_conformance_sweep` (daily default, `GYRE_META_SPEC_SWEEP_INTERVAL_SECS`
override) and spawned in main.rs. For each workspace with a bound set it
compares the active set SHA against `meta_spec_set_sha` in recent
authorization attestations (30-day window); drifted repos get one deduped
drift-review task per workspace+set-sha, `meta_spec_drift_detected` events,
Prometheus `gyre_meta_spec_drift_total`, and priority-6 `MetaSpecDrift`
notifications to Admin/Developer/Owner members. DB queries only — no agent
spawns (§10: "the sweep is cheap").

**Observability (§11)** — `reconciliation_started` wave-start events,
`ReconciliationCompleted` emitted by `maybe_emit_reconciliation_completed`
when the last reconciliation task for a workspace goes terminal (hooked in
REST task transitions, MCP update_task, repo archive cancellation;
deduplicated against persisted wave_started_at),
`meta_spec_drift_resolved`, `meta_spec_changed`, `meta_spec_set_updated`;
`gyre_reconciliation_tasks_total{workspace,status}` and
`gyre_reconciliation_duration_seconds{workspace}` metrics.

**Per-handler authorization repair (this round)** — the earlier commits in
this task's file shifted `meta_specs.rs` registry handlers off their
line-anchored `abac-exempt-handlers-exemptions.txt` entries, failing
`check-abac-exempt-handlers.sh` (3 F1b violations). Fixed with real
authorization instead of re-anchoring: create/update/delete registry
handlers are Admin-only (same NEW-26 rule as `put_meta_spec_set`; a
non-Admin approval would forge the §6 trigger, and approval is `human_only`
per spec §1); `list` enforces tenant containment + workspace membership for
`scope=Workspace&scope_id=` queries. The three exemption lines are deleted
per the exemption file's own instruction. `domain_events` module
declaration restored (an early wip commit had dropped it out of scope).

**Tests** — 19 reconciliation tests (task-per-repo, dedup, set-diff, sweep
drift/clean/skip/empty-sha, API e2e trigger, SHA stability, wave
completion/idempotency, drift resolution, events) + 30 registry tests
including new `registry_writes_reject_non_admin` (developer-JWT 403 on
create/approve/delete, workspace-scoped list 403 for non-members, admin
path OK). Evidence: `/tmp/stage/review-evidence/task-156-focused-tests.txt`.
Mechanical checks green: check-abac-exempt-handlers (89 handlers, 0
violations), check-abac-route-registry, check-arch, check-mcp-write-tools,
check-dead-message-kinds. Clippy `-D warnings` fails pre-existing in
`gyre-common` at the checkpoint (verified with changes stashed); task diff
does not touch gyre-common. Full workspace suites owned by verification.
