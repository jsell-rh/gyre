---
title: "Implement notification entity with full type taxonomy and priority model"
spec_ref: "user-management.md §Notification Entity"
depends_on:
  - task-120
progress: ready-for-review
coverage_sections:
  - "user-management.md §Notification Entity"
commits: ["90edbb2fb3e9c9aca75cde3aedfbead0cceedea0", "6b3d464140fa60b5b7b33e91f784fd576558cc80", "d6592051175e4ca81cfe2567582c3e5f049759fa", "12f5495f0461154f1da417295ed7527cddc139d1", "f92a4891661d9c6f23b923d8e4f51e1b335fd496", "deec7953a0c6a6d289d99f8239068ba403c43b06", "27bfb2a699f9bd6f22085b497726e0d32f8f688a"]
---

## Spec Excerpt

From `user-management.md` §Notification Entity:

```rust
pub struct Notification {
    pub id: Id,
    pub user_id: Id,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    pub source: NotificationSource,
    pub priority: NotificationPriority,
    pub action_url: Option<String>,  // Deep link to relevant page
    pub read: bool,
    pub read_at: Option<u64>,
    pub created_at: u64,
}

pub enum NotificationType {
    SpecApprovalRequested,
    PersonaApprovalRequested,
    AgentEscalation,
    AgentBudgetWarning,
    AgentBudgetExhausted,
    AgentFailed,
    GateFailure,
    GatePassed,
    MrMerged,
    MrReverted,
    MergeQueuePaused,
    BreakingChangeDetected,
    SpecDriftDetected,
    DependencyStale,
    SecurityFinding,
    InvitationReceived,
    MembershipChanged,
    SystemAlert,
}

pub enum NotificationPriority {
    Low,       // Informational (MR merged, gate passed)
    Medium,    // Needs attention eventually (stale dependency, spec drift)
    High,      // Needs attention soon (budget warning, gate failure)
    Urgent,    // Needs attention now (security finding, agent escalation, MR reverted)
}

pub struct NotificationSource {
    pub entity_type: String,  // "agent", "mr", "spec", "gate", "budget"
    pub entity_id: String,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,
}
```

## Implementation Plan

1. **Audit existing notification model:**
   - Check existing `NotificationType` in `gyre-common/src/notification.rs`
   - Check existing notification storage and API
   - Compare existing types against spec's 18-variant taxonomy

2. **Enhance NotificationType:**
   - Add missing variants from spec taxonomy
   - Existing 10 variants (per HSI coverage) → spec requires 18
   - Missing variants likely include: PersonaApprovalRequested, AgentBudgetExhausted, AgentFailed, GatePassed, MrReverted, MergeQueuePaused, BreakingChangeDetected, DependencyStale

3. **Add NotificationPriority:**
   - Low, Medium, High, Urgent enum
   - Map each NotificationType to a default priority per spec

4. **Add NotificationSource:**
   - Structured source tracking: entity_type, entity_id, workspace_id, repo_id
   - Replaces any free-form source fields

5. **Enhance Notification entity:**
   - Ensure all spec fields present: title, body, source, priority, action_url, read, read_at
   - Migration if needed for new columns

6. **Update notification creation paths:**
   - Audit all places notifications are created
   - Ensure they use the correct NotificationType, priority, and source

## Acceptance Criteria

- [ ] Notification entity has all spec'd fields (id, user_id, notification_type, title, body, source, priority, action_url, read, read_at, created_at)
- [ ] All 18 NotificationType variants defined
- [ ] NotificationPriority enum: Low, Medium, High, Urgent
- [ ] NotificationSource struct with entity_type, entity_id, workspace_id, repo_id
- [ ] Default priority mapping per notification type
- [ ] Action URL populated for deep-link navigation
- [ ] Existing notification creation paths updated to use new types
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Notification Entity. Existing notification types: `gyre-common/src/notification.rs`. Notification storage: grep for `NotificationRepository` in `gyre-ports/` and `gyre-adapters/`. Notification creation: grep for `create_notification\|Notification::new\|NotificationType::` in `gyre-server/`. Notification API: `gyre-server/src/api/users.rs` (get_my_notifications, get_notification_count).

