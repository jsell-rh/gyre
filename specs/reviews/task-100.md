# Review — task-100 (Platform Model Secrets Rotation Background Job)

Spec: `platform-model.md` §7 "Secrets Delivery" (Rotated secret type, proactive
rotation, agent teardown revocation, secret access audit logging).
Commits under review: `a42d006b8` (rotation handler, teardown revocation, audit
events), `c7f6d13d5` (round-1 fix: REST completion + all teardown paths),
`afd5ab18b` (round-2 fixes: stop/fail/spec-reject), `abd7dab3b` (round-3 fixes:
stale-pause stop + terminal status update).
Candidate: `116ccb52350c04544c2f9b5c205f3ab7333bfc4e` (base `fefde2946`).
Verdict: **approved** (round 4; rounds 1-3 findings all fixed and re-verified).

## Round 4

Probes (full logs: `/tmp/stage/review-evidence/task-100-round4.md`):

- `cargo test -p gyre-server --lib secret_rotation::` → **8 passed, 0 failed**;
  `... --lib revoke` → **26 passed**; `... --lib task_scoped` → **9 passed** —
  all eight teardown-path tests (MCP complete, REST complete/stop/fail, admin
  kill, spec-reject, stale-pause, terminal status) each assert the task-scoped
  secret is deleted while workspace-scoped and other-task secrets survive.
- Negative controls (fix call site removed → test run → restored, tree verified
  clean): stale-pause revoke removal fails
  `stale_detector_pause_revokes_task_scoped_secrets`; terminal-status revoke
  removal fails `update_agent_status_terminal_revokes_task_scoped_secrets`;
  workspace-scope `try_rotate` removal fails
  `rotation_handler_refreshes_expiring_oauth_secret`. All three tests are
  bug-killing, not self-confirming.
- `cargo test -p gyre-server --lib` → **1497 passed, 0 failed**;
  `cargo test -p gyre-adapters secret` → **17 passed, 0 failed**.
- `cargo fmt -p gyre-server -- --check` clean; mechanical checks pass
  (check-arch, check-unbounded-external-http, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-task-commit-attribution).

Verified working (no findings):

- **Rotation job (AC 1-2):** `secret_rotation_check` registered at
  `interval_secs: 300` (jobs.rs:527-541) and spawned in main.rs:76-84 — the
  5-minute cadence is real. `run_once` scans tenant/workspace/repo scopes for
  `expires_at` within 1 h; each expiring secret with a handler
  (`OAuthRefreshHandler` for `*_refresh_token` names) gets a real RFC 6749 §6
  `refresh_token` grant over the AppState client (lib.rs:914-915, 30 s total
  timeout — latency-bounded). Success replaces the stored credential document,
  sets `last_rotated_at`, extends `expires_at` by the provider's `expires_in`
  (leaving the warning window), and suppresses the notification; failure,
  malformed value, or no handler falls back to the task-098 priority-7
  `SecretExpiring` admin notification. The rotation test drives a real local
  token endpoint and asserts the posted form carried the stored refresh token
  and client_id — real crypto-path behavior, not a structural check.
- **Teardown revocation (AC 3):** `revoke_agent_task_secrets` (resolves tenant
  from the agent's workspace record — unresolvable workspace skips with a warn,
  no fabricated scope) is called from every production path that terminally
  ends an agent run. Verified by exhaustive grep over
  `transition_status(AgentStatus::{Dead,Failed,Stopped})` sites: stale-agent
  detector both arms (abort + Pause), admin kill, REST complete/stop/fail,
  terminal PUT status, spec-reject cascaded shutdown, repo-archive stop,
  compose teardown, and the three spawn process/container/SSH exit monitors.
  The only `agents.delete` sites are spawn-failure rollbacks that run before
  `resolve_for_agent` ever injects secrets.
- **Audit events (AC 4):** `secret.accessed` at spawn injection (spawn.rs,
  name + scope + agent_id, never value), `secret.rotated` on background
  rotation and admin force-rotate (api/secrets.rs), `secret.revoked` per
  deleted task-scoped secret — all tracing macros with `event` field,
  constraint_check.rs pattern.
- **Tests (AC 5):** full lib suite green, and every teardown/rotation test was
  independently mutation-checked this round (three negative controls above).
- Coverage-matrix updates in specs/coverage/system/platform-model.md rows
  39/40/41/43 accurately describe the landed behavior; commit attribution
  frontmatter lists all four task commits (check passes).

Rounds 1-3 findings (durable record) — all resolved and re-verified by
mutation this round: round-1 F1 (REST complete), round-2 F1/F2/F3
(stop/fail/spec-reject), round-3 F1/F2 (stale-pause, terminal status).
