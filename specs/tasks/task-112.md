---
title: "Implement notification delivery channels & routing"
spec_ref: "user-management.md §Delivery Channels"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "user-management.md §Delivery Channels"
  - "user-management.md §Who Gets Notified"
  - "user-management.md §Notification Routing for Agent Escalations"
commits: ["7601f79ab5fd116a7e22fb0a137b18fb895953bc"]
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
Workspace Owner + tenant Admin. Honest decomposition boundary: the budget
warning, budget exhausted, and security finding helpers have no production
emission triggers yet — those are owned by task-191 (platform-model §Enforcement
Behavior) and task-137 (agent-gates §Gate Failure Feedback); the helpers and
their recipient sets are covered by real tests but reviewers must not treat
those three rows as live until the triggers land. Spec approval, gate failure,
MR merged/reverted, and invitation rows are covered by existing creation
paths. The "Breaking change detected → dependent repo team leads" row cannot
be routed as specced — the domain has no team-lead concept; user-management.md
is amended (56059f8d) to record the interim superset routing (all members of
the dependent repo's workspace, pre-existing `process_breaking_changes` path)
and the dependency on Teams gaining a lead concept (task-122/125).

**Wiring**: `notifications::notify`/`notify_rich` (the central creation
helpers) fan out through `dispatch_to_channels`, so every emitting path
(gate failure, spec patrol, merge queue, abandoned branch, trust suggestion,
spec-link staleness, git_http, mcp) delivers to configured channels.
`spawn.rs` calls `notify_agent_escalation` on Overseer escalation.

**Preferences API**: `GET/PUT /api/v1/notifications/preferences` (self-scope,
per-handler auth; ABAC-exempt with documented self-scope invariant). PUT
rejects `in_app: false` (spec: "can't disable") and non-absolute webhook/Slack
URLs. Storage: new `user_channel_preferences` table (migration 000057 —
renumbered from 000056 after the 9bef21a4 rebase merge brought
spec_assertion_results in under the same version, then to 000058 after the
72ad2a128 merge brought spec_approval_attestation in under 000057; portable
SQLite/PG SQL), `UserChannelPreferenceRepository` port with SQLite and mem
adapters (both enforce the upsert contract).

**Test evidence** (re-verified on rebase-repair HEAD d8e12aa4, after merging
main 2427ccc1 — conflicts were confined to generated `web/dist` artifacts,
resolved by regenerating dist from merged `web/src` via `vite build`):
- `cargo test -p gyre-server --lib notification_dispatcher`: 15 passed
  (HMAC signing + threshold filtering, slack posting + channel override,
  email outbox durability + filtering, escalation recipient chain incl.
  offline-spawner and Urgent fan-out, in-app records per recipient, persona
  approval, merge-queue-paused, budget warning/exhausted, security finding)
- `cargo test -p gyre-server --lib api::users`: 13 passed (includes all four
  channel-prefs API tests: defaults, round-trip, rejected `in_app: false`,
  rejected relative webhook URL)
- `cargo test -p gyre-server --lib spawn::`: 29 passed
- `cargo test -p gyre-server --lib mcp::`: 78 passed
- `cargo test -p gyre-server --lib merge_processor::`: 45 passed
- `cargo test -p gyre-adapters --lib user_profile`: 5 passed
- `cargo test -p gyre-domain --lib user::`: 5 passed
- `SKIP_WEB_BUILD=1 cargo build -p gyre-server --lib` clean on merged HEAD
  (full log: /tmp/stage/review-evidence).
- `scripts/check-task-commit-attribution.sh`: OK after the rebase merge.

Sandbox restriction recorded: TCP listener probe unsupported (`accept` →
errno 95), so live HTTP round-trip verification against a local listener is
deferred to host verification; channel delivery is covered by the captured
`HttpSender` test double asserting URL, headers (HMAC signature), and body.

### Repair round (rebase merge 72ad2a128) — compile-verified with real cargo

This round's sandbox has crates.io access, so the merged Rust tree was
actually compiled and tested locally for the first time since the task
landed (prior rounds were inspection-only due to the transport
restriction).

- **Merge resolved** (36985b7c3): base 72ad2a128 (task-096 rollback circuit
  breaker, task-053 meta-spec hash, call-graph/LSP work, pipeline tooling)
  merged into the branch. Three textual conflicts:
  - `api/users.rs` invite_member notification: kept `state.as_ref()` (the
    merged `notify_rich` takes `&AppState`; our handler holds
    `State<Arc<AppState>>`); merged in upstream's richer comment.
  - `merge_processor.rs` apply_revert_side_effects: kept our specialized
    `notify_mr_reverted(state, author_agent_id, workspace, mr_id, reason)`
    call — upstream's positional form referenced a nonexistent `user_id`
    binding in that function and passed a fabricated `"default"` tenant
    literal (would violate the fabricated-scope-defaults invariant our
    helper exists to uphold).
  - `check-fabricated-scope-defaults.sh`: dropped a duplicated
    `FROZEN_EXEMPTION_COUNT=4` line (exemption file has 3 entries ≤ frozen
    4; gate re-run OK).
  - `web/dist` rename/delete conflicts resolved in the index; generated
    assets are restored from the assignment base by the executor and
    rebuilt remotely.
- **Migration version collision fixed again**: upstream brought
  `2026-10-08-000057_spec_approval_attestation` under the same version as
  our `user_channel_preferences`. Renumbered ours to
  `2026-10-08-000058_user_channel_preferences` per
  `check-migration-versions.sh` (gate OK; table references are by schema
  name, not migration dir, so the rename is behavior-neutral).
- **Local verification evidence on merge HEAD 36985b7c3** (this sandbox,
  crates.io reachable):
  - `SKIP_WEB_BUILD=1 cargo build -p gyre-server --lib`: clean.
  - `SKIP_WEB_BUILD=1 cargo test --workspace --no-run`: all test targets
    compile (lib + every integration suite).
  - `cargo test -p gyre-server --lib notification_dispatcher`: 15 passed.
  - `cargo test -p gyre-server --lib api::users`: 13 passed (all four
    channel-prefs API tests included).
  - `cargo test -p gyre-server --lib merge_processor::`: 63 passed.
  - `cargo test -p gyre-server --test graph_integration
    test_divergence_detection_creates_notifications`: 1 passed — the
    previously transport-blocked probe from the 296d542 round now runs
    green locally.
  - Text gates re-run OK: arch, abac-route-registry, abac-exempt-handlers,
    dead-message-kinds, unbounded-external-http, mem-port-contracts,
    byte-slice-truncation, relative-path-defaults, inert-enforcement,
    mcp-write-tools, lossy-secret-conversion, scope-literal-defaults,
    forwarded-header-trust, in-memory-state-stores, forged-scope-fields,
    fail-open-ref-resolution, migration-versions,
    migration-sql-portability, fabricated-scope-defaults, api-auth,
    message-delivery, unwired-port-methods, sandbox-sweep-artifacts,
    no-em-dash, task-commit-attribution.
- **Remaining sandbox restriction** (infrastructure, not a code defect):
  `awk` is mawk-only, so the gawk-syntax gates (notification-audience,
  notification-priority, notification-test-coverage, tenant-filter,
  path-scope-binding, cross-surface-parity) still cannot run locally —
  they parse-error under mawk before evaluating any rule.

### Repair round (rebase merge 9bef21a4)

- **Rebase merge resolved** (6acce3fe): base 9bef21a4 (task-197 tenant
  bootstrap + spec-assertion results) merged into the branch. Textual
  conflicts resolved: `git_http.rs` spec-lifecycle scope lookup (both sides
  implemented the same no-fabricated-"default" invariant — kept HEAD's
  version, which preserves the cross-workspace dependent notify on lookup
  failure), `graph_extraction.rs` type region (kept BOTH sides: our
  `ChannelFanoutCallback` alias + their `PushNotificationScope` struct),
  `Inbox.svelte`/`WorkspaceHome.svelte` type-normalization maps (union of
  both sides' entries), and generated `web/dist` (regenerated from merged
  `web/src` via `vite build`).
- **Semantic merge fix 1 — `DivergencePorts.on_created` wiring**: the base's
  three new `DivergencePorts` construction sites (`mirror_sync.rs` run_once,
  `api/repos.rs` create_mirror_repo spawn, `api/repos.rs` sync_mirror) were
  written against the 2-field struct and would not compile against our
  3-field struct. All three now pass the real channel fan-out closure
  (same pattern as `git_http.rs`), so mirror-clone/mirror-sync spec-assertion
  notifications also deliver to configured channels per §Delivery Channels;
  the in-file port-level test passes `on_created: None` per its documented
  contract.
- **Semantic merge fix 2 — migration version collision** (34867c8b): both
  sides had taken sequence 000056 (`user_channel_preferences` here,
  `spec_assertion_results` upstream). Diesel silently drops all but the
  alphabetically-first per version, so one table would never be created.
  This branch's migration renumbered to 000057 per
  `check-migration-versions.sh` (both gates re-run OK).
- **Local verification evidence** (this sandbox, HEAD 34867c8b):
  - `npm ci` + `npx vite build`: clean, 296 modules transformed; committed
    `web/dist` matches the fresh build.
  - `npx vitest run src/__tests__/Inbox.test.js WorkspaceHome.test.js
    WorkspaceHomeArchitecture.test.js`: 55 passed — covers both resolved
    Svelte conflict sites.
  - Text gates re-run OK: arch, abac-route-registry, abac-exempt-handlers,
    api-auth, fabricated-scope-defaults, migration-versions,
    migration-sql-portability, dead-message-kinds, unbounded-external-http,
    inert-enforcement, mem-port-contracts, unwired-port-methods,
    mcp-write-tools, message-delivery, no-em-dash, task-commit-attribution,
    sandbox-sweep-artifacts, and the security-class lints.
- **Sandbox restrictions recorded for remote verification** (infrastructure,
  not code defects):
  - No crates.io access (`cargo build`/`cargo test` cannot fetch deps;
    registry cache empty, `--offline` cannot resolve) — same restriction as
    the previous repair round. The merged Rust code must be compile-verified
    remotely; all hand-merged seams (DivergencePorts constructions,
    extract_and_store_graph 9-arg call sites, detect_lineage_deps 6-arg call
    sites, NotificationType variants vs. Svelte/locale maps) were checked
    by direct inspection against both parents.
  - `awk` is mawk-only (no gawk), so the gawk-syntax gates
    (notification-audience, notification-priority, notification-test-
    coverage, tenant-filter, path-scope-binding, cross-surface-parity)
    cannot run locally.
  - Pre-existing failures NOT introduced by this merge (fail identically on
    both parent commits): check-type-simplification (gyre-common
    graph/view_query String fields), check-wildcard-exclusion
    (orchestrator.rs:67 — `MetaSpecApprovalStatus` substring false positive
    on an `Option` match), check-dead-components (4 components),
    check-mcp-wrapper-parity (35 repo-wide findings). Left for upstream
    drift repair; not touched to keep this merge scoped.

### Prior repair round (review finding 296d542)

- **Blocking regression fixed** (64279a35): the channel fan-out callback
  addition to `check_divergence` had dropped
  `notif.entity_ref = Some(spec_ref.to_string())` and
  `notif.repo_id = Some(repo_id.as_str().to_string())`, breaking
  `graph_integration::test_divergence_detection_creates_notifications`
  at tests/graph_integration.rs:1167. Both lines restored byte-identical
  to the pre-regression code (17c81d5a).
- **Merge resolved**: base ed57c723 (task-201 search backend) merged into
  the branch; the single conflict (`check-fabricated-scope-defaults.sh`
  FROZEN_EXEMPTION_COUNT) resolved against the merged tree's actual flagged
  sites — 4 entries / count 4, gate re-run OK.
- **Spec finding addressed** (56059f8d): team-lead routing rows amended in
  user-management.md with the interim superset routing and the Teams-lead
dependency.
- **Local re-verification restriction**: this repair sandbox has no
crates.io access (`cargo test` fails to fetch `prost-derive`; registry
cache empty, `--offline` cannot resolve) — an infrastructure transport
  restriction, not a code defect. The regression fix is a two-line
  restoration byte-identical to code that compiled and passed the failing
  test at 17c81d5a, but the focused probe
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test graph_integration
  test_divergence_detection_creates_notifications` must be re-run by remote
  verification on this HEAD (56059f8d). Evidence:
  /tmp/stage/review-evidence.

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
