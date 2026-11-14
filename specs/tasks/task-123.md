---
title: "Implement notification entity with full type taxonomy and priority model"
spec_ref: "user-management.md §Notification Entity"
depends_on:
  - task-120
progress: complete
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

Implemented (commits 27bfb2a69, deec7953a0, f92a48916; re-homed onto base
79dfbdde in merge 33ae5e394; lint-gate repair 12f5495f0 after merging base
6cc3df14) — `user-management.md` §Notification Entity is closed in
production code; no stand-ins remain on the creation paths this task owned.

### Entity model (gyre-common/src/notification.rs)

- `Notification` carries every spec field: `id`, `user_id`,
  `notification_type`, `title`, `body`, `source`, `priority`,
  `action_url`, `read`, `read_at`, `created_at` — plus the HSI §2 inbox
  fields (`workspace_id`, `priority_rank`, `resolved_at`, `dismissed_at`,
  `tenant_id`) that the HSI contracts require.
- All 18 `NotificationType` variants are defined, unified with the HSI §2
  inbox types (38 total after the base's `ImpersonationRequest`,
  `ImpersonationAlert`, and `PersonaChangePendingApproval` variants were
  carried over in merge 33ae5e394). Renames per this spec:
  `SpecPendingApproval` → `SpecApprovalRequested`, `BudgetWarning` →
  `AgentBudgetWarning` (rank 7→4, High), `AgentEscalation` 5→2 (Urgent,
  per the spec's explicit level examples). `parse()` accepts the legacy
  DB strings so existing rows keep parsing; a supersession note was
  added to `specs/system/human-system-interface.md` §2.
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
- `notify_rich` takes `entity_type` and auto-derives `action_url` deep
  links through the workspace slug + repo name to the SPA entity routes
  (`/workspaces/:slug/r/:repo/mrs|agents|specs/:id`); no link is
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
  with en.json labels — verified by the full vitest suite.

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
- Onto 774737638 (merge 7f0ec2b7c): resolved the one content conflict
  (`gyre-server/src/api/spawn.rs`, `fail_agent`) by keeping both sides —
  the base's `revoke_agent_task_secrets` call (Platform Model §7
  terminal-run teardown, review F2) ahead of the task's `AgentFailed`
  typed notification with its taxonomy comment.
- Onto 79dfbdde (merge 33ae5e394, this assignment): resolved two content
  conflicts. `gyre-common/src/notification.rs`
  (`default_priorities_match_hsi_table`): kept the rank-based API and
  carried the base's `PersonaChangePendingApproval` rank-2 assertion
  over to it; the base's three new variants (`ImpersonationRequest`,
  `ImpersonationAlert`, `PersonaChangePendingApproval`) merged into the
  taxonomy with ranks 2/1/2. `gyre-server/src/api/mod.rs` (users import
  list): kept the task's notification handlers
  (`mark_notification_read`/`unread`, `dismiss_notification`) and
  dropped the `get_my_agents`/`get_my_tasks`/`get_my_mrs` imports that
  mainline removed. Adapted the base's new notification creation sites
  to the structured source model: git_http persona-push notifications
  (`entity_type "persona"`, slug id, repo-scoped), impersonation
  request/alert notifications (`entity_type "impersonation"`,
  request/session id), and the prompt_assembly stale-pin drift
  notification (`entity_type "spec"`, binding spec id). Base test
  assertions updated to the new field set (`source.entity_type` /
  `entity_id` / `repo_id`, `priority_rank`).

### Test evidence (all green)

- gyre-common: 8 notification tests — 18-variant taxonomy round-trip,
  legacy-name parsing, band mapping matches the spec's level examples
  (Low=merged/passed, Medium=stale/drift, High=budget-warning/gate
  failure, Urgent=security/escalation/reverted), including the
  base-added variants.
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
- Post-rebase probes (merged tree 33ae5e394 on base 79dfbdde): `cargo
  check --all --all-targets` clean; gyre-common 8 notification tests;
  gyre-server filtered suites — notifications, impersonation, persona,
  stale-pin, users — all passing; gyre-adapters full suite 428 passed.
- Verification-repair round (commit 12f5495f0, merged tree on base
  6cc3df14): verification attempt 31ed4a0f failed the changed-line lint
  gates, not behavior. Fixes: (1) the nine files whose task-changed
  lines needed rustfmt were formatted with the gate's exact rustfmt
  invocation; (2) `clippy::double_must_use` on `NotificationRepository`
  — the `async_trait` macro emits a must_use future per method whose
  desugared span covers the whole trait, so the newly inserted
  `mark_read`/`has_active_notification` methods put the lint on changed
  lines — fixed with the same inline allow every sibling `gyre-ports`
  trait carries; (3) `clippy::too_many_arguments` on `list_for_user`
  (8/7 incl. `self`) — the signature is pre-existing at base (HSI §2
  inbox filter surface) and the warning only lands on changed lines
  because the inserted allow line falls inside the desugared method
  span; allowed with a comment per the merge_processor/
  graph_extraction precedent; (4) `clippy::useless_format` on the
  constant-string `format!` in the MergeQueuePaused deep-link assertion
  replaced with the literal. Post-fix gates on base 6cc3df14:
  `check-rustfmt-diff.py` and `check-clippy-diff.py` both clean; focused
  suites re-run green — gyre-common notification (8), gyre-adapters
  notification (6), gyre-server filtered notification/budget/persona/
  impersonation/merge_queue/staleness/graph/spec_patrol/users suites
  (246 + 23 passed, 0 failed). Evidence retained under
  /tmp/stage/review-evidence.
