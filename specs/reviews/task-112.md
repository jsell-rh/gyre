# Review — task-112 (user-management.md §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations)

Candidate: `6de12de7bb7daaaf6e17468755cdca099c195ce6` on base `9bef21a41729a6b696b48a587b1f2e619d0f6ff2`
(checkpoint-squashed branch; product commit `7601f79a` + process commit `6de12de7`, both recorded in the task
frontmatter / attribution gate). Verdict: **approved**.

## Probes run (all on the exact candidate, evidence in /tmp/stage/review-evidence)

Note on infrastructure: the repo's `.cargo/config.toml` mandates the mold linker, which deadlocks in this
sandbox (ld.mold stuck in `pipe_read` at 0 CPU for 34 min). All Rust probes were run with
`RUSTFLAGS="-C link-arg=-fuse-ld=lld" CARGO_INCREMENTAL=0` (LLD 18.1.3, the linker the stage capabilities
note provides). This is an environment restriction, not a code defect.

- `cargo test -p gyre-server --lib notification_dispatcher` → **15 passed** (HMAC signing + per-channel
  threshold filtering, slack posting + channel override, email outbox durability + filtering, escalation
  chain incl. offline-spawner and Urgent fan-out, in-app per recipient, persona approval, merge-queue
  paused, budget warning/exhausted, security finding).
- `cargo test -p gyre-server --lib api::users` → **13 passed** (includes all four channel-prefs API tests:
  defaults, round-trip across a fresh router over the same state, rejected `in_app: false`, rejected
  relative webhook URL).
- `cargo test -p gyre-server --lib spawn::` → **34 passed** (fail_agent escalation wiring).
- `cargo test -p gyre-server --lib merge_processor::` → **45 passed** (includes the end-to-end
  post-merge-gate-failure → revert → author + workspace-Admin MrReverted fan-out assertion).
- `cargo test -p gyre-server --test graph_integration test_divergence_detection_creates_notifications`
  → **1 passed** — the prior-round regression probe (dropped `entity_ref`/`repo_id` in `check_divergence`)
  passes on this HEAD.
- `cargo test -p gyre-adapters --lib user_profile` → **5 passed** (SQLite channel-prefs round-trip +
  upsert-replaces semantics).
- `cargo test -p gyre-domain --lib user::` → **5 passed**.
- Frontend: `npm ci` then `npx vitest run src/__tests__/Inbox.test.js WorkspaceHome.test.js
  WorkspaceHomeArchitecture.test.js` → **55 passed**; `npx vite build` clean and the committed
  `web/dist` matches the fresh build byte-for-byte (git status clean after rebuild).
- Text gates all OK: arch, abac-route-registry, abac-exempt-handlers (90 handlers), byte-slice-truncation,
  dead-message-kinds, migration-versions, migration-sql-portability, mem-port-contracts, mcp-write-tools,
  unbounded-external-http, fabricated-scope-defaults (count lowered 5→4 by *fixing* the git_http.rs:1495
  site — the sanctioned direction), scope-literal-defaults, lossy-secret-conversion, in-memory-state-stores,
  forged-scope-fields, fail-open-ref-resolution, relative-path-defaults, forwarded-header-trust,
  inert-enforcement, task-commit-attribution.

Deferred to remote verification (infrastructure, not defects): the three gawk-only gates
(check-notification-priority / -audience / -test-coverage) cannot execute under this sandbox's mawk;
their priority table pins only pre-existing `ConstraintViolation:2`, untouched by this diff. Full
`cargo test --all` and all-target Clippy belong to the verification stage per the executor's wrapper.

## Verified working

