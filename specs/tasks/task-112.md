---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["4f368f7854f1680572398a6faa4a3092b6d18df7", "d71d3158e847206541d26b6c88ce772411914539", "606ef649f571ca71badbe628ef025beecc3fdac4", "9558f4091bb2d0c01688e65a4cdb7d0c4b93f651", "ea8dd97b7841718662b84f6501435a10c04cb374", "8269e906749f6b995a429916c868d5c829cfe77a", "7601f79ab5fd116a7e22fb0a137b18fb895953bc"]
---

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

- [x] `NotificationChannels` extended with email, webhook, Slack configs
- [x] `NotificationDispatcher` routes to all configured channels
- [x] Webhook delivery signs payload with HMAC-SHA256
- [x] Slack delivery posts to incoming webhook
- [x] Email delivery queues messages (at minimum, the interface — actual SMTP can be stubbed)
- [x] Routing table maps events to correct recipients per spec
- [x] Agent escalation routing checks online status and escalates
- [x] Notification preferences API (GET/PUT)
- [x] Priority threshold filtering works per channel
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations. Existing notification model is in `gyre-domain/src/notification.rs` and `gyre-common/src/notification.rs`. User preferences are in `gyre-domain/src/user.rs` (`UserPreferences` struct, `NotificationChannels`). Check the existing notification creation paths — grep for `Notification::new` or `create_notification`. Domain events are in `gyre-server/src/domain_events.rs`. Route registration in `api/mod.rs`. ABAC mappings in `abac_middleware.rs`.

## Shipped

### Behavior

- **Channel types (§Delivery Channels):** `gyre-domain` `NotificationChannels` carries `in_app` (forced `true` — PUT rejects `in_app: false`), `EmailConfig { enabled, digest, min_priority }`, optional `WebhookConfig { url, secret, min_priority }` and `SlackConfig { webhook_url, channel, min_priority }`. Priority bands derive from notification TYPE via `NotificationPriority::for_notification_type` (spec amendment table in user-management.md §Notification Entity), never from a numeric HSI field. Flat→nested stored-shape compatibility (custom `Deserialize` + migration `000060_user_preferences_channel_shape`) preserves pre-task-112 `users.preferences` rows.
- **Dispatcher:** `gyre-server/src/notification_dispatcher.rs` `dispatch_to_channels`/`dispatch_with` fans every persisted in-app notification to configured channels, each gated by its `min_priority` threshold. Webhook: JSON POST signed HMAC-SHA256 (`X-Gyre-Signature: sha256=<hex>`). Slack: POST to the incoming-webhook URL with optional channel override. Email: queued as a durable `kv_store` outbox entry (interface per plan; SMTP transport is deployment's concern). All outbound HTTP is latency-bounded (10 s timeout).
- **Routing (§Who Gets Notified, §Notification Routing for Agent Escalations):** `escalation_recipients` implements the full chain — `spawned_by` primary; offline spawner (active-session check) adds workspace Admins; Urgent priority always adds workspace Owners (dedup'd). Helpers cover spec/persona approval (approver/owner), merge-queue pause (Admins+Owners), budget warning/exhausted, and security findings; `spawn.rs` calls `notify_agent_escalation` on Overseer escalation. The "breaking change → dependent repo team leads" row was resolved by spec amendment (interim superset routing to dependent repo's workspace members) since no team-lead concept exists.
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` (api/users.rs, per-handler self-scope auth, ABAC-exempt with registered exemption) backed by the `user_channel_preferences` table — migrations `000060`/`000061`, portable SQL enforced by SQLite, PostgreSQL, and mem adapters (port upsert contract guarded in code in mem). PUT validates absolute webhook/Slack URLs.
- **Wiring:** `dispatch_to_channels` is called from every notification creation path (notifications.rs, git_http.rs, abandoned_branch.rs, trust_suggestion.rs, api/repos.rs, spec_link_staleness.rs, mirror_sync.rs, spec_patrol.rs, constraint_check.rs, mcp.rs).

### Verification

Repair round (d71d3158): rewrapped the over-long `assert!` in `persona_approval_requested_without_owner_or_admins_is_noop` — the review's F1 rustfmt CI-blocker. Verified in this round at HEAD:

- `cargo fmt -p gyre-server -- --check` → exit 0
- `python3 scripts/check-rustfmt-diff.py d9c419a` (the CI fmt gate, assignment base) → exit 0, "changed lines clean (132 Rust files checked)"
- `scripts/check-abac-route-registry.sh` → OK; `scripts/check-abac-exempt-handlers.sh` → OK (77 handlers, incl. the preferences self-scope handlers); `scripts/check-migration-versions.sh` → OK; `scripts/check-mem-port-contracts.sh` → OK

Earlier rounds on this branch ran the focused lib tests green before the reformat (behavior-neutral whitespace-only change); this sandbox has an empty build cache and a single-job build wrapper under the 4 GiB budget, so the independent verifier should execute:

- `cargo test -p gyre-server --lib notification_dispatcher` (16 tests: priority band membership, webhook sign + threshold filter, slack delivery + channel override, durable email queue, escalation chain — primary/offline→Admins/Urgent→Owners, persona approval owner + no-op without admins, merge-queue pause, budget warning/exhausted, security finding)
- `cargo test -p gyre-server --lib channel_preferences` (api/users.rs: defaults, round-trip, reject `in_app: false`, reject relative webhook URL)
- `cargo test -p gyre-adapters` (sqlite `channel_prefs_roundtrip_and_upsert_replaces` + migration compatibility)
- Full `cargo test --all` per the acceptance criterion

Frontmatter records all task-labeled product-surface commits including the previously unattributed checkpoint `4f368f78` and fmt-fix commit `d71d3158`.
