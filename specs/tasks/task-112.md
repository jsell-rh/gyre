---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["ea8dd97b7841718662b84f6501435a10c04cb374", "8269e906749f6b995a429916c868d5c829cfe77a", "7601f79ab5fd116a7e22fb0a137b18fb895953bc", "9558f4091bb2d0c01688e65a4cdb7d0c4b93f651"]
---

## Shipped

Real production implementation of user-management.md §Delivery Channels,
§Who Gets Notified, §Notification Routing for Agent Escalations.

### Behavior

**Domain types** (`gyre-domain/src/user.rs`): `NotificationChannels` with
`in_app` (always true), `email: EmailConfig`, `webhook: Option<WebhookConfig>`,
`slack: Option<SlackConfig>`; `NotificationPriority` (Low..Urgent, ordered),
`DigestFrequency`.

**Dispatcher** (`gyre-server/src/notification_dispatcher.rs`): fans each
persisted in-app notification out to the recipient's configured channels with
per-channel `min_priority` threshold filtering. The band derives from the
notification TYPE via `NotificationPriority::for_notification_type` — never
from the HSI §8 numeric priority (no numeric threshold reproduces the specced
membership; the full type→band table is amended into user-management.md
§Notification Entity, with SecretExpiring and PersonaChangePendingApproval
recorded as High via merge-seam footnotes). Webhook POSTs the JSON payload
signed with HMAC-SHA256 (`X-Gyre-Signature`, ring). Slack POSTs to the
incoming webhook URL (channel override honored). Email queues a durable
outbox entry in `kv_store` (`email_outbox` ns) for the digest sender — the
interface contract per the task plan; SMTP transport is deployment's concern.
All outbound HTTP is latency-bounded (reqwest with 10 s timeout).