## Shipped

`user-management.md` §Notification Entity is closed in production code.
Original implementation: 27bfb2a69, deec7953a0, f92a48916 (re-homed across
base merges 730073ad1/7f0ec2b7c/33ae5e394), lint-gate repair 12f5495f0.
Priority-band repair (attempt 5d7e43ea gap): 6b3d4641 on base 1ee52c43.
`?priority=` filter repair (review F1, attempt 0cd05eaa): 90edbb2f on base
bfc36c9c.

### Entity model (gyre-common/src/notification.rs)

- `Notification` carries every spec field: `id`, `user_id`,
  `notification_type`, `title`, `body`, `source`, `priority`,
  `action_url`, `read`, `read_at`, `created_at` — plus the HSI §2 inbox
  fields (`workspace_id`, `priority_rank`, `resolved_at`, `dismissed_at`,
  `tenant_id`) the HSI contracts require.
- All 18 `NotificationType` variants are defined, unified with the HSI §2
  inbox types (41 total after the base merges carried `ImpersonationRequest`,
  `ImpersonationAlert`, `PersonaChangePendingApproval`, `SecretExpiring`,
  `WorkspaceOwnerless`, `ComputeTargetUnavailable` over). Renames per this
  spec: `SpecPendingApproval` → `SpecApprovalRequested`, `BudgetWarning` →
  `AgentBudgetWarning`, `AgentEscalation` 5→2 (Urgent). `parse()` accepts
  the legacy DB strings so existing rows keep parsing; supersession note in
  `specs/system/human-system-interface.md` §2. `NotificationType::all()` is
  the single enumeration of the taxonomy.
- `NotificationPriority` (Low/Medium/High/Urgent). The band derives from
  the notification TYPE via `for_notification_type` (the amended §Notification
  Entity membership table) — never from the HSI §8 numeric rank, which the
  amendment deliberately does not threshold-map onto the bands
  (`default_priority`, the SQL adapters' read path, and the domain
  channel-routing table all delegate to the same function, so one
  notification carries one band on every surface). `priority_rank` remains
  the HSI §8 Inbox-ordering scale.
- `NotificationSource` (`entity_type`, `entity_id`, `workspace_id`,
  `repo_id`) replaces the flat `entity_ref`/`repo_id` pair.

### Persistence (migration 000059, SQLite + PostgreSQL, portable SQL)

- Columns `entity_type`, `action_url`, `read`, `read_at`; read state
  backfilled from resolved/dismissed timestamps.
- Port methods `mark_read` and `has_active_notification` implemented in
  SQLite, PostgreSQL, and mem; retention counts marked-read as read.
- `list_for_user`'s type filter is an exact-match type-string SET on the
  persisted `notification_type` column (empty set matches nothing),
  implemented with diesel `eq_any` in the SQL adapters and `contains` in
  mem — the surface the `?priority=` band filter pushes down through.

### Creation paths (typed per the taxonomy)

- MrMerged, AgentFailed, SpecApprovalRequested, PersonaApprovalRequested,
  AgentBudgetWarning/AgentBudgetExhausted (threshold crossing, deduped per
  period), GatePassed (deduped per user+MR), MergeQueuePaused,
  BreakingChangeDetected, SpecDriftDetected, DependencyStale (stale
  transition only), InvitationReceived, MembershipChanged.
  `SecurityFinding`/`SystemAlert` have no emitting feature yet (no security
  scanner / system-alert source exists) — documented in the coverage row,
  not faked.
- `notify_rich` auto-derives `action_url` deep links through the workspace
  slug + repo name to the SPA entity routes; no link is fabricated when the
  workspace cannot be resolved.

### API (users.rs)

