# Review — task-123 (Implement notification entity with full type taxonomy and priority model)

Spec: `user-management.md` §Notification Entity (amended priority-band membership table, task-112 review).
Candidate under review: `f615c788` on base `a4c4c012` (rebase repair for the previously
published head `c52cd64a`, which conflicted with main; merge `cd558788` resolves the
task's type-set band filter with main's HSI §12 `exclude_types` preference exclusion).
Product commits in the delta: `6b3d4641` (band from type, not rank), `90edbb2f`
(`?priority=` filter by type-derived band), merge `cd558788`; the original entity work
(`27bfb2a6`, `deec7953a0`, `f92a48916`, `d659205117`, `12f5495f0`) is already in the
base via mainline history.
Verdict: **approved**.

## Round 1 (this review)

Probes (evidence under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-common notification` → **12 passed, 0 failed** (taxonomy
  round-trip incl. all 18 user-management variants, legacy DB-name parsing, band
  mapping vs the spec's level examples, amended-membership-table probe,
  `NotificationType::all()` enumerates exactly once).
- `cargo test -p gyre-adapters notification` → **13 passed, 0 failed** (real SQLite,
  runs the real `2026-11-08-000059_notification_entity_source` migration): create
  persists source/action_url/read, `mark_read` stamps/clears/is user-scoped,
  `has_active_notification` dedup semantics, retention read/unread split,
  `read_path_reconstructs_band_from_type_not_rank` (rank/band-divergent cases —
  would fail under any `from_rank` derivation), `exclude_types` on list+count.
- `cargo check -p gyre-server` → clean (lib + build.rs web rebuild).
- Guard scripts: `check-migration-versions`, `check-arch`,
  `check-abac-route-registry`, `check-mem-port-contracts`,
  `check-inert-enforcement`, `check-abac-exempt-handlers` (87 handlers) → all OK.
- gyre-server test compilation deferred to the verifier per the review assignment's
  build-budget rule; required commands recorded in
  `review-evidence/deferred-verifier-checks.txt` (incl. `cargo test --all`).

Verified against the task contract:

- **Entity fields: PASS.** `gyre-common/src/notification.rs` `Notification` carries
  every spec field (id, user_id, notification_type, title, body, source, priority,
  action_url, read, read_at, created_at) plus the HSI §2/§8 inbox fields
  (workspace_id, priority_rank, resolved_at, dismissed_at, tenant_id) required by the
  HSI contracts that pre-date this task. `body` is `Option<String>` (HSI §2 optional
  JSON payload) rather than the spec excerpt's bare `String` — documented in the
  coverage row; the field exists and is populated by emitters.
- **18-variant taxonomy: PASS.** All 18 user-management variants defined and
  round-trip tested; unified with the HSI §2 inbox types (41 total via
  `NotificationType::all()`, which the band filter expands from — no hand-maintained
  parallel list). Renamed variants keep legacy DB strings parsing
  (`legacy_db_names`) so pre-unification rows keep working.
- **NotificationPriority: PASS.** Low/Medium/High/Urgent with
  `for_notification_type` matching the amended §Notification Entity membership table
  exactly (spot-verified every band example: Low=MrMerged/GatePassed,
  Medium=DependencyStale/SpecDriftDetected, High=AgentBudgetWarning/GateFailure,
  Urgent=SecurityFinding/AgentEscalation/MrReverted; plus the amendment's full table
  for the HSI types). Crucially the band derives from the TYPE, never the numeric
  rank — the spec amendment explicitly forbids threshold mapping, and the repair
  commits `6b3d4641`/`90edbb2f` removed both rank-derived derivations (entity field
  at creation, `?priority=` filter at read time). The domain channel-routing table
  (`gyre-domain/src/user.rs`) now delegates to the canonical common table, so entity
  band, filter band, and routing band cannot disagree — proven by
  `entity_band_equals_channel_routing_band` and the rank-2-High divergence test in
  `api::users`.
- **NotificationSource: PASS.** {entity_type, entity_id, workspace_id, repo_id};
  `notify_rich` populates entity_type/entity_id/repo_id; workspace-sourced fallback
  uses the workspace id (no fabricated scope).
- **Action URL: PASS.** `deep_link` resolves the workspace slug and repo name from
  real storage and builds SPA entity routes; returns `None` when the workspace
  cannot be resolved (no fabricated links; covered by a dedicated test).
- **Creation paths: PASS.** Spot-verified in the merged tree: `notify_mr_merged`
  (MrMerged), spawn.rs failure path (AgentFailed), budget threshold emitters in
  api/budget.rs + notification_dispatcher.rs (AgentBudgetWarning/Exhausted with
  per-period dedup and Who-Gets-Notified routing), invitations (task-110's
  InvitationReceived, correctly typed), spec_patrol SpecDriftDetected (rank-3 inbox
  position kept, band stays Medium per the amended table — the
  `from_rank(3)` override was correctly removed), secret rotation, trust
  suggestions, workspace ownership, staleness.
- **Merge resolution `cd558788`: PASS.** `list_for_user` in SQLite, PostgreSQL, mem,
  and the Broadcasting wrapper composes the task's exact-match type-set filter
  (empty set matches nothing; short-circuit before SQL) with main's
  `exclude_types` (`ne_all` before limit/offset so paging counts filtered rows).
  The `?priority=` level expands via `notification_type_strings()` (legacy DB names
  included) and intersects with `?type=`; unknown level → 400. The mark-read
  endpoints keep per-handler auth (user-scoped `get` + tenant check), registered as
  ABAC-exempt with the same model as dismiss/resolve. Docs and coverage rows updated
  accurately to the type-derived-band semantics.

Findings: none blocking.

Non-blocking observations (for the controller, not the candidate):

- The working tree carries a pipeline-owned modification to `specs/tasks/task-123.md`
  (progress label + shipped-notes text) that predates this review. Note that the
  working-tree `commits:` frontmatter lists 5 commits and omits the task-labeled
  repair commits `6b3d4641` and `90edbb2f` (both in the candidate's ancestry), and
  its Shipped text still describes the superseded rank-band derivation. The
  candidate commit `f615c788` itself carries the accurate 7-commit attribution and
  accurate text; per `check-task-commit-attribution.sh` semantics the frontmatter
  should list every task-labeled product commit so verifier scoping sees them.
- `exclude_types` (HSI §12 preferences) matches canonical type names only; a
  pre-unification row persisted under a legacy name (e.g. `SpecPendingApproval`)
  would not be hidden by a preference disabling the renamed canonical type. The
  `?priority=` filter handles legacy names; the preference exclusion does not.
  Pre-existing mainline behavior (task-112/125 surface), outside this task's
  §Notification Entity contract — noted for a future task, not a defect here.
- gyre-server suite execution (and `cargo test --all`) deferred to the verifier;
  commands recorded in `/tmp/stage/review-evidence/deferred-verifier-checks.txt`.
  `cargo check -p gyre-server` is clean and every smaller-package probe ran green,
  so the deferral is a budget constraint, not an evidence gap the candidate controls.
