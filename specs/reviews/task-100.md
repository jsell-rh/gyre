# Review — task-100 (Platform Model Secrets Rotation Background Job)

Spec: `platform-model.md` §7 "Secrets Delivery" (Rotated secret lifecycle, teardown revocation, audit events).
Candidate under review: `69020fa26287b4e4e3dad9294168e530e58e8127` (base `4e895f766`, commits `a42d006b8` + `c7f6d13d5` + docs/process commits).
Verdict: **needs-revision** (round 2).

## Round 1 (prior review, durable finding)

F1 (major): REST completion path `POST /api/v1/agents/{id}/complete` (`complete_agent`) performed sibling teardown steps but never revoked task-scoped secrets; same class on stale-agent abort, compose teardown, repo-archive stop, and the spawn process/container/SSH exit monitors. Owned by this task's revision round.

## Round 2 (this review)

### Round-1 F1 repair verified ✅

- `complete_agent` (spawn.rs:1425-1433) now calls `revoke_agent_task_secrets` after token revocation and interrogation-policy cleanup, before the jj bookmark step; `agent.current_task_id` is still set at that point (task transition to Review happens earlier, `current_task_id` not cleared).
- All kill-adjacent paths named in round-1 F1 now revoke: MCP `gyre_agent_complete` (mcp.rs:1446-1455, after the status update succeeds), admin kill (admin.rs:357-361), stale-agent heartbeat abort (stale_agents.rs:81-85), compose teardown (compose.rs:216-220), repo-archive stop (repos.rs:311-315), process/container/SSH exit monitors (spawn.rs:966-972, 1083-1089, 1143-1149).
- Mutation probe (evidence: /tmp/stage/review-evidence/mutation-probe-complete-agent.md): commenting out the single `revoke_agent_task_secrets` call in `complete_agent` makes `api::spawn::tests::complete_agent_revokes_task_scoped_secrets` FAIL — the round-1 repair test is bug-killing, not self-confirming. Source restored; tree clean.

### New functionality verified ✅ (no findings)

