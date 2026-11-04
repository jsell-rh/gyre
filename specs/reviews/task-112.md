# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate under review: `2138280c4ae0277d21960141a4cd4b003f06160d` (base
`ed57c723`). Verdict: **needs-revision** (1 blocking finding).

## Round 1 — independent review

Focused probes (all on the exact candidate HEAD, working tree clean before and
after; temporary probe source restored byte-identically and re-verified with
`api::personas` 8 passed):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib notification_dispatcher`
  → **15 passed** (HMAC signing over the exact sent body, threshold filtering
  per channel, slack posting + channel override, email outbox durability,
  escalation chain incl. offline-spawner and Urgent fan-out, persona approval,
  merge-queue-paused, budget warning/exhausted, security finding).
- `… --lib api::users` → **13 passed** (channel-prefs API: defaults,
  round-trip across a fresh router over the same state, rejected `in_app:
  false`, rejected relative webhook URL).
- `… --lib spawn::` → 34 passed; `… --lib merge_processor::` → 45 passed;
  `… --lib api::personas` → 8 passed; `… --lib spec_link_staleness` →
  7 passed; `cargo test -p gyre-adapters --lib user_profile` → 5 passed
  (SQLite upsert-replaces round-trip); `cargo test -p gyre-domain --lib
  user::` → 5 passed.
- `… --test graph_integration test_divergence_detection_creates_notifications`
  → **1 passed** — the repair-round regression probe the implementer could
  not run (no crates.io in their sandbox) now passes on this HEAD; the
  restored `entity_ref`/`repo_id` lines are present in `graph_extraction.rs`
  (`check_divergence`, lines 603-604).
- Invariant gates (all exit 0): arch, abac-route-registry, abac-exempt-
  handlers, mcp-write-tools, migration-versions, migration-sql-portability,
  dead-message-kinds, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults
  (exemption count honestly lowered 5→4 alongside the real
  `git_http.rs:1495` fix — skip+log, not a fabricated scope), scope-literal-
  defaults, lossy-secret-conversion, inert-enforcement, in-memory-state-
  stores, unbounded-external-http, forwarded-header-trust, forged-scope-
  fields, task-commit-attribution.
- Web: `npm ci` + `npx vitest run src/__tests__/Inbox.test.js` → 33 passed.
- Sandbox restriction (environmental, not a code defect): this sandbox has
  mawk, not gawk, so `check-notification-audience.sh`,
  `check-notification-priority.sh`, `check-notification-test-coverage.sh`,
  `check-self-confirming-tests.sh`, and `check-crypto-verify.sh` abort with
  awk syntax errors. The verifier should re-run these five on a gawk host.
- Independent HMAC check: `sign_payload` uses `ring::hmac` HMAC-SHA256 +
  lowercase hex — genuine HMAC, cross-checked against Python `hmac`
  (algorithm-identical); the test also binds signature↔sent-body↔secret.

Verified working (no findings):

- **Domain types** match the spec structs exactly (incl. `DigestFrequency =
  Immediate/Hourly/Daily/Off`); `NotificationPriority` ordered Low..Urgent;
  `Default` keeps `in_app: true`; HSI §8 1–10 → 4-band mapping tested.
- **Dispatcher** fans each persisted in-app notification out to email
  (durable kv outbox — interface contract per the task plan), webhook
  (HMAC-SHA256 `X-Gyre-Signature`), and slack (channel override), each with
  per-channel `min_priority` filtering; all outbound HTTP latency-bounded
  (10 s reqwest timeout). Best-effort per channel, never blocks in-app.
- **Escalation routing** correctly resolves `spawned_by` → workspace Admins
  when the spawner is offline (presence is keyed by verified user id,
  ws.rs:194) → workspace Owners when Urgent, deduplicated; wired into
  `fail_agent` with tenant resolved from the workspace (skip+log on
  unresolvable, never fabricated).
- **Preferences API** is self-scope only (ABAC-exempt with per-handler auth
  — handler reads/writes exclusively the caller's own row), rejects
  `in_app: false` and non-absolute webhook/slack URLs; storage is a real
  port-backed table (portable migration 000056) with SQLite and mem adapters
  both enforcing upsert-replaces.
- **Wiring** is real: `notify`/`notify_rich` fan out centrally; gate
  failure, spec patrol, merge queue (paused → Admins/Owners), abandoned
  branch, trust suggestion, spec-link staleness (spec-owner drift row),
  constraint violations, MCP uncertainty, git_http (divergence + spec
  assertion + cross-workspace dependents), spec-conflict, and
  spec-pending-approval (manifest approvers → owner fallback) all route
  through it; `emit_reconciliation_completed` now skips unresolvable scope
  instead of fabricating `"default"`.
- **Spec amendment** (56059f8d) for the team-lead rows is honest: the domain
  has no Team lead concept; interim superset routing recorded with the
  task-122/125 dependency.
- Honest decomposition boundary in the shipped summary (budget
  warning/exhausted and security-finding helpers are routing contracts whose
  emission triggers are owned by task-191/task-137) — corroborated by grep:
  those three helpers have no production callers yet.

Findings:

- [ ] **F1 (blocking, code): persona-approval routing delivers to a phantom
  recipient — the "Persona approval requested → Persona's owner" row is
  non-functional in the primary human flow.**
  `crates/gyre-server/src/api/personas.rs:147` sets
  `persona.owner = Some(auth.agent_id.clone())`, but for JWT/API-key auth
  `auth.agent_id` is the user's **display_name** (`auth.rs:580, 675, 788`),
  not the user id (and `"system"` for the global dev token);
  `notify_persona_approval_requested` then creates the notification for
  `Id::new(display_name)`. Reproduced with a temporary probe test (restored;
  source in review-evidence): authenticated as `{agent_id: "Alice Display
  Name", user_id: Some(user-1)}`, after `create_persona` → notifications for
  `user-1` = **0**, notifications for `"Alice Display Name"` = **1**. The
  real creator never sees the approval request; `display_name` is
  additionally user-editable (`update_me`), so the "owner" identity is
  unstable. The dispatcher tests hand-construct the persona with owner
  `"user:persona-owner"`, so the wiring defect ships green. Remedy: derive
  the owner from `auth.user_id` (the dispatcher already strips a `user:`
  prefix, indicating the intended `user:<id>` form), and add an API-level
  test that creates a persona as an authenticated user and asserts the
  notification lands on that user's id.
- [ ] **F2 (minor, code): `specs_assist.rs` save_spec comment claims a
  safety net that does not exist.** The comment says "A recipient id with no
  registered user maps to the `system` inbox so the request is still
  visible rather than dropped", but the code maps manifest approvers/owner
  raw to `Id::new(name)` with no registered-user check — an approver
  identity that doesn't match a real user id is silently dropped, not made
  visible. Either implement the mapping or fix the comment.
- [ ] **F3 (minor, code): `web/src/components/Inbox.svelte` dropped
  `TaskCreated: 'task_created'` from `typeNormMap`**, so TaskCreated
  notifications keep PascalCase and no longer match the
  `'task_created'` action-button branch (~line 700), losing the View
  Spec/View Agent buttons; inconsistent with WorkspaceHome.svelte which
  still normalizes. Inbox tests (33 passed) don't cover that branch.

Remedy for approval: fix F1 (with a real API-level test), and address F2/F3
or explicitly defer them. The verifier should additionally re-run the five
gawk-dependent gates listed above on a gawk host.
