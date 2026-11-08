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

**Domain types** (`gyre-domain/src/user.rs`): `NotificationChannels` extended
with `email: EmailConfig`, `webhook: Option<WebhookConfig>`,
`slack: Option<SlackConfig>`; `NotificationPriority` (Low..Urgent, ordered),
`DigestFrequency`. `Default` keeps `in_app` true.

**Dispatcher** (`gyre-server/src/notification_dispatcher.rs`): fans each
persisted in-app notification out to the recipient's configured channels with
per-channel `min_priority` threshold filtering. The band is derived from the
notification TYPE via `NotificationPriority::for_notification_type` — never
from the HSI §8 numeric priority (see the repair round note below). Webhook
POSTs the JSON payload signed with HMAC-SHA256 (`X-Gyre-Signature`, ring).
Slack POSTs to the incoming webhook URL (channel override honored). Email
queues a durable outbox entry in `kv_store` (`email_outbox` ns) for the digest
sender — the interface contract per the task plan (SMTP transport is
deployment's concern). All outbound HTTP is latency-bounded
(`CHANNEL_HTTP_TIMEOUT_SECS` = 10 s, reqwest with timeout —
`check-unbounded-external-http.sh` invariant).

**Routing engine** (§Who Gets Notified): `escalation_recipients` implements the
agent-escalation chain — `spawned_by` primary; offline spawner adds workspace
Admins; Urgent always adds workspace Owners (dedup'd). Helpers cover persona
approval → owner, merge queue paused → all Admins/Owners, budget warning →
spawner + Admins, budget exhausted → spawner + Owner, security finding →
Workspace Owner + tenant Admin. Decomposition boundary: the budget warning,
budget exhausted, and security finding helpers have no production emission
triggers yet — those are owned by task-191 (platform-model §Enforcement
Behavior) and task-137 (agent-gates §Gate Failure Feedback); the helpers and
their recipient sets are covered by real tests but must not be treated as
live until the triggers land. Spec approval, gate failure, MR merged/reverted,
and invitation rows are covered by existing creation paths. The "Breaking
change detected → dependent repo team leads" row cannot be routed as specced
— the domain has no team-lead concept; user-management.md is amended (56059f8d)
to record the interim superset routing (all members of the dependent repo's
workspace, pre-existing `process_breaking_changes` path) and the dependency on
Teams gaining a lead concept (task-122/125).

**Wiring**: `notifications::notify`/`notify_rich` (the central creation
helpers) fan out through `dispatch_to_channels`, so every emitting path
(gate failure, spec patrol, merge queue, abandoned branch, trust suggestion,
spec-link staleness, git_http, mcp, divergence/mirror-sync spec assertions)
delivers to configured channels. `spawn.rs` calls `notify_agent_escalation` on
Overseer escalation.

**Preferences API**: `GET/PUT /api/v1/notifications/preferences` (self-scope,
per-handler auth; ABAC-exempt with documented self-scope invariant). PUT
rejects `in_app: false` (spec: "can't disable") and non-absolute webhook/Slack
URLs. Storage: `user_channel_preferences` table (migration
`2026-10-08-000059_user_channel_preferences` — renumbered across successive
main merges as upstream collisions landed under the same sequence; table
references are by schema name, so the renames are behavior-neutral; portable
SQLite/PG SQL), `UserChannelPreferenceRepository` port with SQLite, Postgres,
and mem adapters (all enforce the upsert contract).

**Test evidence** (repair-round HEAD 8269e9067; all ran locally in this
sandbox — it has crates.io access):
- `SKIP_WEB_BUILD=1 cargo build -p gyre-domain -p gyre-server`: clean.
- `cargo test -p gyre-server --lib notification_dispatcher`: 16 passed
  (HMAC signing + threshold filtering, slack posting + channel override,
  email outbox durability + filtering, escalation recipient chain incl.
  offline-spawner and Urgent fan-out, the new full-path regression
  agent-escalation → Owners with online spawner, in-app records per
  recipient, persona approval, merge-queue-paused, budget warning/exhausted,
  security finding, per-type band membership).
- `cargo test -p gyre-server --lib api::users`: 18 passed — the four
  channel-prefs API tests (defaults, round-trip, rejected `in_app: false`,
  rejected relative webhook URL; the defaults test now actually executes —
  see repair round) plus the base's profile/PUT-me/notification tests.
- `cargo test -p gyre-server --lib api::spawn`: 36 passed (fail_agent path
  covered by the module suite).
- `cargo test -p gyre-server --test graph_integration
  test_divergence_detection_creates_notifications`: 1 passed.
- `cargo test -p gyre-adapters --lib user_profile`: 5 passed.
- `cargo test -p gyre-domain --lib user`: 11 passed.
- `cargo test -p gyre-server --lib notifications`: 12 passed.
- Clippy on touched crates: no findings in the changed files.
- Text gates re-run OK: arch, dead-message-kinds, abac-route-registry,
  inert-enforcement, unbounded-external-http, byte-slice-truncation.
  Earlier-round gates (abac-exempt-handlers, mem-port-contracts,
  relative-path-defaults, mcp-write-tools, lossy-secret-conversion,
  scope-literal-defaults, forwarded-header-trust, in-memory-state-stores,
  forged-scope-fields, fail-open-ref-resolution, migration-versions,
  migration-sql-portability, fabricated-scope-defaults, api-auth,
  message-delivery, unwired-port-methods, sandbox-sweep-artifacts,
  task-commit-attribution) are unaffected by this round's diff (no routes,
  migrations, ports, or stores touched).
- Web (`web/src`) is untouched by the repair round; the prior round's
  vitest evidence (65 passed on Inbox, WorkspaceHome,
  WorkspaceHomeArchitecture, last-seen) stands.
- Evidence captured under `/tmp/stage/review-evidence/`
  (`task-112-repair-round.txt`, probe script `run.sh`).

### Review repair round (findings 07fd53c9, commit 8269e9067)

Two review findings repaired:

1. **Band mapping contradicted the spec (code).** The old
   `NotificationPriority::from_band` numeric threshold mapping (1–3 Urgent,
   4–6 High, 7–8 Medium, 9–10 Low) classified AgentEscalation (HSI numeric 5)
   as High — so §Notification Routing for Agent Escalations step 3 ("For
   Urgent priority: always notify workspace Owners") was unreachable in the
   only production escalation path (`fail_agent` → `notify_agent_escalation`),
   with the unit test hand-feeding Urgent masking the dead branch. It also
   sent GateFailure (3) as Urgent and BudgetWarning (7) as Medium where
   user-management.md §Notification Entity says High. No numeric threshold
   can reproduce the specced membership (SecurityFinding and GateFailure
   share numeric 3 but sit in different bands), so `from_band` is replaced by
   `NotificationPriority::for_notification_type`, a per-type mapping anchored
   on the spec's band comments; user-management.md is amended with the full
   type→band table (same amendment pattern as the team-lead routing note).
   New regression test `agent_escalation_notifies_owners_even_when_spawner_online`
   drives the full `notify_agent_escalation` path (online spawner + Owner
   member) and asserts the Owner receives the in-app notification and Admins
   are not fanned out — the exact reproduce case from the finding.
2. **Dead test (test).** `get_channel_preferences_returns_defaults` had no
   `#[tokio::test]` attribute and never executed (dead_code warning, absent
   from the run). It now runs and covers the GET defaults path (no stored row
   → 200 with default channels); `api::users` went from 17 to 18 executing
   tests with no dead-code warning.

**Merge history**: successive rebase merges from main (ed57c723, 9bef21a4,
72ad2a128, c5deca80) each resolved with conflicts confined to the seams noted
in their merge commits; generated `web/dist` assets are restored from the
assignment base by the executor and rebuilt remotely.

**Standing sandbox restriction for remote verification** (infrastructure,
not a code defect): `awk` is mawk-only, so the gawk-syntax gates
(notification-audience, notification-priority, notification-test-coverage,
tenant-filter, path-scope-binding, cross-surface-parity) cannot run locally.
Channel delivery is covered by the captured `HttpSender` test double asserting
URL, headers (HMAC signature), and body; live HTTP round-trip verification
against a local listener is deferred to host verification.

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