- **Domain types** (`gyre-domain/src/user.rs`) match the §Delivery Channels spec excerpt exactly:
  `NotificationChannels{in_app, email, webhook, slack}`, `EmailConfig`, `WebhookConfig` (HMAC secret),
  `SlackConfig`, ordered `NotificationPriority` (Low..Urgent), `DigestFrequency` (Immediate/Hourly/Daily/Off
  — the spec's own enum). `Default` keeps `in_app: true`.
- **Dispatcher** (`notification_dispatcher.rs`): real ring HMAC-SHA256 over the exact sent body
  (`X-Gyre-Signature`, hex), real latency-bounded reqwest POST (10 s, satisfies
  check-unbounded-external-http), per-channel `min_priority` filtering, email queued durably in
  `kv_store` (`email_outbox` ns) — the task plan's stated minimum ("queue messages, SMTP can be stubbed").
  In-app delivery is the persist call itself (can't be disabled).
- **Escalation routing** matches §Notification Routing for Agent Escalations: `spawned_by` primary;
  offline (no presence-map entry, 60 s eviction window) adds workspace Admins; Urgent always adds Owners;
  dedup'd. Presence semantics verified against the janitor in lib.rs.
- **Routing table rows**: persona approval → owner (creator-owned, `user:` prefix stripped, scope resolved
  or skipped — never fabricated); merge queue paused → all Admins/Owners (wired into
  `pause_merge_queue`); MR reverted → author + workspace Admins (wired into `apply_revert_side_effects`,
  tenant resolved from workspace, `"default"` fabrication removed); spec approval → manifest
  `human_approvers` (owner fallback) in `save_spec`; spec drift → spec owner via ledger entry;
  invitation/gate-failure/MR-merged via existing paths now fanning out to channels. Budget
  warning/exhausted and security-finding helpers are implemented and tested but their emission triggers
  are honestly disclosed as owned by task-191 (exists, not-started, §Enforcement Behavior) and task-137
  (exists, not-started, §Gate Failure Feedback) — verified those task files claim exactly that scope.
- **Spec amendment** for the two team-lead rows is present in user-management.md (interim superset routing
  + dependency on Teams lead concept, task-122/125) — the sanctioned "amend the spec, don't reinterpret"
  path, not a silent contradiction.
- **Wiring**: `notify`/`notify_rich` fan out through `dispatch_to_channels`; every direct
  `notifications.create` call site (git_http, mcp, merge_processor, abandoned_branch, constraint_check,
  spec_link_staleness, specs_assist, personas, invite) was either converted to the helpers or given an
  explicit post-create fan-out; `DivergencePorts.on_created` carries fan-out into mirror clone/sync and
  push extraction (all three base-introduced construction sites wired, incl. the port-level tests passing
  `None` per documented contract).
- **Preferences API**: GET/PUT `/api/v1/notifications/preferences`, self-scope per-handler auth
  (`resolve_user_id(&auth)` — the only readable/writable entity is the caller's own row, same pattern as
  the exempted `/users/me/notifications` routes); ABAC-exempt entry in the resolver; PUT rejects
  `in_app: false` and non-absolute webhook/Slack URLs. Storage: migration `000057` (next unused sequence,
  portable SQL), SQLite + Postgres + mem adapters all enforcing replace-upsert; new state went through a
  port + `store!` wiring (satisfies check-in-memory-state-stores).
- **Incidental in-scope hardening**: three `mr_id[..8]` byte-slice truncations fixed to
  `chars().take(8)`; two `"default"` tenant fabrications removed (notify_mr_reverted, merge_processor
  revert side effect); the git_http.rs:1495 ws_id fallback exemption site actually fixed.

## Minor observations (recorded, not blocking)

- `NotificationType::PersonaApprovalRequested` doc comment says "Priority 3" while `default_priority()`
  returns 2 (2 aligns with sibling `SpecPendingApproval`; no spec-pinned value exists for this variant).
  Cosmetic doc-comment inconsistency only.
- The webhook-signature test computes its expected value with the same `sign_payload` helper (mirrored
  logic): it binds the header to the exact body+secret but would not catch an algorithm swap. The
  production function structurally uses ring's `HMAC_SHA256`, so the criterion is met in substance.
- `notify_security_finding` resolves "tenant Admin" as all platform users holding `UserRole::Admin`:
  no production code sets `user.tenant_id` anywhere in the codebase, so no closer concept exists; the
  choice is documented in the code comment. If users ever gain tenant bindings, this should be narrowed.
- `save_spec` reads the approvers manifest from repo HEAD, so a brand-new spec (manifest entry only on
  the feature branch) falls back to the "system" inbox rather than named approvers; modified specs
  resolve real recipients. Interim visibility-preserving fallback.
- Channel delivery is synchronous within request/notification paths (bounded at 10 s per call); the spec
  does not mandate background delivery, but a future async outbox would remove tail-latency coupling.

## Conclusion

Real production implementations for all three coverage sections; no stubs, no audit-only enforcement,
no fabricated scopes, no test inflation observed. Acceptance criteria verified by focused probes listed
above. Approved.