**Routing engine** (§Who Gets Notified): `escalation_recipients` implements
the agent-escalation chain — `spawned_by` primary; offline spawner adds
workspace Admins; Urgent always adds workspace Owners (dedup'd). Helpers
cover persona approval → owner, merge queue paused → all Admins/Owners,
budget warning → spawner + Admins, budget exhausted → spawner + Owner,
security finding → Workspace Owner + tenant Admin. Decomposition boundary:
the budget warning/exhausted and security finding helpers have no production
emission triggers yet — owned by task-191 (platform-model §Enforcement
Behavior) and task-137 (agent-gates §Gate Failure Feedback); tested but not
live until the triggers land. Spec approval, gate failure, MR merged/reverted,
and invitation rows ride existing creation paths. The "Breaking change
detected → dependent repo team leads" row cannot be routed as specced — no
team-lead concept in the domain; user-management.md is amended to record the
interim superset routing (all members of the dependent repo's workspace) and
the dependency on Teams gaining a lead concept (task-122/125).

**Wiring**: `notifications::notify`/`notify_rich` fan out through
`dispatch_to_channels`, so every emitting path (gate failure, spec patrol,
merge queue, abandoned branch, trust suggestion, spec-link staleness,
git_http, mcp, divergence/mirror-sync spec assertions, secret expiry)
delivers to configured channels. `spawn.rs` calls `notify_agent_escalation`
on Overseer escalation.

**Preferences API**: `GET/PUT /api/v1/notifications/preferences` (self-scope,
per-handler auth; ABAC-exempt with documented self-scope invariant). PUT
rejects `in_app: false` (spec: "can't disable") and non-absolute webhook/Slack
URLs. Storage: `user_channel_preferences` table (migration
`2026-10-08-000059_user_channel_preferences`; portable SQLite/PG SQL),
`UserChannelPreferenceRepository` port with SQLite, Postgres, and mem
adapters (all enforce the upsert contract).

**Stored-preferences shape compatibility** (review repair, findings
2facf1217): the flat→nested `NotificationChannels` shape change initially
left pre-task-112 `users.preferences` rows unreadable — the adapters'
`.ok().unwrap_or_default()` read path silently reset them to defaults,
losing user customizations on upgrade, and the 000058 backfill test failed.
Repaired on both layers: (1) a custom `Deserialize` for `NotificationChannels`
accepting both the nested shape and the pre-task-112 flat shape
(`in_app`/`email_enabled`/`email_digest`; flat maps onto `EmailConfig` with
the default Medium band), so no stored row or old-client API payload can
fall into the reset path; (2) migration
`2026-11-09-000060_user_preferences_channel_shape` (next unused sequence;
portable REPLACE/LIKE SQL only) converging stored rows to the nested shape
while preserving stored email values across all 8 combinations —
already-nested and NULL rows untouched, `down.sql` reverses the lossless
subset. The 000058 test now runs 000060 and asserts byte-equal convergence
to the current default serialization; a dedicated regression drives the real
000060 up.sql against flat-default/flat-custom/flat-in_app-false/nested/NULL
rows.

### Merge rounds (latest: 7edbddb4c, merge of main f7d61ef58)

Merge seams resolved across rounds (c4ebdc2d0, 6703a0353, and earlier):
`api/spawn.rs` keeps both main's task-scoped secret revocation and our full
§Agent Escalations chain (`notify_agent_escalation` subsumes the plain
spawned_by notify); `git_http.rs` adopts main's hoisted workspace resolution
(behavior identical — never fabricate a scope); `gyre-common/src/notification.rs`
keeps both sides' appended `NotificationType` variants;
`check-fabricated-scope-defaults.sh` takes main's lowered exemption count.
This round's seam: main's task-116 added
`NotificationType::PersonaChangePendingApproval`, breaking the exhaustive
`for_notification_type` match — banded High (same pending-approval class as
`SpecPendingApproval`), recorded in the spec's type→band table (footnote 2)
and asserted in the dispatcher's band-membership test. Generated `web/dist`
assets are restored from the assignment base by the executor and rebuilt
remotely.

### Review repair history

- Findings 2facf1217 (commit 9558f4091, this round): the flat→nested channel
  shape change had no data migration or serde compatibility for persisted
  `users.preferences` JSON — silent defaults-reset on upgrade plus the
  failing 000058 backfill test in `gyre-adapters`. Repaired with the compat
  `Deserialize` + migration 000060 described above; the full
  `gyre-adapters` lib suite (the review's reproduce command) is green.
- Findings 63609fed (commit ea8dd97b7): changed-line rustfmt violations and
  a `clippy::let_and_return` in test helpers — fixed, no behavior changes.
- Findings 07fd53c9 (commit 8269e9067): the numeric
  `NotificationPriority::from_band` threshold mapping made the §Agent
  Escalations "Urgent → Owners" step unreachable; replaced by the per-type
  `for_notification_type` mapping with the spec amendment table and the
  full-path regression test. A dead channel-prefs defaults test now executes.

### Test evidence (repair-round HEAD 9558f4091, this sandbox)

- `cargo test -p gyre-adapters --lib`: 394 passed, 0 failed, 12 ignored —
  the review's reproduce command, previously failing at
  `sqlite::tests::migration_000058_backfills_unique_url_safe_usernames`.
- `cargo test -p gyre-server --lib`: 1573 passed, 0 failed (full suite:
  notification_dispatcher 16 incl. band membership with
  PersonaChangePendingApproval → High, api::users 21 incl. channel-prefs
  API tests, git_http 49, secret_rotation, notifications).
- `cargo test -p gyre-domain --lib`: 444 passed (incl. the two new
  flat-shape compat tests); `cargo test -p gyre-common --lib`: 116 passed.
- `cargo fmt` clean and zero clippy warnings in the touched files
  (remaining warnings are the pre-existing gyre-ports merged baseline).
- Text gates OK: migration-versions (000060 is the next unused sequence),
  migration-sql-portability (REPLACE/LIKE only), arch, dead-message-kinds.
- Evidence: `/tmp/stage/review-evidence/task-112-channel-shape-repair.txt`.

**Standing sandbox restriction for remote verification** (infrastructure,
not a code defect): `awk` is mawk-only, so the gawk-syntax gates
(notification-audience, notification-priority, notification-test-coverage,
tenant-filter, path-scope-binding, cross-surface-parity) cannot run locally.
Channel delivery is covered by the `HttpSender` test double asserting URL,
headers (HMAC signature), and body; live HTTP round-trip verification against
a local listener is deferred to host verification.

## Spec Excerpt

From `user-management.md` §Delivery Channels:

```rust
pub struct NotificationChannels {
    pub in_app: bool,           // Always true (can't disable)
    pub email: EmailConfig,
    pub webhook: Option<WebhookConfig>,
    pub slack: Option<SlackConfig>,
}

pub struct EmailConfig {
    pub enabled: bool,
    pub digest: DigestFrequency,
    pub min_priority: NotificationPriority,
}

pub struct WebhookConfig {
    pub url: String,
    pub secret: String,            // HMAC-SHA256 signing secret
    pub min_priority: NotificationPriority,
}

pub struct SlackConfig {
    pub webhook_url: String,
    pub channel: Option<String>,
    pub min_priority: NotificationPriority,
}
```

From §Who Gets Notified — routing table mapping events to recipients (spec approval → approvers, agent escalation → spawning user + workspace admins, gate failure → MR author, etc.).

From §Notification Routing for Agent Escalations:
1. Check agent's `spawned_by` user — primary recipient
2. If offline, also notify workspace Admins
3. For `Urgent` priority: always notify workspace Owners

## Implementation Plan

1. **Extend domain types in `gyre-domain`:**
   - Update `NotificationChannels` to include `email: EmailConfig`, `webhook: Option<WebhookConfig>`, `slack: Option<SlackConfig>`
   - Add `EmailConfig`, `WebhookConfig`, `SlackConfig` structs
   - Add `NotificationPriority` enum (Low, Medium, High, Urgent) if not present

2. **Notification dispatcher service:**
   - Create a `NotificationDispatcher` in `gyre-server` that receives a notification and routes it to configured channels
   - For each channel, check `min_priority` threshold before sending
   - In-app delivery: create notification record (existing path)
   - Email delivery: format notification into email template, queue for send (use lettre or similar, or HTTP-based email API)
   - Webhook delivery: POST JSON payload to configured URL, sign with HMAC-SHA256
   - Slack delivery: POST to Slack incoming webhook URL

3. **Notification routing engine:**
   - Implement the routing table from §Who Gets Notified
   - For each event type, resolve the list of recipient user IDs
   - For agent escalations: check `spawned_by`, check active sessions for online status, escalate to workspace admins/owners as needed

4. **Notification preferences API:**
   - `GET /api/v1/notifications/preferences` — current user's channel preferences
   - `PUT /api/v1/notifications/preferences` — update preferences
   - Store as part of `UserPreferences` or separate table

5. **Wire into domain event handlers:**
   - When domain events fire (gate failure, budget warning, spec approval, etc.), call the notification dispatcher
   - Map `MessageKind` events to `NotificationType` and resolve recipients

## Acceptance Criteria

- [ ] `NotificationChannels` extended with email, webhook, Slack configs
- [ ] `NotificationDispatcher` routes to all configured channels
- [ ] Webhook delivery signs payload with HMAC-SHA256
- [ ] Slack delivery posts to incoming webhook
- [ ] Email delivery queues messages (at minimum, the interface — actual SMTP can be stubbed)
- [ ] Routing table maps events to correct recipients per spec
- [ ] Agent escalation routing checks online status and escalates
- [ ] Notification preferences API (GET/PUT)
- [ ] Priority threshold filtering works per channel
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations. Existing notification model is in `gyre-domain/src/notification.rs` and `gyre-common/src/notification.rs`. User preferences are in `gyre-domain/src/user.rs` (`UserPreferences` struct, `NotificationChannels`). Check the existing notification creation paths — grep for `Notification::new` or `create_notification`. Domain events are in `gyre-server/src/domain_events.rs`. Route registration in `api/mod.rs`. ABAC mappings in `abac_middleware.rs`.
