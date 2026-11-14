# Review — task-123 (Notification entity with full type taxonomy and priority model)

Spec: `specs/system/user-management.md` §Notification Entity (amended type→band
membership table).
Candidate: `3a65e90fce1cc2386ec14489255f2b4d602eb319` on base
`1ee52c431b24c02e4833ac585147d1695ef26486` (branch
`pipeline/task-123/0cd05eaa61a44a13b41fd2ead8d55d04-1`).
Verdict: **needs-revision** — one finding (F1, `?priority=` list filter
contradicts the `priority_level` it reports); the priority-band repair itself
is faithful, well-tested, and verified clean on every other surface.

## Round 1 (2026-11-14)

### Scope

The base already contains the original task-123 implementation (27bfb2a6,
deec7953a, f92a4891, d659205, 12f5495f — all verified ancestors of base
1ee52c43). The candidate delta is the priority-band repair: 6b3d4641
(`fix(task-123): derive notification priority band from type, not rank`) plus
process commits 908206d2 / 6c14f6c1 (spec amendment note + ready-for-review) /
3a65e90f (commit-list order). Diff: 12 files, +459/−274.

Note on working-tree state: `specs/tasks/task-123.md` carries an uncommitted
modification restoring the assigned task text (progress `complete`, the five
original SHAs). `/tmp/stage/review-before.json` shows this state at
review-stage setup, matching the assigned body in the stage prompt — stage
setup, not implementation tampering. The reviewed candidate is the committed
SHA 3a65e90f, whose task file correctly attributes the repair commit 6b3d4641.

### Probes (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-common notification` → 10 passed. Includes
  `entity_priority_matches_amended_membership_table`, which pins every
  variant's band against an independently spelled-out table (partition
  uniqueness + per-variant assertions on both `default_priority()` and
  `for_notification_type`) — it kills the pre-repair rank-derived band
  (SpecApprovalRequested rank 2 must be High, CascadeTestTriggered rank 4 must
  be Low, SecretExpiring rank 7 must be High).
- `cargo test -p gyre-adapters --lib notification` → 11 passed, real SQLite
  with the real migration. `read_path_reconstructs_band_from_type_not_rank`
  creates rank 2/4/7/9 notifications, reloads through
  `NotificationRepository::get`, and asserts the band reconstructs from the
  persisted type — a genuine read-path bug-killer.
- `cargo test -p gyre-domain` → 8 passed, 0 failed.
- `cargo check -p gyre-server` → clean. Server test compilation deferred to
  the independent verifier per the 4 GiB policy (the build wrapper rejects
  server test builds in-model; a standalone path-dependency probe was likewise
  rejected — recorded, an infrastructure restriction, not a code defect).

### Verified clean

- **The repair is faithful to the amended membership table.** The new
  canonical `NotificationPriority::for_notification_type` in
  `gyre-common/src/notification.rs` replicates the domain's task-112-reviewed
  table variant-by-variant (verified against the removed domain code); the
  domain's channel-routing `for_notification_type` now delegates to it, so the
  entity band and the routing band cannot disagree. `Notification::new`
  derives `priority` from the type; both SQL adapters reconstruct it from the
  persisted `notification_type` on read (sqlite/postgres notification.rs:52-60);
  the mem adapter stores the struct. No production `from_rank`-derived band
  remains anywhere (grep: the only production `rank_band()` consumer is the
  `?priority=` filter — see F1).
- **Downstream consumers updated coherently:** spec_patrol keeps the rank-3
  Inbox position but drops the `from_rank(3)` band override (SpecDriftDetected
  band stays Medium per the amended table; test updated);
  workspace_ownership's WorkspaceOwnerless assertion moved to High; the users
  list test asserts `priority_level: "High"` for the rank-2
  SpecApprovalRequested; the dispatcher test
  `entity_band_equals_channel_routing_band` covers the twelve types whose rank
  and band disagree.
- **Spec/coverage/docs updated consistently** (user-management.md amendment
  note now points at the canonical gyre-common table; api-reference.md
  priority_level wording; coverage row 15 rewritten).
- **No new routes, migrations, port methods, or hexagonal crossings** in the
  delta; domain→common is an allowed dependency.
- SecurityFinding/SystemAlert remain emitter-less by documented record (no
  emitting feature exists; not fakery).

### Findings

- [ ] **F1 (major): `GET /api/v1/notifications?priority=<level>` filters by
  HSI rank band and now contradicts the `priority_level` field returned in the
  same response.** `list_my_notifications_impl`
  (crates/gyre-server/src/api/users.rs:354-368) widens the four-level name via
  `NotificationPriority::rank_band()` into a min/max `priority_rank` SQL
  filter (High → ranks 3–4, Urgent → 1–2), while `NotificationResponse::from`
  (users.rs:288-289) reports `priority_level` from the type-derived band the
  candidate just made authoritative. Deterministic consequences at the
  candidate tree: `?priority=urgent` returns eight rank-2 types labeled
  `"High"` (SpecApprovalRequested, SpecRejected, ConstraintViolation,
  SpecConflict, PersonaApprovalRequested, PersonaChangePendingApproval,
  ImpersonationRequest, WorkspaceOwnerless); `?priority=high` returns
  CascadeTestTriggered (labeled `"Low"`) and CrossWorkspaceSpecChange
  (labeled `"Medium"`) while omitting those eight High types and SecretExpiring
  (rank 7, High). Before the repair, filter and field agreed by construction
  (both `from_rank`); the repair changed one side only, leaving the query
  surface deriving the band from the rank — exactly what the candidate's own
  module doc calls "a spec violation, not a simplification", against the
  repair's stated goal of "one notification, one band, on every surface".
  user-management.md §In-App Notification UI requires "Filter by type,
  priority, workspace, repo" where the entity's `priority` IS the four-level
  band, and docs/api-reference.md (updated by this candidate) documents
  `?priority=` beside the type-derived `priority_level` with no caveat.
  Weak-test aspect (same finding): the only filter test,
  `spec_path_notifications_list_with_filters` (users.rs:2164+), seeds exactly
  the two types whose rank band and type band agree (GateFailure rank 3,
  AgentCompleted rank 9), so it cannot detect the divergence.
  **Repair direction:** make `?priority=` match the type-based band (translate
  the level to its set of `NotificationType` strings and filter on the
  persisted `notification_type`, or post-filter), and add a filter test using
  a rank/band-disagreeing type (e.g. SpecApprovalRequested under
  `?priority=high`). Alternatively, amend the spec/API docs to define
  `?priority=` as a numeric-rank-band filter with a different name — but as
  shipped, the same four level names mean two different things in one
  response.

### Verifier-required checks (deferred server test builds)

- `cargo test --all` (task acceptance criterion).
- `cargo test -p gyre-server --lib` filtered: users (notification
  list/priority_level/mark-read), notification_dispatcher
  (entity_band_equals_channel_routing_band), spec_patrol, workspace_ownership.
- Changed-line lint gates (`check-rustfmt-diff.py`, `check-clippy-diff.py`)
  and the repo check scripts on the integrated tree.
