---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["5c37f969f35a81916ec3f276b2693fae9af22e69", "ee4e434251595b7b664b73d99be3ea3c80c25044", "896c9055969f118b82c0abb688da60453a9028a5", "94cf2f62088dba6d0b7136d792167e0ca7e6e3dd", "a3a3ea0f7f3a0cd48aff40ca57f1c33fbbdea6da"]
---

## Shipped

Real production implementation of user-management.md §Delivery Channels,
§Who Gets Notified, §Notification Routing for Agent Escalations.

**Domain types** (`gyre-domain/src/user.rs`): `NotificationChannels` extended
with `email: EmailConfig`, `webhook: Option<WebhookConfig>`,
`slack: Option<SlackConfig>`; `NotificationPriority` (Low..Urgent, ordered),
`DigestFrequency` (Off/Hourly/Daily/Weekly). `Default` keeps `in_app` true.

**Dispatcher** (`gyre-server/src/notification_dispatcher.rs`): fans each
persisted in-app notification out to the recipient's configured channels with
per-channel `min_priority` threshold filtering. Webhook POSTs the JSON payload
signed with HMAC-SHA256 (`X-Gyre-Signature`, ring). Slack POSTs to the incoming
webhook URL (channel override honored). Email queues a durable outbox entry in
`kv_store` (`email_outbox` ns) for the digest sender — the interface contract
per the task plan (SMTP transport is deployment's concern). All outbound HTTP
is latency-bounded (`CHANNEL_HTTP_TIMEOUT_SECS` = 10 s, reqwest with timeout —
`check-unbounded-external-http.sh` invariant).

**Routing engine** (§Who Gets Notified): `escalation_recipients` implements the
agent-escalation chain — `spawned_by` primary; offline spawner adds workspace
Admins; Urgent always adds workspace Owners (dedup'd). Helpers cover persona
approval → owner, merge queue paused → all Admins/Owners, budget warning →
spawner + Admins, budget exhausted → spawner + Owner, security finding →
Workspace Owner + tenant Admin. Existing creation paths already cover the
remaining rows (spec approval → manifest approvers, gate failure → MR author,
MR merged/reverted, invitation → invited user).

**Wiring**: `notifications::notify`/`notify_rich` (the central creation
helpers) fan out through `dispatch_to_channels`, so every emitting path
(gate failure, spec patrol, merge queue, abandoned branch, trust suggestion,
spec-link staleness, git_http, mcp) delivers to configured channels.
`spawn.rs` calls `notify_agent_escalation` on Overseer escalation.

**Preferences API**: `GET/PUT /api/v1/notifications/preferences` (self-scope,
per-handler auth; ABAC-exempt with documented self-scope invariant). PUT
rejects `in_app: false` (spec: "can't disable") and non-absolute webhook/Slack
URLs. Storage: new `user_channel_preferences` table (migration 000056, portable
SQLite/PG SQL), `UserChannelPreferenceRepository` port with SQLite and mem
adapters (both enforce the upsert contract).

**Test evidence** (all on merged HEAD 5c91da97):
- `cargo test -p gyre-server --lib notification_dispatcher`: 15 passed
  (HMAC signing + threshold filtering, slack posting + channel override,
  email outbox durability + filtering, escalation recipient chain incl.
  offline-spawner and Urgent fan-out, in-app records per recipient, persona
  approval, merge-queue-paused, budget warning/exhausted, security finding)
- `cargo test -p gyre-server --lib users`: 17 passed (channel prefs API)
- `cargo test -p gyre-server --lib spawn::`: 29 passed
- `cargo test -p gyre-server --lib notifications`: 10 passed
- `cargo test -p gyre-server --lib mcp`: 71 passed
- `cargo test -p gyre-server --lib merge_processor`: 49 passed
- `cargo test -p gyre-adapters --lib user_profile`: 5 passed
- `cargo test -p gyre-domain --lib user::`: 5 passed
- Prior checkpoint gates (unchanged by merge, which touched only spec .md
  files): rustfmt + changed-lines clippy clean, `SKIP_WEB_BUILD=1 cargo
  build -p gyre-server --lib` clean.
- `scripts/check-abac-exempt-handlers.sh`: OK (91 handlers).

Sandbox restriction recorded: TCP listener probe unsupported (`accept` →
errno 95), so live HTTP round-trip verification against a local listener is
deferred to host verification; channel delivery is covered by the captured
`HttpSender` test double asserting URL, headers (HMAC signature), and body.

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
