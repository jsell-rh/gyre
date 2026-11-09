---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["8269e906749f6b995a429916c868d5c829cfe77a", "7601f79ab5fd116a7e22fb0a137b18fb895953bc"]
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
§Notification Entity). Webhook POSTs the JSON payload signed with HMAC-SHA256
(`X-Gyre-Signature`, ring). Slack POSTs to the incoming webhook URL (channel
override honored). Email queues a durable outbox entry in `kv_store`
(`email_outbox` ns) for the digest sender — the interface contract per the
task plan; SMTP transport is deployment's concern. All outbound HTTP is
latency-bounded (reqwest with 10 s timeout).

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

### Merge round (fefde294, commit 6703a0353)

Rebase merge from main resolved. `gyre-common/src/notification.rs`: both
sides appended `NotificationType` variants at the same arms — resolution
keeps both sets (task-112's PersonaApprovalRequested/MergeQueuePaused/
BudgetExhausted/SecurityFinding and main's SecretExpiring).
`check-fabricated-scope-defaults.sh`: both sides lowered
`FROZEN_EXEMPTION_COUNT`; the exemption file auto-merged to main's 2-entry
version (auth.rs:773 entry removed upstream by task-103's repair), so the
script takes main's count=3 — keeping our 4 would reintroduce the off-by-one
slack the gate exists to close. Merge seam: `for_notification_type` is
exhaustive, so SecretExpiring is banded High (same approaching-deadline class
as BudgetWarning, numeric 7); the spec's type→band amendment table records
the banding with a footnote and the dispatcher's membership test asserts it.
Prior merge rounds (ed57c723, 9bef21a4, 72ad2a128, c5deca80) resolved with
conflicts confined to the seams noted in their merge commits; generated
`web/dist` assets are restored from the assignment base by the executor and
rebuilt remotely.

### Review repair history

Findings 07fd53c9 (commit 8269e9067): (1) the old numeric
`NotificationPriority::from_band` threshold mapping made the §Agent
Escalations "Urgent → Owners" step unreachable (AgentEscalation is numeric
5); replaced by the per-type `for_notification_type` mapping with the full
type→band table amended into the spec, plus the full-path regression
`agent_escalation_notifies_owners_even_when_spawner_online`. (2) A dead
channel-prefs defaults test (missing `#[tokio::test]`) now executes.

### Test evidence (merge-round HEAD 6703a0353, this sandbox)

- `SKIP_WEB_BUILD=1 cargo build -p gyre-common -p gyre-domain -p gyre-server`:
  clean.
- `cargo test -p gyre-server --lib notification_dispatcher`: 16 passed
  (HMAC signing + threshold filtering, slack posting + channel override,
  email outbox, escalation recipient chain incl. offline-spawner and Urgent
  fan-out, full-path agent-escalation → Owners regression, band membership
  incl. SecretExpiring → High).
- `cargo test -p gyre-server --lib secret_rotation`: 3 passed (main's new
  SecretExpiring emitter flowing through the dispatcher).
- `cargo test -p gyre-server --lib notifications`: 13 passed;
  `api::users`: 18 passed (incl. four channel-prefs API tests);
  `api::spawn`: 38 passed.
- `cargo test -p gyre-common --lib notification`: 4 passed;
  `cargo test -p gyre-domain --lib user`: 11 passed.
- Text gates OK this round: fabricated-scope-defaults, migration-versions
  (full-prefix Diesel versions unique across the merged set), arch,
  dead-message-kinds, abac-route-registry, unbounded-external-http,
  inert-enforcement. Other gates are unaffected (no routes, migrations,
  ports, or stores touched by the seam edits).
- Web (`web/src`) untouched by this round; prior vitest evidence stands.
- Evidence: `/tmp/stage/review-evidence/task-112-merge-round.txt`.

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
