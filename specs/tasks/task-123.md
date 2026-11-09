---
title: "Implement notification entity with full type taxonomy and priority model"
spec_ref: "user-management.md §Notification Entity"
depends_on:
  - task-120
progress: ready-for-review
coverage_sections:
  - "user-management.md §Notification Entity"
commits: ["f92a4891661d9c6f23b923d8e4f51e1b335fd496", "deec7953a0c6a6d289d99f8239068ba403c43b06", "27bfb2a699f9bd6f22085b497726e0d32f8f688a"]
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

Implemented (commits 27bfb2a69, deec7953a0; rebased onto base fefde294
in merge 730073ad1 + fix f92a48916, then onto base 774737638 in merge
7f0ec2b7c) — `user-management.md` §Notification Entity is closed in
production code; no stand-ins remain on the creation paths this task
owned.

### Entity model (gyre-common/src/notification.rs)

- `Notification` carries every spec field: `id`, `user_id`,
  `notification_type`, `title`, `body`, `source`, `priority`,
  `action_url`, `read`, `read_at`, `created_at` — plus the HSI §2 inbox
  fields (`workspace_id`, `priority_rank`, `resolved_at`, `dismissed_at`,
  `tenant_id`) that the HSI contracts require.
- All 18 `NotificationType` variants are defined, unified with the HSI §2
  inbox types (35 total). Renames per this spec: `SpecPendingApproval` →
  `SpecApprovalRequested`, `BudgetWarning` → `AgentBudgetWarning` (rank
  7→4, High), `AgentEscalation` 5→2 (Urgent, per the spec's explicit
  level examples). `parse()` accepts the legacy DB strings so existing
  rows keep parsing; a supersession note was added to
  `specs/system/human-system-interface.md` §2.
- `NotificationPriority` (Low/Medium/High/Urgent) with
  `default_priority()` per type. It is always the band of the HSI 1–10
  rank (1–2 Urgent, 3–4 High, 5–7 Medium, 8–10 Low), so the two priority
  models can never disagree.
- `NotificationSource` (`entity_type`, `entity_id`, `workspace_id`,
  `repo_id`) replaces the flat `entity_ref`/`repo_id` pair.

### Persistence (migration 000059, both SQLite and PostgreSQL)

- New columns `entity_type`, `action_url`, `read`, `read_at`; read state
  backfilled from resolved/dismissed timestamps. Portable SQL only
  (`check-migration-sql-portability.sh` passes).
- New port methods, implemented in SQLite, PostgreSQL, and mem adapters:
  `mark_read(id, user_id, read)` (user-scoped; stamps/clears `read_at`)
  and `has_active_notification(ws, user, type, entity_ref, since)` for
  event-driven dedup. Retention (`delete_older_than`) now counts
  marked-read notifications as read.

### Creation paths updated to the spec taxonomy

- `MrMerged` — `notify_mr_merged` (was the `SuggestedSpecLink` stand-in
  the code itself flagged with a TODO).
- `AgentFailed` — agent-failure path in spawn.rs (was mis-typed
  `AgentEscalation`).
- `SpecApprovalRequested` — spec pending-approval emitter in
  specs_assist.rs (rename).
- `BreakingChangeDetected` — git_http `process_breaking_changes` (was
  mis-typed `CrossWorkspaceSpecChange`).
- `SpecDriftDetected` — spec patrol error findings (was mis-typed
  `GateFailure`; the rank-3 High override is kept).
- `InvitationReceived` — `invite_member` (was mis-typed
  `TrustSuggestion`).
- New emitters: `PersonaApprovalRequested` (create_persona → owner, or
  workspace Admins/Owners), `AgentBudgetWarning`/`AgentBudgetExhausted`
  (usage-recording threshold crossing ≥80%/≥100%, deduped once per
  budget period, recipients per the Who-Gets-Notified table),
  `GatePassed` (gate executor when the finishing gate makes all gates
  green, deduped per user+MR), `MergeQueuePaused` (pause_merge_queue →
  workspace Admins/Owners), `DependencyStale` (staleness job, on stale
  transition only), `MembershipChanged` (remove_member).
- `notify_rich` now takes `entity_type` and auto-derives `action_url`
  deep links through the workspace slug + repo name to the SPA entity
  routes (`/workspaces/:slug/r/:repo/mrs|agents|specs/:id`); no link is
  fabricated when the workspace cannot be resolved.

### API

- `POST /api/v1/notifications/:id/read` and `/unread` (per-handler auth
  identical to dismiss/resolve: user-scoped repo access + tenant check);
  registered in the ABAC resolver as exempt with per-handler auth.
