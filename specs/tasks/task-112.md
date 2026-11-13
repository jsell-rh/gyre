---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["64b30291b90671f4a626ed9a22e4da1824a98572", "e5903baae5756bc418c5af4ac7ab6c122a90972e", "d2068c4a79d72b6167272ea555a4133c6cd6527b", "7999a73b5c948cdae4e8ef2e901c04e4a55b60e1", "b44534026bc96c24978a09a5978bc9868aa70e1a", "87a1d9522b3bcc0c09fc515abe7c09be5c8c97d2", "b0837f87a3938e60796ea1711f0cecd9917f2aa1", "d71d3158e847206541d26b6c88ce772411914539", "606ef649f571ca71badbe628ef025beecc3fdac4", "9558f4091bb2d0c01688e65a4cdb7d0c4b93f651", "ea8dd97b7841718662b84f6501435a10c04cb374", "8269e906749f6b995a429916c868d5c829cfe77a", "7601f79ab5fd116a7e22fb0a137b18fb895953bc"]
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

## Shipped

### Behavior

- **Channel types (§Delivery Channels):** `gyre-domain` `NotificationChannels` carries `in_app` (forced `true` — every write endpoint rejects `in_app: false`), `EmailConfig { enabled, digest, min_priority }`, optional `WebhookConfig { url, secret, min_priority }` and `SlackConfig { webhook_url, channel, min_priority }`. Priority bands derive from notification TYPE via `NotificationPriority::for_notification_type` (spec amendment table in user-management.md §Notification Entity). Flat→nested stored-shape compatibility (custom `Deserialize` + migration `000060_user_preferences_channel_shape`) preserves pre-task-112 `users.preferences` rows.
- **Dispatcher:** `notification_dispatcher.rs` `dispatch_to_channels`/`dispatch_with` fans every persisted in-app notification to configured channels, each gated by its `min_priority` threshold. Webhook: JSON POST signed HMAC-SHA256 (`X-Gyre-Signature: sha256=<hex>`). Slack: POST to the incoming-webhook URL with optional channel override. Email: queued as a durable `kv_store` outbox entry (interface per plan; SMTP transport is deployment's concern). All outbound HTTP is latency-bounded (10 s timeout). Every production notification-creation path in `gyre-server` dispatches after the in-app record persists — including (review 1cae4a25 F1 repair) `workspace_ownership::notify_tenant_admins`, previously the last unwired path; the push-scoped paths (git_http, mirror_sync, api/repos extraction) compose the task-125 SSE `broadcasting_notifications()` wrapper with the channel fan-out.
- **Single effective channel config:** `user_channel_prefs` is the delivery-read store and every write endpoint converges it — `PUT /users/me` carrying `preferences.notification_channels` validates it with the same rules as the dedicated endpoint (shared `validate_channel_config`) and upserts `user_channel_prefs` with the same value; a channel update mirrors its value into `users.preferences.notification_channels` so `GET /users/me` echoes the config delivery actually uses. `resolve_channels` falls back to the user's stored `users.preferences` channels when no `user_channel_prefs` row exists (migration `000061`). No API-visible config is silently ignored by delivery.
- **Routing (§Who Gets Notified, §Notification Routing for Agent Escalations):** `escalation_recipients` implements the full chain — `spawned_by` primary; offline spawner (active-session check) adds workspace Admins; Urgent priority always adds workspace Owners (dedup'd). Helpers cover spec/persona approval (approver/owner), merge-queue pause (Admins+Owners) and security finding (Owner + tenant Admin, emission trigger owned by task-137); `spawn.rs` calls `notify_agent_escalation` on Overseer escalation. Budget rows are a single production implementation converged with the task-125 base's level-crossing architecture: `api/budget.rs` `notify_budget_level` builds the `BudgetNotificationContext` and calls the dispatcher's `notify_agent_budget_warning`/`notify_agent_budget_exhausted`, which own recipient resolution and per-period de-duplication.
- **Spec-owner routing (review 1cae4a25 F3 repair):** the "Spec drift detected → Spec owner" row is implemented. New shared resolver `notifications::resolve_owner_ref` resolves ledger owner refs — `user:{name}` by user id, username, or display name; `team:{slug}` via team members. `spec_patrol` error findings add the drifted spec's owner to the recipients (deduplicated over the Admin/Developer accountability superset), and the reconciliation MetaSpecDrift sweep adds the effective meta-spec set's pinned specs' owners (deduplicated over the Admin/Developer/Owner superset).
- **Team-lead rows (review 1cae4a25 F2 repair):** the spec note now records the accurate gap — `Team.lead` ships (migration `000062`); what is missing is a repo→team ownership mapping, so "the repo's team lead" / "dependent repo team leads" cannot be resolved. Interim superset routing stands (escalation chain for agent escalations; dependent repo's workspace members for breaking changes) pending that mapping.
- **No fabricated scope (review 1cae4a25 F4 repair):** `complete_agent`'s AgentCompleted and `trip_circuit_breaker`'s AgentEscalation notifications resolve the tenant from the workspace record (skip + warn when unresolvable), removing the last literal `"default"` tenant ids on notification paths in these sections.
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` (api/users.rs, per-handler self-scope auth, ABAC-exempt with registered exemption) — one route serving both preference surfaces with combined handlers: GET returns `preferences` (per-type, HSI §12) and `channels` (delivery channels); PUT accepts either or both. Backed by the `user_notification_prefs` and `user_channel_preferences` stores — migrations `000060`/`000061`, portable SQL enforced by SQLite, PostgreSQL, and mem adapters.

### Verification

- Guard tests added across the rounds, each killing a real defect class: email/webhook/slack delivery + HMAC signature + per-channel priority thresholds; emitter-path fan-out guards on every creation path (this round: `ownerless_notification_fans_out_to_configured_channels`); escalation/online/owner routing; budget once-per-period dedup; single-effective-config guards; band membership over the base's new variants; spec-owner routing (this round: `spec_drift_notification_routes_to_spec_owner`, `spec_drift_notification_routes_to_team_owner_ref`, `sweep_drift_notification_routes_to_meta_spec_owners` — a Viewer who owns the drifted spec is notified, proving owner routing is real and orthogonal to role routing; an Admin-who-is-owner gets exactly one notification per finding).
- This sandbox's build wrapper blocks `gyre-server` test compilation (4 GiB budget: "Server builds/tests belong to the independent verifier") — an infrastructure restriction, not a failure. Verified locally this round (evidence: `/tmp/stage/review-evidence/task-112-review-repair.md`):
  - `cargo check -p gyre-server --all-targets` → exit 0
  - `cargo clippy -p gyre-server` → no warnings in any edited region (remaining warnings pre-exist in the base)
  - `cargo fmt -p gyre-server -- --check` → exit 0; `scripts/check-arch.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-dead-message-kinds.sh`, `check-task-commit-attribution.sh` → OK
- The independent verifier must execute:
  - `cargo test -p gyre-server --lib workspace_ownership` (ownerless routing + new fan-out guard)
  - `cargo test -p gyre-server --lib spec_patrol` (error-finding routing + spec-owner/team-owner-ref routing)
  - `cargo test -p gyre-server --lib sweep_` (drift recipients incl. meta-spec owners)
  - `cargo test -p gyre-server --lib circuit_breaker_trips_at_three_reverts` and `cargo test -p gyre-server --lib complete_agent` (tenant resolution on repaired paths)
  - The prior rounds' suites: `notification_dispatcher`, `put_me_notification_channels`, `put_channel_preferences`, `fans_out_to_configured_channels`, `approval_code_and_alert_fan_out_to_configured_channels`, `budget`
  - Full `cargo test --all` per the acceptance criterion
