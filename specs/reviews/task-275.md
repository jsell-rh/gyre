# Review: TASK-275 — Apply operator decision for spec-44383e89f5c1275953582cde (amend database-migrations.md table list)

**Reviewer:** Independent review agent
**Commit:** `d5e6b36a` (base `28a3a40b`)
**Verdict:** `approved`

---

## Scope verification

`git diff --name-only 28a3a40b..d5e6b36a` touches exactly three files, all
within the authorized amendment paths:

- `specs/development/database-migrations.md`
- `specs/system/user-management.md`
- `specs/tasks/task-275.md` (new task bookkeeping)

No production code, tests, scripts, CI, exemptions, or verification gates were
changed. Single commit; HEAD matches the assigned candidate SHA.

## Acceptance-criteria verification

- **Canonical spec agrees with the recorded choice.** The "Which Tables Get
  tenant_id" section carries the versioned decision record
  (`3e7bbde727f5dcf55afb0217`, option_a, 2026-11-19, operator `jsell`),
  removes `projects` from the tenant-scoped list, moves `users` into a new
  tenant-bound category, and preserves the prior classification verbatim under
  a labelled "Historical classification (superseded)" heading. Every schema
  fact cited was independently re-verified against the repo:
  - `2026-03-24-000013_remove_projects/up.sql` — `DROP TABLE IF EXISTS
    projects`; recreated `repositories` has `workspace_id TEXT NOT NULL`.
  - `2026-10-08-000058_user_entity_profile_fields/up.sql` — `ALTER TABLE
    users ADD COLUMN tenant_id TEXT` (nullable, no default).
  - `2026-11-13-000065_user_tenant_scope/up.sql` — `CREATE INDEX
    idx_users_tenant ON users (tenant_id)`.
  - `crates/gyre-server/src/auth.rs` — API-key path fail-closes on users with
    no tenant binding ("user has no tenant binding; re-provision via POST
    /api/v1/users"), with tests `api_key_auth_rejects_user_without_tenant_binding`
    and the AuthFailure audit-event test.
  - `specs/system/user-management.md` — `pub tenant_id: Id`; "Cross-tenant
    sharing: Not supported."
  - `specs/index.md` M33 — "Project entity removed; Workspace becomes the
    primary grouping entity."
- **Parent task no longer instructed toward contradictory behavior.** The
  parent is a controller-held spec-planner record, correctly not created as a
  repo task file. The two authorized contradictions are closed at the source;
  the two directly contradicting neighbors in the same spec (Tenant
  Enforcement example using the dropped `projects`; Tenant Resolution's
  "user's active tenant") were aligned with the same correction. Residual
  `projects`/users-as-global references appear only in labelled historical
  text.
- **Choice, rationale, authority recorded.** The full approved decision JSON
  (options, recommendation, selection by `jsell` via local cockpit) is
  retained verbatim in `specs/tasks/task-275.md`, with only option_a marked
  authorized.
- **Honest status.** The shipped notes claim no new implementation, state the
  amendment documents already-shipped behavior, and explicitly flag residual
  limitations (siem_targets TBD left unchanged as out of scope; the list
  remains a hand-maintained snapshot). Verified accurate.
- **No production code changed.** Confirmed by the diff.

Registry check: `specs/task-spec-inputs.json` contains zero references to
`database-migrations.md`, and the amended section keeps its heading, so
leaving the registry unchanged is correct; this task's own GOAL.md binding is
intact. No repo script validates the spec's hand-maintained table list
(`grep -rl database-migrations scripts/` — no matches), so the amendment
cannot break CI checks; `scripts/check-tenant-filter.sh` passes (exit 0) and
derives tenant-column tables from the migrations, not from this spec.

No test suite run is required for a documentation-only change touching no
compiled or executed surface; remote verification runs GitHub checks on the
merged result as usual.

## Non-blocking observation (recorded for the controller's parent reassessment)

The retained "Tenant-scoped (NOT NULL tenant_id)" list still names eight
tables that, per the migrations, carry no `tenant_id` column at all:
`review_comments`, `reviews`, `merge_queue`, `agent_commits`,
`agent_worktrees`, `audit_events`, `quality_gates`, `gate_results`. Only
migration `2026-03-20-000002_add_tenant_id` adds NOT NULL `tenant_id` (to
repositories, agents, tasks, merge_requests, activity_events,
analytics_events, cost_entries — plus the since-dropped projects), and
`scripts/check-tenant-filter.sh` lists those eight in its "tables with no
tenant_id column — spec §3 gap class" backlog (see
`/tmp/stage/review-evidence/probe-tenant-filter-lint.txt`).

This inaccuracy **predates** the candidate — the pre-amendment list claimed
"have tenant_id" for the same entries — and is outside option_a's authorized
two-entry scope: reclassifying those tables requires product decisions
(structural isolation vs. new migrations vs. backlog) that belong to the
parent reconciliation task and `hierarchy-enforcement.md` §3, not to this
requirements amendment. The amendment's closing note ("the migrations remain
ground truth for which tables carry tenant_id; check-tenant-filter.sh derives
the set mechanically") correctly directs readers to the mechanical
derivation. The controller should pick this up when reassessing the parent
task against the corrected contract.

## Probes

Evidence saved under `/tmp/stage/review-evidence/`:
- `probe-facts.txt` — diff scope, all cited migration/auth/spec facts,
  registry check, script-coupling check.
- `probe-tenant-filter-lint.txt` — `bash scripts/check-tenant-filter.sh`
  (exit 0) with the no-tenant_id backlog listing.
