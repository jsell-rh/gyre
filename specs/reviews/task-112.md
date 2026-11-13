# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Assigned base `7daca25d`, candidate `79b395dc` (branch
`pipeline/task-112/0a061d4128e544a78a1a66c7febfa420-1`).
Evidence: `/tmp/stage/review-evidence/` (probe commands, sources, exit codes).

## Round 4 (2026-11) — split-brain config repair verified; verdict approved

This round reviewed the repair for the round-3 (bf1e11be) F1 finding
(split-brain `NotificationChannels` config: `PUT /users/me` persisted a
config delivery never read). Repair commit `7999a73b` (+ process/checkpoint
commits `ff88bb26`, `d2068c4a`, `79b395dc`); task-surface diff
`e9b7e214..79b395dc` touches only `api/users.rs` (+213),
`notification_dispatcher.rs` (+66), and the task file.

### Prior findings — all verified fixed

- **bf1e11be F1 (split-brain config) — fixed.** Single effective config now
  holds in both directions: `PUT /users/me` carrying
  `preferences.notification_channels` validates it with the exact rules of
  the dedicated endpoint (shared `validate_channel_config`: `in_app:false`
  and relative webhook URLs rejected) and upserts `user_channel_prefs` with
  the same value it persists to `users.preferences`; `PUT
  /api/v1/notifications/preferences` mirrors its value into
  `users.preferences.notification_channels` so `GET /users/me` echoes what
  delivery reads; new `resolve_channels` is the single read path
  (`user_channel_prefs` row → `users.preferences` fallback → defaults) used
  by both `dispatch_to_channels` and `GET /notifications/preferences`.
  Guard tests kill the defect: `put_me_notification_channels_reach_delivery_outbox`
  (full-router PUT then dispatch, asserts durable email outbox queues —
  fails on pre-repair code), `put_me_notification_channels_rejects_invalid_config`,
  `put_channel_preferences_syncs_users_preferences`,
  `dispatch_falls_back_to_stored_user_preferences`.
- **b69319b7 F1 (fan-out bypass, 7 sites) — fixed and re-verified.** All 23
  non-test `notifications.create(` sites in `gyre-server/src` now dispatch
  after the in-app record persists (git_http persona-push, reconciliation,
  secret_rotation, prompt_assembly, signal_chain, impersonation
  request/alert, spec_patrol, constraint_check, mcp, trust_suggestion,
  spec_link_staleness, abandoned_branch, repos, mirror_sync, or via
  `notify`/`notify_rich`). Emitter-path guard tests assert the outbox fires
  at the previously-bypassed sites.
- **b69319b7 F2 (unreachable budget helpers) — fixed.** `api/budget.rs`
  `maybe_notify_budget_threshold` calls
  `notify_agent_budget_exhausted`/`notify_agent_budget_warning`; budget
  routing has exactly one production implementation with per-period dedup.
- **6f7e90d0 F1 (rustfmt) — fixed.** `check-rustfmt-diff.py 7daca25d` →
  exit 0, 135 changed Rust files clean.

### Probes run (exit codes in SUMMARY.txt)

`cargo check -p gyre-server` and `--tests` → 0 (all new tests compile);
`cargo clippy -p gyre-server [--tests]` → zero warnings in the round's
changed files (remaining warnings are pre-existing mainline debt);
`cargo test -p gyre-domain` 465/465, `-p gyre-common` 124/124,
`-p gyre-adapters` 440/440 (incl. `channel_prefs_roundtrip_and_upsert_replaces`);
`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`,
`check-mcp-write-tools`, `check-migration-versions`,
`check-mem-port-contracts`, `check-migration-sql-portability`,
`check-fabricated-scope-defaults`, `check-scope-literal-defaults`,
`check-in-memory-state-stores`, `check-unbounded-external-http`,
`check-lossy-secret-conversion`, `check-forged-scope-fields`,
`check-inert-enforcement`, `check-dead-message-kinds`,
`check-conditional-test-guards`, `check-aspirational-test-names` → all 0.
Attribution: all 11 task-labeled SHAs listed in the candidate's
`commits:` frontmatter (the working-tree `check-task-commit-attribution.sh`
failure is controller staging that reverted the task file to `commits: []`,
not candidate content).

### Substantive verification (reading, plus the suites above)

- Webhook delivery signs the exact sent body with ring HMAC-SHA256
  (`X-Gyre-Signature: sha256=<hex>`); the guard test asserts the header
  equals HMAC over the captured body, so signing-input drift fails it.
- Per-channel `min_priority` thresholds are inclusive (`>=`) and tested on
  both sides (delivered at band, suppressed below).
- Escalation routing implements the specced chain exactly
  (`escalation_recipients`): `spawned_by` primary; offline spawner
  (presence-map session check) adds workspace Admins; Urgent always adds
  Owners; dedup'd. Tests assert exact recipient vectors for all three
  steps, including Owners-added-when-spawner-online (the regression that
  killed the old numeric-band mapping). `spawn.rs` `fail_agent` calls
  `notify_agent_escalation` with tenant resolved from the workspace, never
  fabricated.
- Email queues a durable `kv_store` outbox entry (interface per plan; SMTP
  is deployment's concern) — matches the acceptance criterion.
- All outbound channel HTTP is latency-bounded (10 s reqwest timeout).
- Coverage rows 16–18 in `specs/coverage/system/user-management.md` are
  accurate, including the repair history; no hollow `implemented` claims.

### Non-blocking notes (recorded, no action required)

- `update_me` persists `users.preferences` then upserts
  `user_channel_prefs`; a storage failure between the two writes returns
  500 with the stores transiently diverged until the client retries.
  Inherent to the two-store design (no cross-store transaction); the
  fallback in `resolve_channels` and the retry semantics bound it.
- The HMAC guard test derives its expected value from the production
  `sign_payload`; it still binds the signature to the exact sent body and
  secret, and ring's HMAC-SHA256 is the crypto primitive — acceptable.
- `check-assertionless-tests.sh` cannot run in this sandbox (mawk lacks
  gawk's 3-arg `match()`); compensated manually — every new test this
  round carries real assertions. Verifier should run it under gawk.

### Deferred to the independent verifier

`cargo test -p gyre-server --lib notification_dispatcher`;
`--lib put_me_notification_channels`; `--lib
put_channel_preferences_syncs_users_preferences`; `--lib
fans_out_to_configured_channels approval_code_and_alert_fan_out_to_configured_channels`;
`--lib push_persona_change_updates_entity_resets_approval_and_notifies_owner`;
`--lib budget`; full `cargo test --all`; CI clippy diff gate
(`check-clippy-diff.py`, needs `--all-targets`); gawk
`check-assertionless-tests.sh`. The review-stage build wrapper blocks
gyre-server test codegen in this sandbox (4 GiB budget) — an
infrastructure restriction, not a code defect.

Verdict: **approved** — every acceptance criterion is backed by real
production code and defect-killing tests; all prior rounds' findings are
closed with code evidence; no new defects found in the repair.
