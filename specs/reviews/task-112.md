# Review — task-112 (notification delivery channels & routing)

Task: `specs/tasks/task-112.md` — implement `user-management.md` §Delivery Channels,
§Who Gets Notified, §Notification Routing for Agent Escalations.

Candidate under review: `1abb305a` (branch
`pipeline/task-112/b09b397040eb4abaaa88567823eaf182-2`), assigned base `6f5a631c`.
Tree since the last reviewed checkpoint: review-repair commit `64b30291`
(1cae4a25 F1–F4 repairs) + merge of base `6f5a631c` (`b951b59a`) + process commits
(`ee1ff23f`, `1abb305a`). Verdict: **complete / approved**.

## Round 3 (this review) — repair + merge verification

### Prior findings (1cae4a25) — all repaired, verified against source

- **F1 (ownerless fan-out unwired):** `workspace_ownership.rs:145`
  `notify_tenant_admins` now calls
  `notification_dispatcher::dispatch_to_channels(state, &notif)` in the `Ok` arm
  after `state.create_notification` persists. Guard test
  `ownerless_notification_fans_out_to_configured_channels`
  (workspace_ownership.rs:550) is hard: seeds a `user_channel_prefs` email row,
  runs the real `run_once` job, and asserts BOTH the in-app record AND an
  `email_outbox` kv entry keyed `admin-chan:{id}` — it fails if the dispatch call
  is removed. Verified no other production creation path persists without
  dispatch: audited every `create_notification` / `notifications.create(` call
  site in gyre-server (abandoned_branch, notifications, spec_patrol,
  signal_chain, prompt_assembly, trust_suggestion, mcp, spec_link_staleness,
  reconciliation, impersonation ×2, secret_rotation, constraint_check, git_http,
  workspace_ownership) — each is followed by `dispatch_to_channels` or goes
  through `notify_rich`/`notify` (both fan out internally); the push-scoped
  paths (git_http:1312, mirror_sync:89, api/repos:664/776) compose the SSE
  broadcasting wrapper with an `on_created` channel-fan-out hook. Bare
  `.create(..).unwrap()` sites are all inside `#[cfg(test)]` modules.
- **F2 (false team-lead premise):** the spec note (user-management.md §Who Gets
  Notified) and coverage row 17 now state the accurate gap — `Team.lead` ships
  (migration 000062); the missing piece is the repo→team ownership mapping.
  Confirmed against the domain: no repo↔team link exists to resolve through.
- **F3 (spec-owner routing):** `notifications::resolve_owner_ref`
  (notifications.rs:615) resolves `user:{name}` by id/username/display-name and
  `team:{slug}` via members, returning an empty vec + warn when unresolvable
  (no fabricated recipient). `spec_patrol.rs:411-468` adds the drifted spec's
  ledger owner to the recipients deduplicated over the accountability superset;
  `reconciliation.rs:1883-1899` does the same for the MetaSpecDrift sweep's
  pinned-spec owners. Tests are discriminating:
  `spec_drift_notification_routes_to_spec_owner` proves a plain **Viewer** owner
  (no accountability role) is notified — owner routing orthogonal to role
  routing — and an Admin-who-is-owner gets exactly one notification (dedup);
  `spec_drift_notification_routes_to_team_owner_ref` and
  `sweep_drift_notification_routes_to_meta_spec_owners` cover the other forms.
- **F4 (fabricated "default" tenant):** `complete_agent` (spawn.rs:~2660)
  resolves the tenant from the MR's workspace record and skips + warns when
  unresolvable; `trip_circuit_breaker` (merge_processor.rs:658+) does the same.
  No literal `"default"` tenant remains on these notification paths (remaining
  occurrences are test fixtures and unrelated code).

### Merge onto base `6f5a631c` — no semantic regressions found

Base-side changes integrated: `agent_messages`/`messages` module removals (no
stale references — `cargo check -p gyre-server` exits 0), task-195 manifest
gate selection, task-109/116/125 notification variants
(`ImpersonationAlert`, `PersonaChangePendingApproval`,
`ComputeTargetUnavailable`). The dispatcher's band mapping
(`NotificationPriority::for_notification_type`, gyre-domain/src/user.rs:291)
classifies all base-side new variants — no `_ =>` catch-all gap (match is
exhaustive over the enum, so a future variant fails compilation rather than
silently defaulting). Budget routing is converged with the base's
level-crossing architecture: `api/budget.rs:notify_budget_level` builds the
`BudgetNotificationContext` and delegates to the dispatcher's
`notify_agent_budget_warning`/`notify_agent_budget_exhausted`, which own
recipient resolution and per-period dedup — single implementation. No conflict
markers anywhere in the tree (scanned .rs/.md/.svelte/.sql).

### Acceptance criteria — evidence

