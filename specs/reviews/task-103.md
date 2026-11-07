# Review — task-103 (Add agent lifecycle audit event types)

Spec: `observability.md` §Audit Event Types (7 agent lifecycle events).
Commits under review: `3b9d312fc` (implementation, listed in `commits:`), plus `5ecbecb97`, `f2eab3807`, `b8f323bfb`, `45cd873bb` (task-doc/coverage/workflow only). Base `a884329d7`, candidate `45cd873bb`.
Verdict: **approved**.

## Round 1

### Probes (evidence: `/tmp/stage/review-evidence/task-103-*.txt`)

- `cargo test -p gyre-server --lib -- <10 targeted tests>` → **10 passed, 0 failed** — all 7 emission-point tests, `minted_jwt_carries_unique_jti`, `cef_agent_lifecycle_severities`, `heartbeat_audit_reports_attested_pid_liveness`.
- `cargo test -p gyre-domain --lib audit` → **8 passed, 0 failed** (incl. `audit_event_type_round_trip` over all 7 new variants).
- `cargo test -p gyre-server --lib` (full crate) → **1348 passed, 0 failed** — no regressions from the `AuthenticatedAgent` extractor added to admin kill/reassign.
- `cargo test -p gyre-adapters --lib audit` → **11 passed, 0 failed** — new event_type strings persist/query through the SQLite adapter.
- **Mutation probe:** disabling the `emit_heartbeat` production call made both heartbeat audit tests **fail** (0 passed, 2 failed); file restored (tree clean), test passes again. The tests are tied to the production emission, not self-confirming.
- Mechanical checks all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-byte-slice-truncation.sh`, `check-forwarded-header-trust.sh`, `check-task-commit-attribution.sh`.

### Verified

- **Enum + round-trip:** all 7 variants in `AuditEventType` with snake_case `as_str()`/`from_str()`, matching the pre-existing 9-type vocabulary; `#[serde(rename_all = "snake_case")]` keeps wire/DB/query consistent (adapter tests cover persistence).
- **Emission points are real production paths** through the `AuditRepository` port + `audit_broadcast_tx` SSE broadcast (`agent_audit.rs`, same pattern as M23 container events):
  - `AgentSpawned` in `spawn_agent_core` post-success; all 5 detail fields are the actual spawn values (`spawned_by` = caller identity).
  - `AgentCompleted` in `complete_agent` after MR creation; the 202 idempotent re-complete path returns before emission (no double-audit).
  - `AgentKilled` in `admin_kill_agent`; `killed_by` from `AuthenticatedAgent` (user_id, agent_id fallback); `pid` from the process-registry handle or workload attestation, null when unknown — never fabricated. Admin kill now also **actually revokes** the agent's token (enforcement added, not just audit; test asserts the token is really gone from `agent_tokens`).
  - `AgentDead` in the stale detector's `Abort` branch — the only branch that marks an agent Dead; `last_heartbeat_at` from the agent record; `pid_alive` measured via `/proc`.
  - `AgentHeartbeat` in the heartbeat handler, fed by the attestation liveness check already performed; liveness is **measured** (test: live PID → true, exited child PID → false), null when no attestation.
  - `AgentTokenRevoked` centralized in `revoke_agent_token` (complete, operator stop, admin kill); token read before deletion, `jti` extracted from the actual revoked token — the test asserts the audited jti equals the jti embedded in the minted token; legacy non-JWT tokens → null jti, never fabricated. `extract_jti_unverified` is audit-detail-only; all auth paths still use `validate()`.
  - `AgentReassigned` in `admin_reassign_agent`, one event per moved task, `reassigned_by` = authenticated caller.
- **CEF severities** exactly per the task plan (kill 7, dead 6, revoke 5, reassign 4, spawned/completed/heartbeat 3), test-asserted against the formatted CEF string.
- **Scope handling:** events carry the agent's real `workspace_id`; no `"default"` fabrication anywhere in the new code.
- **Task doc discipline:** `## Shipped` preserved exactly (### subheadings) per the durable finding; product commit `3b9d312f` listed in `commits:` and is an ancestor of the candidate; all four commits are task-labeled; attribution check passes.

### Non-blocking observations (documented interpretations, no action required)

- `AgentReassigned` emits `old_task_id == new_task_id` because task identity is stable across reassignment in this system; the actual move is recorded as `from_agent_id`/`to_agent_id`. Both specced fields are present with real looked-up values; fabricating a nonexistent "new task id" would be worse.
- Wire serialization of `event_type` is snake_case while the spec's SSE example shows CamelCase (`ContainerStarted`). This is pre-existing and cross-cuts the entire vocabulary (all 9 pre-existing types serialize snake_case); the task-102 review already deferred it, and the assigned task plan explicitly prescribes `as_str()`/`from_str()` round-trip. If the spec intends CamelCase wire strings, that is a separate spec-lifecycle amendment affecting all 16 types.
- Token removals outside the audited path are all outside the spec trigger set ("revoked on complete or admin action"): spawn-failure rollback deletes a token that was never delivered, and `revoke_gate_token` removes ephemeral synthetic `gate-review-*`/`gate-validate-*` runner credentials that have no agent lifecycle.