- `GET /api/v1/notifications` (spec path) and `/users/me/notifications`
  (legacy): paginated, filterable by `?type=`, `?priority=`, `?workspace_id=`,
  `?repo_id=`, `?min_priority=&max_priority=` (numeric ranks).
- `?priority=<level>` filters by the TYPE-derived band — the same
  `for_notification_type` derivation as the response's `priority_level`
  field — never by the HSI rank band (review F1 repair, 90edbb2f): the
  handler expands the level via `NotificationPriority::notification_type_strings()`
  (canonical strings plus the legacy DB names of renamed variants, so
  pre-unification rows filter by their band) into the port's type-set
  filter and intersects with `?type=`. Unknown level → 400.
- `POST /api/v1/notifications/:id/read|unread`, mark-all-read,
  unread-count, preferences, SSE stream — per-handler auth identical to
  dismiss/resolve; registered in the ABAC resolver as exempt with
  per-handler auth.
- Response carries `priority_level` (type-derived band), numeric `priority`
  rank, structured source, `action_url`, `read`/`read_at` alongside the
  fields the web Inbox and CLI read today. `docs/api-reference.md` and the
  user-management coverage rows updated.

### Test evidence

- gyre-common (128 tests green, incl. 12 notification tests): taxonomy
  round-trip, legacy-name parsing, amended membership table pinned
  per-variant, `all()` enumerates the taxonomy exactly once, and the band
  type-string sets partition the taxonomy with legacy names riding their
  variant's band and the rank/band divergences pinned (SpecApprovalRequested
  High, CascadeTestTriggered Low, SecretExpiring High).
- gyre-adapters SQLite (real migration, 12 notification tests green):
  create/mark_read/dedup/retention, read-path band reconstruction from the
  persisted type, and the type-set filter matching the persisted column —
  including a raw legacy `SpecPendingApproval` row — with empty-set
  semantics.
- gyre-server: type-checked via `cargo check --all --all-targets` (clean);
  test execution deferred to the independent verifier (this sandbox's
  build wrapper blocks server test codegen within the 4 GiB budget).
  Required probes recorded in /tmp/stage/review-evidence/deferred-probes.txt:
  `spec_path_notifications_list_with_filters` (now seeds the rank-2
  SpecApprovalRequested divergence: `?priority=high` returns it labeled
  `priority_level: High`, `?priority=urgent` returns none of the seeded
  items, `?type=`×`?priority=` intersect to empty) plus the notifications,
  users, secret_rotation, trust_suggestion, constraint_check, and
  workspace_ownership suites covering the updated `list_for_user` call
  sites.
- Changed-line gates on base bfc36c9c: `check-rustfmt-diff.py` and
  `check-clippy-diff.py` both clean; `check-arch.sh` and
  `check-mem-port-contracts.sh` pass. Evidence retained under
  /tmp/stage/review-evidence.

### Rebase resolutions (durable history)

- Onto fefde294 (merge 730073ad1, fix f92a48916): kept the task's
  rank-based API in `gyre-common/src/notification.rs`; adapted the base's
  `secret_rotation.rs` expiry notifications to the structured source model.
- Onto 774737638 (merge 7f0ec2b7c): `spawn.rs` `fail_agent` — kept both
  sides (base's `revoke_agent_task_secrets` + the task's `AgentFailed`
  typed notification).
- Onto 79dfbdde (merge 33ae5e394): `notification.rs` and `api/mod.rs` —
  kept the task's notification handlers and rank-based API, carried the
  base's new variants and creation sites over to the structured source
  model.
- Priority-band repair 6b3d4641 (base 1ee52c43): the entity band derives
  from the type via `for_notification_type`, never `from_rank`; adapters
  reconstruct the band from the persisted type on read; domain
  channel-routing delegates to the same table.
- Filter repair 90edbb2f (base bfc36c9c): `?priority=` band filter moved
  from rank-band widening to the type-string set (review F1).
