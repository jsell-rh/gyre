# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Assigned base `d9c419a0`, candidate `0800abaf` (merge-base `8729385d`; the
base→candidate negative diff is mainline advancement after the merge-base —
task-191/037/202 work the candidate branch has not yet absorbed — not a
candidate regression; integration is a controller stage. Diesel migration
versions are date-scoped, so the candidate's 000060/000061 do not collide with
the base's same-numbered migrations from other dates).
Verdict: **needs-revision** — one material finding (F1, channel fan-out not
wired on seven production notification-creation paths, contradicting the
candidate's own "every creation path" claim); everything else verified clean,
including the prior round's F1 rustfmt repair.

## Round 4 (this review)

Probes run (checkout at candidate `0800abaf`; working-tree reset of
`specs/tasks/task-112.md` by controller staging left untouched):

- `python3 scripts/check-rustfmt-diff.py d9c419a0` (exact CI fmt gate vs
  assigned base) → exit 0, "changed lines clean (132 Rust files checked)" —
  prior round's F1 confirmed repaired.
- `cargo check -p gyre-server` → exit 0 (cold, 11m11s); forced lib recheck
  after `touch crates/gyre-server/src/notification_dispatcher.rs` →
  "Checking gyre-server v0.1.0 … Finished in 34.13s", exit 0.
- `cargo test -p gyre-domain` → 465 passed (incl.
  `user::tests::notification_channels_deserializes_nested_and_partial_flat`).
- `cargo test -p gyre-adapters channel` → 2 passed
  (`sqlite::tests::migration_000060_rewrites_legacy_notification_channels` —
  the real embedded up.sql driven against seeded flat/nested/NULL rows on real
  SQLite; `sqlite::user_profile::tests::channel_prefs_roundtrip_and_upsert_replaces`).
- `cargo test -p gyre-common` → 124 passed.
- `cd web && npx vitest run src/__tests__/Inbox.test.js
  src/__tests__/WorkspaceHomeQueuePause.test.js` → 41 passed.
- Invariant scripts, all exit 0: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers (77 handlers), check-migration-versions,
  check-mem-port-contracts, check-migration-sql-portability,
  check-unbounded-external-http, check-in-memory-state-stores,
  check-byte-slice-truncation, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-inert-enforcement,
  check-lossy-secret-conversion. ABAC exemption files unchanged vs base
  (no growth); the preferences route is a resolver `RouteResourceMapping::exempt`
  entry with per-handler self-scope auth (handler reads/writes only
  `resolve_user_id(&auth)` rows), satisfying check-abac-exempt-handlers.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 **against the
  committed candidate file** (the working-tree failure is the controller's
  unstaged `not-started`/`commits: []` reset of task-112.md, restored exactly
  after the probe; committed frontmatter lists all seven task-labeled SHAs).
- Full evidence under `/tmp/stage/review-evidence/`.

Verified working (no findings):

- **Domain types (§Delivery Channels):** `NotificationChannels { in_app,
  email: EmailConfig, webhook/slack: Option<…> }` with per-channel
  `min_priority`, `DigestFrequency`, `NotificationPriority` (Low..Urgent,
  Ord-derived). `in_app` forced true (PUT rejects false; Default sets true).
  Flat→nested stored-shape compat via custom `Deserialize` + migration
  `000060` convergence; the migration's REPLACE-needle approach matches the
  pre-task-112 declaration-order serialization and is proven by a real-SQLite
  migration test covering all flat combinations, pass-through siblings,
  already-nested and NULL rows, plus a documented partially-reversible down.sql.
- **Dispatcher:** `dispatch_with` gates each channel on its `min_priority`
  (inclusive `>=`), webhook POSTs JSON signed with real ring HMAC-SHA256
  (`X-Gyre-Signature`, hex), Slack POSTs to the incoming-webhook URL with
  optional channel override, email queues a durable `kv_store` outbox entry
  (interface per plan; SMTP is deployment's concern). All outbound HTTP is
  `ReqwestSender` with a 10 s timeout (unbounded-HTTP gate passes). Band
  derives from `NotificationPriority::for_notification_type` — the
  type-membership table recorded as a spec amendment in user-management.md —
  never from the HSI numeric priority; the unreachable-Owner bug from the
  earlier numeric mapping is covered by a regression test
  (`agent_escalation_notifies_owners_even_when_spawner_online`).
- **Routing (§Notification Routing for Agent Escalations):**
  `escalation_recipients` implements the full chain — `spawned_by` primary,
  offline spawner (presence/active-session check) adds workspace Admins,
  Urgent always adds Owners, dedup'd — and `spawn.rs fail_agent` routes its
  AgentFailed notification through `notify_agent_escalation` with the tenant
  resolved from the workspace (skip+log, never fabricated).
- **Routing table (§Who Gets Notified):** persona approval → owner (with
  Admins/Owners fallback, `user:` prefix stripped); merge-queue pause →
  Admins/Owners (wired in merge_processor); MR reverted → author + workspace
  Admins (new, with tenant resolution replacing the old `"default"`
  fabrication); budget warning/exhausted rows live via budget.rs
  `maybe_notify_budget_threshold` (spawner + Admins/Owners per table,
  `notify_rich` → fan-out); security finding → Owners + tenant Admins
  (helper, see F2); team-lead rows resolved by recorded spec amendment
  (interim superset) since no lead concept exists — amendment is in the spec,
  not a reinterpretation.
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` over the
  port-backed `user_channel_preferences` table (SQLite + PostgreSQL + mem
  adapters, `store!` wiring); PUT validates absolute webhook/Slack URLs;
  router-driven tests assert defaults, round-trip through a fresh router over
  the same state (durable store), rejection of `in_app: false` and relative
  webhook URLs.
- **Wiring (partial — see F1):** notify/notify_rich fan out after successful
  create (and now return early on create failure); extraction notifications
  fan out via a `DivergencePorts.on_created` callback wired at every
  `extract_and_store_graph` call site (git_http, api/repos, mirror_sync);
  abandoned_branch, trust_suggestion, spec_link_staleness, spec_patrol,
  constraint_check, mcp all dispatch after create.
- **Frontend:** additive PascalCase→snake_case label-map entries in
  Inbox.svelte / WorkspaceHome.svelte for the new notification types; focused
  vitest suites pass. (Generated `web/dist/` observations belong to the
  deterministic verifier per checkpoint policy.)

Findings:

- [.] **F1 (major): seven production notification-creation paths never reach
  the delivery-channel dispatcher — the configured email/webhook/slack
  channels silently do not fire for them, contradicting the candidate's own
  completeness claims.** `dispatch_to_channels` is not called after the
  in-app create at: `git_http.rs:2195` (persona-push hook,
  PersonaChangePendingApproval — High band per this task's own spec
  amendment — to the persona owner; the same file's extraction notifications
  ARE wired via DivergencePorts, so "git_http.rs" in the shipped wiring list
  is only half true), `reconciliation.rs:1427` (MetaSpecDrift sweep),
  `secret_rotation.rs:542` (SecretExpiring — High), `prompt_assembly.rs:505`
  (stale meta-spec pin), `signal_chain.rs:537` (CrossWorkspaceSpecChange),
  `impersonation.rs:803` (ImpersonationRequest — High) and
  `impersonation.rs:852` (ImpersonationAlert — **Urgent**, banded by this
  task's own amendment with the rationale "an admin is acting as this user
  right now"; only a live-WS push follows, so a user relying on a configured
  Urgent webhook/email/Slack channel gets nothing). This falsifies the
  dispatcher doc comment ("fans every persisted in-app notification to
  configured channels"), the task-112 Shipped notes ("called from every
  notification creation path"), and coverage row 16's note — §Delivery
  Channels is marked implemented on that claim. Most of these paths arrived
  on the branch via the controller-orchestrated merge rounds (impersonation,
  secret rotation, signal chain, prompt assembly), i.e. the wiring claim was
  true at an earlier checkpoint and silently rotted as merges landed.
  Reproduce (read-only): `grep -rn "notifications\.create(" crates/gyre-server/src/
  --include=*.rs` and cross-check each non-test site for a following
  `dispatch_to_channels`/`notify_rich`/`on_created`; evidence with exact sites
  in `/tmp/stage/review-evidence/fanout-bypass-audit.txt`.
  **Repair path:** add the post-create `dispatch_to_channels` call (or route
  through `notify_rich`) at all seven sites, extend the dispatcher test set
  with one guard per newly wired emitter path where feasible, update the
  Shipped notes/coverage wording, commit, and append the SHA to task-112.md's
  `commits:` frontmatter.
- [-] **F2 (minor): dead routing helpers with an inaccurate justification.**
  `notify_agent_budget_warning` / `notify_agent_budget_exhausted` /
  `notify_security_finding` in notification_dispatcher.rs have no production
  caller in the candidate or the assigned base — the live budget emission is
  budget.rs's own inline recipient resolution (candidate:
  `maybe_notify_budget_threshold`; base/task-191: `notify_budget_level`,
  same recipients, `notify_rich` → fan-out preserved after integration), and
  no security-scanner emitter exists product-wide (pre-existing recorded gap,
  owned by task-137). The Shipped note "task-191's enforcement helper … calls
  this function" is factually wrong. The helpers are correct and tested but
  unreachable; budget routing duplication risks future drift between the two
  implementations. Suggest consolidating (have budget.rs call the helpers, or
  delete them) in the F1 repair round; not blocking on its own since the
  specced behavior is live through the wired path.

Test-inflation check: the new tests are substantive — router-driven API tests
against the real state/store, real-SQLite migration test driving the real
embedded up.sql, dispatcher tests whose threshold assertions fail if filtering
breaks (low-band must NOT deliver to a High-threshold channel) and whose
escalation assertions enumerate exact recipient sets per role. One soft spot,
not blocking: the webhook-signature test computes the expected value with the
same `sign_payload` function (mirrored), though it does verify the header is
the HMAC of the exact sent body under the configured secret, and the
production code is plainly ring HMAC-SHA256.

Deferred to the independent verifier (wrapper-blocked at review stage; list in
`/tmp/stage/review-evidence/deferred-verifier-probes.txt`):
`cargo test -p gyre-server --lib notification_dispatcher`,
`cargo test -p gyre-server --lib channel_preferences`,
`cargo test -p gyre-server --lib merge_processor`, `cargo test -p gyre-adapters`
(full), `cargo test --all` (acceptance criterion), the CI clippy gate
(`python3 scripts/check-clippy-diff.py <integrated-base>`), and the full web
suite (`cd web && npm ci && npm test`).