- **Channel types:** `NotificationChannels { in_app, email: EmailConfig,
  webhook: Option<WebhookConfig>, slack: Option<SlackConfig> }` plus
  `DigestFrequency { Immediate, Hourly, Daily, Off }` and
  `NotificationPriority { Low..Urgent }` (gyre-domain/src/user.rs:147-260)
  — matches the spec struct-for-struct. Custom `Deserialize` keeps pre-task-112
  flat stored rows readable; migration 000060 converges stored shapes with
  portable REPLACE/LIKE SQL (000061 creates `user_channel_preferences`).
- **Dispatcher / HMAC / Slack / email queue:** `dispatch_with` fans out per
  channel gated by `min_priority` (inclusive `>=`, PartialOrd Low<Urgent).
  Webhook signature is real crypto — `ring::hmac::sign(HMAC_SHA256, secret)` over
  the exact sent body bytes, hex-encoded in `X-Gyre-Signature`; the test asserts
  the header matches HMAC over the captured body. Slack POSTs (with channel
  override). Email queues a durable `kv_store` outbox entry. All outbound HTTP
  is latency-bounded (reqwest client built with a 10 s timeout —
  check-unbounded-external-http passes).
- **Routing table:** every §Who Gets Notified row is implemented (spec approval
  → manifest approvers incl. `team:{slug}` resolution, persona approval → owner
  with disclosed Admin/Owner fallback, budget warning/exhausted, gate failure,
  MR merged/reverted, queue pause, spec drift → owner (F3), security finding →
  Owner + tenant Admin with the emitter gap disclosed as task-137's, invitation)
  or covered by the recorded spec amendment (team-lead rows: interim superset
  pending a repo→team mapping — F2-corrected).
- **Escalation routing:** `escalation_recipients` implements
  spawned_by-primary → offline-spawner adds Admins (live presence-map check) →
  Urgent always adds Owners, deduplicated; band membership derives from the
  notification type so the Urgent step is reachable exactly for specced types.
- **Preferences API:** `GET/PUT /api/v1/notifications/preferences` (+
  `/users/me/notification-preferences`) registered in api/mod.rs:922/944,
  ABAC-exempt with per-handler self-scope auth (`resolve_user_id` derives
  exclusively from the authenticated caller); `validate_channel_config` rejects
  `in_app: false` and non-absolute webhook/Slack URLs; updates persist to BOTH
  `user_channel_prefs` (what delivery reads) and `users.preferences` (what
  GET /users/me echoes) — single effective config.
- **`cargo test --all`:** deferred to the independent verifier — this sandbox's
  build wrapper blocks gyre-server test compilation (4 GiB budget) and
  `--all-targets` checks. This round's evidence below covers everything the
  sandbox permits.

### Probes run this round (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → **exit 0** (7m22s, cold).
- `cargo test -p gyre-domain --lib` → **509 passed, 0 failed**.
- `cargo test -p gyre-common --lib` → **125 passed, 0 failed**.
- All 19 mechanical invariant scripts → **exit 0**: arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, migration-versions,
  dead-message-kinds, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults,
  lossy-secret-conversion, scope-literal-defaults, inert-enforcement,
  migration-sql-portability, forged-scope-fields, forwarded-header-trust,
  in-memory-state-stores, unbounded-external-http.
- `check-task-commit-attribution.sh` → **exit 0 against the committed
  candidate** (the working-tree task file was reset by the controller to
  `commits: []` for this round; re-running with the committed `1abb305a`
  version of the file passes — the initial failure is a round-setup artifact,
  not a candidate defect; working tree restored afterward).
- `cargo test -p gyre-server --lib notification_dispatcher` and
  `cargo check -p gyre-server --all-targets` → **blocked by the sandbox build
  wrapper** (infrastructure restriction, not a code defect; logs retained).

### Verifier must execute (recorded per protocol)

- `cargo test --all`
- `cargo test -p gyre-server --lib notification_dispatcher`
- `cargo test -p gyre-server --lib workspace_ownership`
- `cargo test -p gyre-server --lib spec_patrol`
- `cargo test -p gyre-server --lib sweep_`
- `cargo test -p gyre-server --lib circuit_breaker` and `complete_agent`
- `cargo test -p gyre-server --lib budget` and the preferences endpoint tests
  (`put_channel_preferences`, `put_me_notification_channels`,
  `fans_out_to_configured_channels`)

### Generated-output observation (non-blocking, verifier-owned)

`web/dist/` was not rebuilt by the agent in this checkpoint — expected; the
deterministic verifier rebuilds and binds publication to the tested tree. The
authored frontend changes (Inbox.svelte, WorkspaceHome.svelte) are label-map
additions for the new notification types — reviewed, correct, and trivially
safe.
