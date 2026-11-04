# Review — task-112 (Notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate under review: `a1a25b2ad005520900b604ac3450c30ff6887e75`
(base `2427ccc14cf6e0c7d732a3f021ca0bdb14242f47`; all 5 task-listed commits are
ancestors of the candidate).
Verdict: **needs-revision** (one blocking finding).

## Round 1

Focused probes (evidence in `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test graph_integration
  test_divergence_detection_creates_notifications` → **FAILED** (see F1).
- `cargo test -p gyre-server --lib notification_dispatcher` → 15 passed, 0 failed.
- `cargo test -p gyre-server --lib api::users` → 13 passed, 0 failed (includes
  the four channel-prefs endpoint tests).
- Invariant scripts all exit 0: check-fabricated-scope-defaults,
  check-abac-exempt-handlers, check-abac-route-registry,
  check-migration-versions, check-unbounded-external-http,
  check-in-memory-state-stores, check-mem-port-contracts,
  check-migration-sql-portability.

Verified working (no findings):

- **Domain types** match the spec's struct shapes exactly
  (`NotificationChannels{in_app,email,webhook,slack}`, `EmailConfig`,
  `WebhookConfig`, `SlackConfig`, `NotificationPriority` Low..Urgent ordered,
  `DigestFrequency` Immediate/Hourly/Daily/Off); `Default` keeps `in_app: true`.
- **Dispatcher** (`notification_dispatcher.rs`): per-channel `min_priority`
  threshold filtering (asserted both directions), webhook POST signed with real
  HMAC-SHA256 via ring (`X-Gyre-Signature` over the exact sent body — the test
  recomputes the signature over the captured body, so a signing bug fails it),
  Slack POST with channel override, email queued durably in the `kv_store`
  outbox (restart-surviving store, asserted). All outbound HTTP is
  latency-bounded (reqwest client with 10 s timeout). Production sender is a
  real `ReqwestSender`; the test double only substitutes the `HttpSender` port.
- **Escalation routing** (§Notification Routing for Agent Escalations):
  `escalation_recipients` implements the exact 3-step chain — `spawned_by`
  primary; offline spawner (real presence map with 60 s heartbeat eviction in
  `lib.rs::spawn_presence_eviction`) adds workspace Admins; Urgent always adds
  Owners; dedup'd. Wired into `spawn.rs::fail_agent` (replacing the previous
  fabricated-`"default"`-tenant notification).
- **Routing table rows**: persona approval → owner (wired in
  `personas.rs::create_persona`, `user:` prefix stripped, scope resolved not
  fabricated); merge queue paused → all Admins/Owners (wired in
  `merge_processor.rs::pause_merge_queue`); MR reverted → author + workspace
  Admins (tenant resolved from workspace, never "default"); spec approval →
  manifest `approvers` (rewired in `specs_assist.rs::save_spec` from the old
  `system`-inbox placeholder to real manifest approvers with owner fallback);
  spec drift → spec owner (`spec_link_staleness.rs::notify_spec_owner_of_drift`,
  wired at line 94); gate failure / MR merged / invitation rows pre-existing
  and now fan out to channels via the central `notify`/`notify_rich` helpers.
- **Preferences API**: `GET/PUT /api/v1/notifications/preferences` registered,
  ABAC-exempt with genuine per-handler self-scope (handlers touch only
  `resolve_user_id(&auth)`'s own row), PUT rejects `in_app: false` and
  non-absolute webhook/Slack URLs (tested). Storage is port-backed
  (`UserChannelPreferenceRepository`) with SQLite (Diesel upsert), Postgres, and
  mem adapters enforcing the same upsert contract; migration 000056 uses the
  next free sequence number and portable SQL. AppState wiring uses the
  `store!` pattern — no bare in-memory state.
- The diff also fixes a pre-existing fabricated-scope violation
  (git_http.rs ws_id fallback) and removes its exemption entry (frozen count
  7→6, correct direction).

Findings:

- **F1 (major, blocking): divergence notifications lost `entity_ref`/`repo_id`
  and `cargo test --all` fails.** While adding the `on_created` channel fan-out
  callback to `check_divergence`
  (`crates/gyre-server/src/graph_extraction.rs`, diff hunk at ~line 600), the
  candidate deleted:

  ```rust
  notif.entity_ref = Some(spec_ref.to_string());
  notif.repo_id = Some(repo_id.as_str().to_string());
  ```

  ConflictingInterpretations notifications no longer carry their deep-link
  entity reference or repo scoping — a regression of shipped inbox behavior,
  unrelated to task-112's scope. The existing integration test catches it:
  `test_divergence_detection_creates_notifications` FAILS at
  `tests/graph_integration.rs:1167` (`left: None, right:
  Some("specs/system/auth.md")`), violating the acceptance criterion
  "`cargo test --all` passes". Reproduce:

  ```
  SKIP_WEB_BUILD=1 cargo test -p gyre-server --test graph_integration \
    test_divergence_detection_creates_notifications
  ```

  Fix: restore the two assignments (they are independent of the fan-out
  callback).
- **F2 (moderate, non-blocking): three routing helpers have no production
  emitters — those §Who Gets Notified rows are dormant.**
  `notify_agent_budget_warning`, `notify_agent_budget_exhausted`, and
  `notify_security_finding` are called by no production code (only
  persona-approval, agent-escalation, and merge-queue-paused helpers are
  wired). The budget-warning/exhausted and security-finding rows therefore
  never fire today. This is honestly disclosed in code comments (triggers owned
  by task-191 / task-137, both `not-started`) and the helpers themselves are
  correct with real recipient-set tests, so it is a legitimate decomposition —
  but the coverage claim for those rows is "routing contract ready", not
  "shipping behavior". Verifiers must not treat them as live until
  task-191/137 land.
- **F3 (minor): "Breaking change detected → dependent repo team leads" row is
  not implemented and currently cannot be.** The domain has no team-lead
  concept (`Team` carries only `member_ids`; no repo→team mapping). The
  pre-existing path (`git_http.rs::process_breaking_changes`) notifies all
  members of the dependent repo's workspace — a superset, unchanged by this
  candidate. The shipped summary's "existing creation paths already cover the
  remaining rows" omits this row. Needs a spec-lifecycle decision once Teams
  gain leads (task-122/125 territory): amend the row or route to the closest
  available concept.

Minor (recorded, not blocking):

- `NotificationType::PersonaApprovalRequested` doc comment says "Priority 3"
  but `default_priority()` returns 2 (Urgent band) — cosmetic inconsistency
  with no behavioral defect (spec assigns no numeric priority to this type).
- `dispatch_to_channels` performs the (10 s-bounded) webhook/Slack POST
  synchronously inside request handlers via `notify_rich` (e.g.
  `invite_member`), so a slow webhook endpoint adds up to 10 s to that request.
  Best-effort and latency-bounded, so within the repo's invariants, but a
  future outbox/async fan-out would be the production-grade shape.

Conclusion: the implementation is real (real HMAC, real HTTP sender, real
presence-based online checks, real durable preference storage, real routing
recipient resolution) and the test suite is meaningful (signature recomputed
over the captured body, threshold filtering asserted both directions,
negative recipient assertions per routing row). One accidental regression
(F1) breaks an existing test and the `cargo test --all` acceptance criterion,
so the candidate needs a revision round to restore the two deleted lines.
