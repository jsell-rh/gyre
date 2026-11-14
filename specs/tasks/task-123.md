---
title: "Implement notification entity with full type taxonomy and priority model"
spec_ref: "user-management.md §Notification Entity"
depends_on:
  - task-120
progress: ready-for-review
coverage_sections:
  - "user-management.md §Notification Entity"
commits: ["6b3d464140fa60b5b7b33e91f784fd576558cc80", "12f5495f0461154f1da417295ed7527cddc139d1", "f92a4891661d9c6f23b923d8e4f51e1b335fd496", "deec7953a0c6a6d289d99f8239068ba403c43b06", "27bfb2a699f9bd6f22085b497726e0d32f8f688a", "d6592051175e4ca81cfe2567582c3e5f049759fa"]
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
Original implementation: commits 27bfb2a69, deec7953a0, f92a48916 (re-homed
across base merges 730073ad1/7f0ec2b7c/33ae5e394), lint-gate repair
12f5495f0. Priority-band repair for the verification requirements gap
(attempt 5d7e43ea): commit 6b3d4641 on base 1ee52c43.

### Entity model (gyre-common/src/notification.rs)

- `Notification` carries every spec field: `id`, `user_id`,
  `notification_type`, `title`, `body`, `source`, `priority`,
  `action_url`, `read`, `read_at`, `created_at` — plus the HSI §2 inbox
  fields (`workspace_id`, `priority_rank`, `resolved_at`, `dismissed_at`,
  `tenant_id`).
- All 18 `NotificationType` variants defined, unified with the HSI §2
  inbox types (41 total, including base-merged `ImpersonationRequest`,
  `ImpersonationAlert`, `PersonaChangePendingApproval`, `SecretExpiring`,
  `WorkspaceOwnerless`, `ComputeTargetUnavailable`). `parse()` accepts
  legacy DB strings (`SpecPendingApproval`, `BudgetWarning`) so existing
  rows keep parsing.
- `NotificationSource` (`entity_type`, `entity_id`, `workspace_id`,
  `repo_id`) replaces the flat `entity_ref`/`repo_id` pair.

### Priority model (repaired per the amended membership table)

- The authoritative type→band table lives in
  `NotificationPriority::for_notification_type` (gyre-common).
  `NotificationType::default_priority()` derives from it — never from
  `from_rank(default_priority_rank())`, which the amendment forbids
  ("deliberately not threshold-mapped onto these bands": the 12 types
  whose rank and band disagree, e.g. SpecApprovalRequested rank 2 → High
  not Urgent, CascadeTestTriggered rank 4 → Low, SecretExpiring rank 7 →
  High, are now banded per the table).
- `gyre-domain`'s channel-routing `for_notification_type` delegates to
  the same table, so the entity band (API `priority_level`, webhook/email
  payloads) and the delivery-gating band can never disagree.
- The SQLite and PostgreSQL adapters reconstruct `priority` from the
  persisted `notification_type` on every read (only the rank is
  persisted); the mem adapter stores the struct whose band was set at
  creation. `from_rank`/`rank_band` remain only as the numeric scale's
  band description for the `?priority=` list-filter widening.
- spec_patrol error findings keep the rank-3 Inbox position; their band
  is the type's Medium per the table (the `from_rank(3)` High override
  was removed).
- Module doc, `default_priority_rank` doc, coverage row, and the spec
  amendment's code-location pointer updated; no surface still claims the
  band-of-rank invariant.

### Persistence & creation paths (unchanged by the repair)

- Migration 000059 (SQLite + PostgreSQL, portable SQL): `entity_type`,
  `action_url`, `read`, `read_at` columns; read state backfilled.
- Port methods `mark_read` / `has_active_notification` implemented in
  SQLite, PostgreSQL, and mem; retention counts marked-read as read.
- Typed emitters: MrMerged, AgentFailed, SpecApprovalRequested,
  PersonaApprovalRequested, AgentBudgetWarning/Exhausted (threshold
  crossing, deduped per period), GatePassed (deduped per user+MR),
  MergeQueuePaused, BreakingChangeDetected, SpecDriftDetected,
  DependencyStale (stale transition only), InvitationReceived,
  MembershipChanged. `SecurityFinding`/`SystemAlert` have no emitting
  feature yet (no security scanner / system-alert source exists) —
  documented, not faked.
- `notify_rich` auto-derives `action_url` deep links via workspace slug +
  repo name to the SPA entity routes; never fabricates a link.

### API

- `GET /api/v1/users/me/notifications` and `/api/v1/notifications`
  expose `priority_level` (type-derived band), numeric `priority` rank,
  `entity_type`, `action_url`, `read`/`read_at`. `POST
  /api/v1/notifications/:id/read|unread` with per-handler auth.

### Test evidence

- gyre-common (10 notification tests, green): 18-variant round-trip,
  legacy-name parsing, spec level examples, full amended-membership
  probe `entity_priority_matches_amended_membership_table` (all 41
  variants, table spelled out independently of the implementation so
  drift fails the probe).
- gyre-domain: 509 tests green (delegation compiles; band behavior
  covered by the dispatcher suite).
- gyre-adapters SQLite (11 notification tests, green, real migration):
  new `read_path_reconstructs_band_from_type_not_rank` pins rank 2 →
  High, 4 → Low, 7 → High, 9 → Medium on the read path — fails if the
  adapter regresses to `from_rank`.
- gyre-server (type-checked via `cargo check --all-targets`; test
  execution recorded for the independent verifier — this sandbox cannot
  compile gyre-server test binaries within the 4 GiB budget): suites to
  run — `notification_dispatcher` (incl. new
  `entity_band_equals_channel_routing_band` over the 12 repaired types),
  `notifications`, `spec_patrol`, `workspace_ownership`, `users`
  (priority_level now High for SpecApprovalRequested), `budget`,
  `personas`, `dep_staleness`, `merge_queue`, `impersonation`,
  `secret_rotation`, `merge_processor`, `git_http`.
- Changed-line gates on base 1ee52c43: `check-rustfmt-diff.py` clean,
  `check-clippy-diff.py` clean (8 Rust files); `check-arch.sh` and
  `check-mem-port-contracts.sh` pass. Evidence:
  /tmp/stage/review-evidence (focused-tests.txt, gates.txt).