- The list response adds `priority_level`, `entity_type`, `action_url`,
  `read`, `read_at` while keeping the numeric `priority` rank and
  `entity_ref`/`repo_id` fields the web Inbox and CLI read today.
- `docs/api-reference.md` updated (also fixed the stale
  `PUT /users/me/notifications/{id}/read` row, which never existed in
  code).
- Inbox UI (commit deec7953a0): type normalization and badge variants
  map the renamed types (`SpecApprovalRequested`, `AgentBudgetWarning`,
  legacy names kept for pre-migration rows) and all new taxonomy types,
  with en.json labels — verified by the full vitest suite (1858 passed).

### Not emitted (documented, not hidden)

`SecurityFinding` and `SystemAlert` are defined with priority mapping
but have no emitting feature yet — there is no security scanner or
system-alert source in the platform. They are recorded as such in the
coverage row rather than wired to a fake event.

### Rebase resolutions

- Onto fefde294 (merge 730073ad1, fix f92a48916): resolved the one
  content conflict (`gyre-common/src/notification.rs`,
  `default_priorities_match_hsi_table`) — kept the task's rank-based API
  (`default_priority_rank()` — `default_priority()` now returns the
  four-level `NotificationPriority`) and carried the base's new
  `SecretExpiring` rank-7 assertion over to that API. Adapted the base's
  new `secret_rotation.rs` expiry notifications to the structured source
  model: the dedup check reads `source.entity_id`, and creation sets
  `source.entity_type = "secret"`, `source.entity_id` to the secret id,
  plus `repo_id` for repo-scoped secrets.
- Onto 774737638 (merge 7f0ec2b7c, this assignment): resolved the one
  content conflict (`gyre-server/src/api/spawn.rs`, `fail_agent`) by
  keeping both sides — the base's `revoke_agent_task_secrets` call
  (Platform Model §7 terminal-run teardown, review F2) ahead of the
  task's `AgentFailed` typed notification with its taxonomy comment.
  The `notify_rich` call below the markers already carried the task's
  `NotificationType::AgentFailed` and the 11-argument structured-source
  signature; no other hunk conflicted.

### Test evidence (all green)

- gyre-common: 8 notification tests — 18-variant taxonomy round-trip,
  legacy-name parsing, band mapping matches the spec's level examples
  (Low=merged/passed, Medium=stale/drift, High=budget-warning/gate
  failure, Urgent=security/escalation/reverted).
- gyre-adapters (SQLite, runs the real migration): create persists
  source/action_url/read; mark_read stamps, clears, and is user-scoped;
  has_active_notification matches type/entity/since and respects
  resolve; retention splits read/unread including the marked-read flag.
- gyre-server: budget thresholds (no notification <80%; warning to
  spawner+Admins; deduped within a period; exhaustion to spawner+Owner,
  Urgent), merge-queue pause (Admins/Owners only, High, deep link),
  persona approval (Admins/Owners, not Viewer), staleness (Medium, once
  per stale transition, repo deep link), gate-passed dedup + deep links,
  MrMerged/GateFailure typed notifications with deep links, no
  fabricated link for unresolvable workspaces, mark-read endpoints
  (read/unread cycle + cross-user 404 with no mutation).
- Full suites: gyre-server 1429 lib tests + integration suites,
  gyre-adapters 378, gyre-common 120, gyre-domain 432, gyre-cli 129 —
  all passing. All mechanical checks pass (arch, migration versions and
  portability, ABAC route registry, mem port contracts, scope-default
  and enforcement lints); clippy introduces no new warnings.
- Post-rebase probes (this assignment, merged tree 7f0ec2b7c on base
  774737638): `cargo check --all` clean; gyre-common 8 notification
  tests, gyre-server 34 notification-filtered + 8 secret_rotation lib
  tests, gyre-adapters 6 notification tests — all passing; the full
  gyre-server lib suite ran twice consecutively green (1537/0 both
  runs; one earlier run had a single flaky failure that did not
  reproduce, output retained in
  /tmp/stage/review-evidence/server-lib-tests*.txt). All 20 mechanical
  checks pass (arch, migration versions and portability, ABAC route
  registry and exempt handlers, MCP write tools, dead message kinds,
  mem port contracts, byte-slice, relative paths, fail-open refs,
  commit attribution, scope-default lints, lossy secrets, inert
  enforcement, forwarded headers, in-memory stores, unbounded HTTP,
  forged scope fields). Clippy on gyre-server reports only warnings
  pre-existing in both parents (none in the resolved conflict region).

