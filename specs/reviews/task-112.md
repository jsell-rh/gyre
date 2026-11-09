# Review — task-112 (Notification Delivery Channels & Routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate: `6d70869801753f61e5369ee593be89cdb38c41f9` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`).
Round 3, after verification findings 63609fed (rustfmt/clippy, repaired in `ea8dd97b7`)
and 07fd53c9 (numeric priority-band mapping made "Urgent → Owners" unreachable; repaired
in `8269e9067` with the per-type `for_notification_type` mapping + full-path regression).
Verdict: **approved** (full evidence: `/tmp/stage/review-evidence/task-112-round3.txt`).

## Probes (independent, this sandbox, at the candidate SHA)

- `cargo test -p gyre-server --lib notification_dispatcher` → 16/16; `api::users` → 18/18
  (incl. 4 channel-prefs API tests); `notifications` → 13/13; `secret_rotation` → 3/3;
  `gyre-common notification` → 4/4; `gyre-domain user` → 11/11. `cargo fmt --check` clean;
  `cargo clippy -p gyre-server --lib --tests` exit 0 with no warnings in task files
  (remaining `must_use` warnings are the pre-existing `UserNotificationPreferenceRepository`
  trait, verbatim in the base).
- **Live HTTP round-trip of the production `ReqwestSender`** (temporary integration test,
  deleted after the run; tree restored clean): real TcpListener capture shows the exact
  body bytes transmitted, `X-Gyre-Signature` transmitted verbatim, and an independent
  receiver-side HMAC-SHA256 recomputation (ring, not the production `sign_payload`)
  verifies over the wire bytes; non-2xx surfaces an error. This closes the mirrored-logic
  gap in the unit test (which re-uses `sign_payload` to compute the expected value).
- 17 mechanical gates exit 0 (migration-versions, arch, abac-route-registry,
  unbounded-external-http, fabricated-scope-defaults, dead-message-kinds,
  inert-enforcement, mem-port-contracts, lossy-secret-conversion, scope-literal-defaults,
  byte-slice-truncation, relative-path-defaults, in-memory-state-stores,
  abac-exempt-handlers, fail-open-ref-resolution, forwarded-header-trust,
  migration-sql-portability, task-commit-attribution).
- Frontend: `npm ci` + `npm test` → 1936 passed / 0 failed (89 files). Web diff is
  i18n/label additions for the new NotificationType variants.

## Verified against the task contract

- **Domain types** match the spec structs field-for-field; `NotificationPriority` is
  ordered (Low<Medium<High<Urgent) and `for_notification_type` is an exhaustive match —
  new variants cannot silently fall into a band. The band table is amended into
  user-management.md §Notification Entity (rule-4 spec amendment, including the
  SecretExpiring → High merge seam).
- **Dispatcher**: per-channel inclusive `>=` min_priority filtering; email queues a
  durable kv outbox entry (key `user:notif`, re-dispatch overwrites — no duplicates);
  webhook HMAC-SHA256-signed; slack honors channel override; all outbound HTTP
  latency-bounded (10 s). Best-effort per channel, in-app already persisted.
- **Routing table**: every row implemented or riding an existing creation path —
  spec approval → manifest `human_approvers` (owner fallback, unknown ids collapse to
  the system inbox); persona approval → owner; agent escalation chain (spawned_by →
  Admins if offline via the presence map + 60 s eviction janitor → Owners when Urgent,
  dedup'd) wired into `fail_agent`; budget warning/exhausted and security-finding
  helpers are real, tested routing contracts whose emission triggers are documented as
  task-191/task-137 scope; MR reverted → author + Admins (fabricated "default" tenant
  removed); merge-queue paused → Admins/Owners (wired in `pause_merge_queue`);
  breaking change → dependent-workspace members (interim superset, spec amendment
  records the team-lead dependency on task-122/125); spec drift → spec owner;
  invitation → invited user.
- **Wiring**: `notify`/`notify_rich` fan out through `dispatch_to_channels`; all four
  production `DivergencePorts` sites set `on_created` (None only in tests); eight
  further direct fan-out call sites.
- **Preferences API**: GET/PUT self-scope (resolver-exempt with per-handler auth —
  handlers touch only the authenticated caller's row), rejects `in_app: false` and
  non-absolute webhook/Slack URLs; durable storage via migration 000059 +
  `UserChannelPreferenceRepository` implemented in SQLite, Postgres (real Diesel), and
  mem (contract-guarded).
- No fake completion: coverage rows 16/17/18 remain `task-assigned`.

## Notes for remote verification (infrastructure, not defects)

- `awk` is mawk-only here; the six gawk-syntax gates (notification-priority,
  notification-audience, notification-test-coverage, tenant-filter, path-scope-binding,
  cross-surface-parity) must be run under gawk on the host.
- `cargo test --all` (acceptance criterion 10) belongs to host verification; the
  focused suites above all pass.

## Non-blocking observations (future hardening, no contract violation)

- User-configurable webhook URL is an SSRF surface in multi-tenant deployments; the
  spec designs `WebhookConfig.url` this way and mandates no mitigation (cf. GitHub
  webhooks). Consider an egress allowlist/private-range block when specced.
- Webhook secret persisted as plaintext JSON — the spec models `secret: String`.
- Email outbox has no drain job yet; task-113 owns §Email Notifications, and this
  task's criterion explicitly requires only the queue interface.
- `notify_persona_approval_requested` resolves Tenant-scoped personas to the tenant's
  first workspace (arbitrary but skip-not-fabricate; recipient is correct).