- **Rotation handler.** `SecretRotationHandler` trait + `OAuthRefreshHandler` (secret_rotation.rs): real RFC 6749 §6 refresh grant over the AppState reqwest client (built with a 30 s total timeout, lib.rs:914-916 — latency-bounded per the repo's outbound-HTTP invariant); rotated document re-stored via `SecretRepository::rotate` (value + `last_rotated_at`), expiry extended by `expires_in`, metadata rewritten with the same delete+create pattern the task-098 Admin API uses (`rewrite_secret`, api/secrets.rs:425-441). Failure/malformed-value/no-handler falls back to the priority-7 `SecretExpiring` admin notification. `secret_rotation_check` job registered at 300 s interval (jobs.rs:526-540) and spawned in main.rs:78-82.
- **Tests.** All 8 `secret_rotation` tests pass on the candidate, including `rotation_handler_refreshes_expiring_oauth_secret` (asserts the actual form body posted to a real local token endpoint — grant_type, stored refresh token, client_id — plus the rotated stored document, extended expiry, `last_rotated_at`, and notification suppression) and the failure/malformed fallbacks. The three teardown-path tests (mcp complete, admin kill, REST complete) pass and assert repository state (deletion + survival of workspace/other-task secrets), not handler echoes. Focused runs via /tmp/stage/bin/cargo; evidence: /tmp/stage/review-evidence/focused-tests.md.
- **Audit events.** `secret.accessed` in the production spawn path (spawn.rs:739-753, inside the `Ok(resolved)` arm of `resolve_for_agent`, before `GYRE_CRED_*` injection); `secret.rotated` on handler rotation and admin force-rotate (api/secrets.rs:286-296); `secret.revoked` per deleted secret in `revoke_task_secrets`. All tracing macros with `event` field, secret name only — never the value.
- **Scope safety.** `revoke_task_secrets` resolves the tenant from the agent's workspace record and skips with a warn on unresolvable workspace (no fabricated scope; tested by `revoke_task_secrets_skips_unresolvable_workspace`). No mem/SQLite divergence introduced (no port changes).

### Findings (revision required)

- [ ] **F1 (major): `POST /api/v1/agents/:id/stop` (`stop_agent`, spawn.rs:1693) leaks the task-scoped secret.** The handler terminally ends the run (Stopped has no exit transitions; it revokes the agent token "so it can no longer authenticate" — the same teardown steps as the kill handler) but never calls `revoke_agent_task_secrets`. Probe on the candidate (evidence: /tmp/stage/review-evidence/stop-fail-task-secret-leak-probe.md): spawn → seed Task-scoped Ephemeral secret → POST `/agents/{id}/stop` → 200, agent Stopped, secret still present. The leak is permanent: `secret_rotation::run_once` scans only Tenant/Workspace/Repo scopes, never Task scope. Contradicts §7 ("Agent completes or is killed → Secrets access revoked") and the shipped-notes claim that revocation runs on "every path that ends an agent run". Fix: revoke alongside `revoke_agent_token` in `stop_agent`, with a REST-path test.
- [ ] **F2 (major): `POST /api/v1/agents/:id/fail` (`fail_agent`, spawn.rs:1632) leaks the task-scoped secret.** Failed is a terminal run-end (agent-runtime.md §1: "max iterations or spawn failure"); same probe shows 200, agent Failed, secret still present. Same permanent-leak shape as F1. Fix: revoke after the status update, with a REST-path test.
- [ ] **F3 (moderate): `reject_spec` cascaded shutdown (specs.rs:1078-1106) leaks the cancelled tasks' secrets.** Cancels in-flight spec tasks and transitions their Active agents to Stopped without revocation — the identical cancel+stop shape the candidate covers at repos.rs:311-315 (repo-archive) with the rationale "the agents' tasks are cancelled, so their one-time secrets must not outlive the task". Internally inconsistent; same fix shape.

Note (not counted): `stale_agents.rs:127` (DisconnectedBehavior::Pause → Stopped) was inspected and deliberately not flagged — Pause semantics are "wait for reconnection" (gyre-domain agent.rs:17-19), so skipping revocation there is defensible. If F1's fix makes Stopped definitively terminal-for-secrets, the pause path deserves a deliberate decision (revoke or document) rather than silence.

### Non-blocking observation

`attempt_rotation`'s metadata rewrite (rotate value → delete row → create row with extended `expires_at`) is non-atomic: a `create` failure after `delete` succeeds loses the secret row entirely, including the freshly rotated credential whose predecessor the provider has already invalidated. It mirrors the established task-098 Admin API `rewrite_secret` pattern and the port offers no metadata-update method, and a failure still falls back to an admin notification — recorded for a future port-level `set_expires_at`/update method, not a revision blocker.

### Probes run (all on candidate 69020fa26, tree clean before and after; temporary edits restored)

- `cargo test -p gyre-server --lib secret_rotation` → 8 passed.
- `cargo test -p gyre-server --lib agent_complete_revokes_task_scoped_secrets` / `admin_kill_revokes_task_scoped_secrets` / `complete_agent_revokes_task_scoped_secrets` → 1 passed each.
- Mutation probe on `complete_agent` revocation → test fails with the call disabled (bug-killing).
- Stop/fail leak probe → both endpoints leak the task-scoped secret (evidence saved).
- Static: job registration (jobs.rs:526-540, main.rs:78-82), HTTP client 30 s timeout (lib.rs:914-916), audit-event placement in production paths.

Summary: the round-1 repair is genuine and bug-killing, the rotation handler is a real refresh grant with real tests, and the covered teardown paths are correct — but two documented REST endpoints (`/agents/:id/stop`, `/agents/:id/fail`) and the spec-reject cascaded shutdown still terminally end agent runs without revoking task-scoped ephemeral secrets, the exact defect class round 1 established as major. Revision required on F1–F3.

— Reviewer, round 2
