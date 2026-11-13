# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate: `e9b7e214` (base `f764bc00`; task-scoped diff taken against merge-base
`8729385d`, the newest assignment base the branch merged — all earlier merged
bases are its ancestors, so `git diff 8729385d..e9b7e214` is the pure task-112
change set; commits recorded in task-112.md frontmatter).
Verdict: **needs-revision** — one finding (F1, split-brain channel config store).
Evidence: `/tmp/stage/review-evidence/review-round-evidence.md`.

## Probes run (candidate checkout, /tmp/stage/bin/cargo)

- `cargo test -p gyre-domain` → 465 passed; `cargo test -p gyre-common` → 124 passed.
- `cargo test -p gyre-adapters channel_prefs` → passed (SQLite round-trip/upsert-replaces).
- `cargo test -p gyre-adapters migration_000060` → passed (real embedded up.sql over
  seeded flat/custom/in_app=false/nested/NULL rows; converges to nested shape).
- `cargo check -p gyre-server` → clean. `--all-targets` blocked by the build wrapper
  (verifier's domain) — server test list recorded in the evidence file.
- rustfmt diff gate (`scripts/check-rustfmt-diff.py 8729385d`, 39 files) → clean
  (the round-1 fmt violation is gone).
- All mechanical invariant scripts (arch, ABAC registry, exempt handlers,
  migration versions/portability, inert enforcement, unbounded HTTP, lossy
  secrets, fabricated scope defaults, scope literals, mem port contracts, dead
  message kinds, in-memory stores) → OK.

## Verified working (no findings)

- **Domain types (§Delivery Channels):** `NotificationChannels` reshaped exactly
  per spec — `in_app` (forced true; PUT rejects false), `EmailConfig
  {enabled, digest, min_priority}`, optional `WebhookConfig {url, secret,
  min_priority}` / `SlackConfig {webhook_url, channel, min_priority}`;
  `NotificationPriority` Low/Medium/High/Urgent with `for_notification_type`
  deriving the band from the notification TYPE (never the HSI §8 number — the
  amendment table in user-management.md records why). Flat→nested stored-shape
  compat via custom `Deserialize` + migration 000060 convergence, with hard
  tests on both (domain serde tests + the real-up.sql adapter test).
- **Dispatcher:** `dispatch_to_channels`/`dispatch_with` fans every persisted
  in-app notification to configured channels, each gated by inclusive
  `min_priority`. Webhook POST is genuinely HMAC-SHA256-signed (ring
  `HMAC_SHA256` over the exact body bytes, `X-Gyre-Signature` header); Slack
  posts to the incoming-webhook URL with channel override; email queues a
  durable `kv_store` outbox entry (SMTP interface per plan — the acceptance
  criterion explicitly allows the stub). All outbound HTTP latency-bounded
  (10 s reqwest timeout).
- **Routing (§Who Gets Notified):** escalation chain `escalation_recipients`
  (spawned_by primary → workspace Admins when spawner has no live presence
  entry → workspace Owners when Urgent, deduplicated) with real
  presence-map/active-session check, wired on the production `fail_agent`
  escalation path; persona approval → owner ("user:" stripped, Admins/Owners
  fallback, scope never fabricated); merge-queue pause → Admins+Owners;
  MR reverted → author + workspace Admins (tenant resolved from workspace,
  never "default"); spec approval → manifest `approvers` (fallback owner,
  unknown ids collapse to the visible "system" inbox); spec drift → spec owner
  via ledger entry. Budget rows are a single implementation: api/budget.rs
  `maybe_notify_budget_threshold` classifies the level and calls the
  dispatcher's `notify_agent_budget_warning`/`notify_agent_budget_exhausted`
  (recipients spawner+Admins / spawner+Owner, per-period dedup) — the prior
  round's duplicated-unreachable-helpers finding is repaired. The
  security-finding helper (Owner + tenant Admin) remains the routing contract
  with no product-wide emitter — honestly recorded as task-137's gap, unchanged
  from the prior round's accepted disposition.
- **Fan-out wiring (prior round F1 repair):** all 22 `notifications.create(`
  sites audited — every production site dispatches to channels (else-branch or
  via notify/notify_rich/on_created callback); remaining hits are `#[cfg(test)]`.
  Emitter-path guard tests assert a configured email channel queues outbox
  entries for each repaired emitter (reconciliation, secret rotation, prompt
  assembly, signal chain, impersonation request+alert, git_http persona push).
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` registered,
  ABAC-exempt with per-handler self-scope auth (handlers touch only
  `resolve_user_id(&auth)`'s own row), `in_app: false` rejected, non-absolute
  webhook/Slack URLs rejected; backed by the `user_channel_preferences` table
  (migrations 000060/000061, next-unused numbers, portable SQL) with real
  SQLite/PostgreSQL/mem adapters (mem guards the upsert contract in code).
- **UI source:** Inbox/WorkspaceHome add the new type→label mappings
  (PersonaApprovalRequested, MergeQueuePaused, BudgetExhausted, SecurityFinding,
  MrReverted) — cosmetic normalization only, consistent with the existing map.

## Findings

- **F1 (moderate, blocking):** split-brain channel config. `PUT /api/v1/users/me`
  still accepts and persists `preferences.notification_channels`
  (UserPreferencesPatch → users.preferences, echoed by GET /users/me), but the
  dispatcher reads only the `user_channel_preferences` table; nothing in
  production reads the users.preferences copy for delivery. A user enabling
  email/webhook/slack via /users/me gets a persisted, echoed config that
  silently never fires — the same "configured channel never fires" class as the
  prior round's fan-out bypass. The task even migrates users.preferences rows
  to the nested shape (000060) while delivery ignores that store. Repair:
  sync the two on write (upsert user_channel_prefs when /users/me carries
  notification_channels), or fall back to the user's stored UserPreferences
  channels in dispatch_to_channels when no table row exists, or reject the
  field with a pointer to PUT /api/v1/notifications/preferences — plus a guard
  test (config set via /users/me reaches the outbox, or is rejected).

## Non-blocking observations (recorded for the verifier / next rounds)

- The webhook-signature test computes its expected value via the same
  `sign_payload` helper (mirrored logic); the essential property (signature over
  the exact sent body with the configured secret) is still verified, and
  `sign_payload` is a 4-line genuine ring HMAC-SHA256 — but an independent
  known-vector assertion would harden it.
- `merge_processor.rs` circuit-breaker escalation still notifies all workspace
  members with tenant literal "default" — pre-existing base code, unchanged by
  this candidate; a superset consistent with the amended team-lead note, and
  channel fan-out now applies there via notify_rich.
- The spec's amended band table does not enumerate the main-merged types banded
  in code (AgentFailed, SystemAlert, SpecDriftDetected, DependencyStale,
  MrMerged, GatePassed, InvitationReceived, MembershipChanged,
  BreakingChangeDetected); each carries an in-code rationale. Doc-completeness
  follow-up, not a contradiction.
- `docs/api-reference.md` does not yet document the new
  GET/PUT `/api/v1/notifications/preferences` endpoint.
- Integration note: the branch has not yet merged the current base f764bc00
  (1458 commits ahead of the merge-base); the controller rebase will need to
  resolve migration-number collisions (base ships its own 000060/000061) and
  the usual conflict set — controller concern, not a candidate defect.
- Verifier must run the server test suite (list in the evidence file) and full
  `cargo test --all`; server test compilation is blocked in this sandbox by the
  build wrapper (infrastructure, not a code failure).
