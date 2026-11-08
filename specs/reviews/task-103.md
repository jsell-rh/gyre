# Review — task-103 (Add agent lifecycle audit event types)

Spec: `observability.md` §Audit Event Types (7 agent lifecycle events).
Commits under review (candidate `3fe84517a`, base `98a5299b9`): `3b9d312fc5`
(product), `ad99cb8c9` (formatting/clippy repair), `cead5a881` (exemption-freeze
repair). HEAD also carries process-only commits (`9c0b3b544`, `3fe84517a`) —
no product surface.
Verdict: **approved**.

## Probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain --lib` → **432 passed, 0 failed** (includes
  `audit_event_type_round_trip` covering all 7 new variants).
- `cargo test -p gyre-server --lib` → **1428 passed, 0 failed** (full lib
  suite, warm build; covers auth/admin/spawn/agents/stale_agents/siem).
- Focused lifecycle filters → 10/10 passed:
  `spawn_emits_agent_spawned_audit_event`,
  `complete_emits_agent_completed_and_token_revoked_audit_events`,
  `admin_kill_emits_lifecycle_audit_events`,
  `admin_reassign_emits_agent_reassigned_audit_event`,
  `heartbeat_emits_agent_heartbeat_audit_event`,
  `heartbeat_audit_reports_attested_pid_liveness`,
  `stale_detector_emits_agent_dead_audit_event`,
  `stale_detector_reports_pid_liveness_in_agent_dead_event`,
  `minted_jwt_carries_unique_jti`, `cef_agent_lifecycle_severities`.
- `cargo test -p gyre-server --lib auth::` → **44 passed, 0 failed**, including
  all JWT tenant-resolution tests pinning the `cead5a881` repair as
  behavior-preserving.
- `bash scripts/check-fabricated-scope-defaults.sh` → OK (2 entries ≤ frozen 3;
  base had 3 entries/frozen 4 — entry removed and count lowered, not grown).
- `bash scripts/check-scope-literal-defaults.sh` → OK.
- `bash scripts/check-in-memory-state-stores.sh` → OK.
- `bash scripts/check-task-commit-attribution.sh` → OK.
- `python3 scripts/check-rustfmt-diff.py 98a5299b9` → changed lines clean
  (9 Rust files).
- `python3 scripts/check-clippy-diff.py 98a5299b9` → **deferred to
  verification**: the script needs a workspace-wide clippy build, which the
  executor wrapper reserves for the verification stage. Prior round's
  retained evidence (`ad99cb8c9` repair) shows changed lines clean.

## Verified against acceptance criteria

- **Enum + round-trip**: all 7 variants in `AuditEventType` with snake_case
  `as_str()`/`from_str()` (consistent with the pre-existing event vocabulary;
  the CamelCase-vs-snake_case question was adjudicated cross-cutting in the
  task-102 review). Domain round-trip test covers them.
- **AgentSpawned**: emitted in `spawn_agent_core` (spawn.rs:1203) with
  `name`/`task_id`/`compute_target` (label resolved from the compute-target
  entity, spawn.rs:647)/`branch`/`spawned_by` (authenticated caller). Test
  asserts each field exactly against the audit port.
- **AgentCompleted**: emitted after MR creation + task/agent transitions with
  `task_id`/`mr_id`/`branch`; `current_task_id` is still populated at emit
  time (verified handler ordering). Idempotent 202 re-complete does not
  re-emit — correct, the completion was already audited.
- **AgentKilled**: admin kill endpoint; `killed_by` from the
  `AuthenticatedAgent` extractor (`user_id`, falling back to `agent_id`);
  `pid` from the process-registry handle or workload attestation, null when
  unknown (never fabricated). Kill now also revokes the token, closing the
  spec's "revoked on … admin action" trigger (docs updated).
- **AgentDead**: stale detector `Abort` branch — the only branch that marks an
  agent Dead (verified against `DisconnectedBehavior` arms); `last_heartbeat_at`
  from the stored heartbeat, `pid_alive` measured via `/proc`.
- **AgentHeartbeat**: heartbeat handler has no early returns before the emit;
  `pid`/`pid_alive` from the attestation liveness check already performed.
  The quality test proves liveness is measured (live test-process PID → true,
  exited child PID → false), not assumed.
- **AgentTokenRevoked**: all agent-lifecycle revocation sites (agent complete,
  operator stop, admin kill) route through `agent_audit::revoke_agent_token`,
  which reads the token before deleting it from `agent_tokens` and attributes
  the minted JWT's `jti`; the test asserts the audited jti equals the jti in
  the actually-minted token and that the token is really deleted. Minted JWTs
  carry unique `jti` claims (`minted_jwt_carries_unique_jti` also checks the
  signature-validated claims match). `extract_jti_unverified` is audit-detail
  only and operates on tokens read from the server's own KV — no auth path
  uses it.
- **AgentReassigned**: one event per moved task with `old_task_id`/
  `new_task_id`/`reassigned_by`. Task identity is stable across reassignment
  in this product (the reassign endpoint moves the assignee), so both task-id
  fields carry the true task id and the actual move is recorded as
  `from_agent_id`/`to_agent_id` — disclosed in the shipped notes; all specced
  field names are present with real values.
- **CEF severities**: kill 7, dead 6, revoke 5, reassign 4,
  spawned/completed/heartbeat 3, asserted by parsing real `format_cef` output.
- Tests are real: each drives the actual HTTP handler (oneshot) or detector
  loop (`run_once`) and asserts exact event counts and detail fields via the
  `AuditRepository` port query — removing an emitter yields count 0 and fails.
- Coverage matrix updated honestly (section 5 remains task-assigned with
  16/34; task-104 remainder identified). Task frontmatter lists all three
  product commits; attribution check passes.

## Non-blocking observations

- Token removals outside the agent lifecycle are not audited as
  `AgentTokenRevoked`: spawn-failure rollback paths (spawn.rs:511, :616)
  delete never-issued tokens on failed spawns, and `gate_executor`'s
  `revoke_gate_token` cleans up ephemeral internal `gate-review-*` credentials
  that have no `Agent` entity and never participate in the spawn/complete/
  heartbeat lifecycle the spec's event table tracks. Out of the spec's
  "revoked on complete or admin action" trigger.
- In the heartbeat container path, `pid_alive` carries the container
  liveness result when a `container_id` is attested (documented at the call
  site); the PID-liveness semantics are exact on the non-container path that
  the quality test exercises.
