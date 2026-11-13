---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["87a1d9522b3bcc0c09fc515abe7c09be5c8c97d2", "b0837f87a3938e60796ea1711f0cecd9917f2aa1", "d71d3158e847206541d26b6c88ce772411914539", "606ef649f571ca71badbe628ef025beecc3fdac4", "9558f4091bb2d0c01688e65a4cdb7d0c4b93f651", "ea8dd97b7841718662b84f6501435a10c04cb374", "8269e906749f6b995a429916c868d5c829cfe77a", "7601f79ab5fd116a7e22fb0a137b18fb895953bc"]
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

- **Channel types (§Delivery Channels):** `gyre-domain` `NotificationChannels` carries `in_app` (forced `true` — PUT rejects `in_app: false`), `EmailConfig { enabled, digest, min_priority }`, optional `WebhookConfig { url, secret, min_priority }` and `SlackConfig { webhook_url, channel, min_priority }`. Priority bands derive from notification TYPE via `NotificationPriority::for_notification_type` (spec amendment table in user-management.md §Notification Entity), never from a numeric HSI field. Flat→nested stored-shape compatibility (custom `Deserialize` + migration `000060_user_preferences_channel_shape`) preserves pre-task-112 `users.preferences` rows.
- **Dispatcher:** `gyre-server/src/notification_dispatcher.rs` `dispatch_to_channels`/`dispatch_with` fans every persisted in-app notification to configured channels, each gated by its `min_priority` threshold. Webhook: JSON POST signed HMAC-SHA256 (`X-Gyre-Signature: sha256=<hex>`). Slack: POST to the incoming-webhook URL with optional channel override. Email: queued as a durable `kv_store` outbox entry (interface per plan; SMTP transport is deployment's concern). All outbound HTTP is latency-bounded (10 s timeout).
- **Routing (§Who Gets Notified, §Notification Routing for Agent Escalations):** `escalation_recipients` implements the full chain — `spawned_by` primary; offline spawner (active-session check) adds workspace Admins; Urgent priority always adds workspace Owners (dedup'd). Helpers cover spec/persona approval (approver/owner) and merge-queue pause (Admins+Owners); `spawn.rs` calls `notify_agent_escalation` on Overseer escalation. The "breaking change → dependent repo team leads" row was resolved by spec amendment (interim superset routing to dependent repo's workspace members) since no team-lead concept exists.
- **Budget rows are a single production implementation (review F2 repair):** `api/budget.rs` `maybe_notify_budget_threshold` classifies the level (≥80% warning / ≥100% exhausted) and calls the dispatcher's `notify_agent_budget_warning`/`notify_agent_budget_exhausted` (`BudgetNotificationContext`), which own recipient resolution (spawner + Admins / spawner + Owner) and per-period de-duplication — the routing contract cannot drift from the live emission path. The security-finding helper remains the routing contract for the "Security finding → Workspace Owner + tenant Admin" row; its emission trigger (a Critical/High scanner finding) does not exist product-wide yet — recorded gap owned by task-137, no caller today.
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` (api/users.rs, per-handler self-scope auth, ABAC-exempt with registered exemption) backed by the `user_channel_preferences` table — migrations `000060`/`000061`, portable SQL enforced by SQLite, PostgreSQL, and mem adapters (port upsert contract guarded in code in mem). PUT validates absolute webhook/Slack URLs.
- **Fan-out wiring (review F1 repair, commit 87a1d952):** every production notification-creation path in `gyre-server` dispatches to configured channels after the in-app record persists — notifications.rs (`notify`/`notify_rich`), git_http.rs (persona-push hook AND extraction notifications), abandoned_branch.rs, trust_suggestion.rs, api/repos.rs, spec_link_staleness.rs, mirror_sync.rs, spec_patrol.rs, constraint_check.rs, mcp.rs, reconciliation.rs (MetaSpecDrift sweep), secret_rotation.rs (SecretExpiring), prompt_assembly.rs (stale meta-spec pin), signal_chain.rs (CrossWorkspaceSpecChange), impersonation.rs (request + alert). Audit evidence: `/tmp/stage/review-evidence/fanout-bypass-audit-repair.txt` — 0 BYPASS sites at 581d25ea.

### Verification

Repair round (87a1d952, review b69319b7 F1+F2):

- Wired `dispatch_to_channels` at the seven bypassed creation sites (git_http persona-push, reconciliation sweep, secret expiry, stale pin, cross-workspace change, impersonation request/alert) and consolidated the budget routing (budget.rs now calls the dispatcher helpers; the duplicated unreachable helpers are gone).
- Added emitter-path guard tests for every repaired emitter: each seeds a configured email channel and asserts the durable kv outbox gains an entry — `fans_out_to_configured_channels` tests in reconciliation/secret_rotation/prompt_assembly/signal_chain, `approval_code_and_alert_fan_out_to_configured_channels` (impersonation, new tests module), and the extended `push_persona_change_updates_entity_resets_approval_and_notifies_owner` (git_http). Added `budget_level_notifies_each_user_once_per_period` (dispatcher dedup). The pre-existing end-to-end `budget_threshold_notifications_fire_once_per_period` (api/budget.rs) exercises the consolidated path: recipients per row, priority bands, once-per-period dedup.
- This sandbox's build wrapper blocks `gyre-server` test compilation (4 GiB budget: "Server builds/tests belong to the independent verifier") — that is an infrastructure restriction, not a failure. Verified locally instead:
  - `cargo check -p gyre-server --all-targets` → exit 0 (type-checks all new tests)
  - `cargo clippy -p gyre-server --all-targets` → no warnings in any changed file
  - `cargo fmt -p gyre-server -- --check` and `python3 scripts/check-rustfmt-diff.py f764bc00` (the CI fmt gate) → exit 0
  - `scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-inert-enforcement.sh`, `check-unbounded-external-http.sh`, `check-lossy-secret-conversion.sh`, `check-fabricated-scope-defaults.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh` → all OK
- The independent verifier must execute (recorded in `/tmp/stage/review-evidence/repair-round-evidence.md`):
  - `cargo test -p gyre-server --lib notification_dispatcher` (channel suite + rewritten budget helper tests + new dedup test)
  - `cargo test -p gyre-server --lib fans_out_to_configured_channels` (six emitter-path guards)
  - `cargo test -p gyre-server --lib push_persona_change_updates_entity_resets_approval_and_notifies_owner`
  - `cargo test -p gyre-server --lib approval_code_and_alert_fan_out_to_configured_channels`
  - `cargo test -p gyre-server --lib budget` (threshold emission through the consolidated helpers)
  - Full `cargo test --all` per the acceptance criterion
