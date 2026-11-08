# Review — task-103 (Add agent lifecycle audit event types)

Spec: `observability.md` §Audit Event Types (agent lifecycle table, 7 events).
Candidate: `edf0637b9` (base `72ad2a128`); product commits `3b9d312fc`
(implementation) and `ad99cb8c9` (rustfmt/clippy repair, formatting + one
borrow fix, no behavior change). Both are recorded in the task's `commits:`
frontmatter; `check-task-commit-attribution.sh` passes.
Verdict: **approved**.

## Probes (all on the assigned checkout at `edf0637b9`; evidence in
`/tmp/stage/review-evidence/`)

- `cargo test -p gyre-domain --lib audit` → **8 passed, 0 failed**
  (`audit_event_type_round_trip` covers all 7 new variants).
- `cargo test -p gyre-server --lib --` filtered to the lifecycle tests
  (spawn/complete/admin-kill/admin-reassign/heartbeat/stale-detector emission,
  pid-liveness quality, jti uniqueness, CEF severities) → **11 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- admin_kill admin_reassign
  kill_agent stale_detector_restarts` → **10 passed, 0 failed** — the
  `AuthenticatedAgent` extractor added to the admin kill/reassign handlers
  breaks no pre-existing caller (all send Bearer tokens; the ABAC middleware
  still owns the Admin-role gate, e.g. `developer_cannot_access_admin_kill_or_tunnel`).
- **Mutation probe** (restored immediately; tree verified clean at
  `edf0637b9` afterwards): disabling the `emit_heartbeat` call in the
  heartbeat handler makes both heartbeat audit tests fail (`exactly one
  agent_heartbeat event: left 0, right 1` / missing event). The tests bind to
  real production emission, not stubs.
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK (no routes added or changed).
- `bash scripts/check-abac-exempt-handlers.sh` → OK (88 handlers).
- `bash scripts/check-dead-message-kinds.sh`,
  `check-in-memory-state-stores.sh`, `check-lossy-secret-conversion.sh`,
  `check-forged-scope-fields.sh`, `check-scope-literal-defaults.sh`,
  `check-inert-enforcement.sh` → all OK.
- `python3 scripts/check-rustfmt-diff.py 72ad2a128` → "changed lines clean
  (9 Rust files checked)" — the round-1 verification finding
  (`2b5e29b31c5f482d9b530bdc8b37c611`) is genuinely repaired.
- Focused `cargo clippy -p gyre-server -p gyre-domain`: the only warning
  located in a touched file is `gyre-domain/src/audit.rs:133`
  (`AuditEvent::new`, 14 args) — pre-existing from task-102, outside this
  diff's hunks. No new warnings on changed lines. (Workspace clippy-diff and
  full suites belong to the verification stage; the wrapper rejects
  workspace-wide invocations, recorded in `clippy-diff.txt`.)

## Verified against the task contract

- **All 7 variants** (`AgentSpawned/Completed/Killed/Dead/Heartbeat/
  TokenRevoked/Reassigned`) exist in `AuditEventType` with snake_case
  `as_str()`/`from_str()` round-trip; the SQLite adapter stores/reads
  `event_type` through exactly these strings (`sqlite/audit.rs:39,86`), so
  the round-trip holds through persistence, not just in-memory.
- **Emission points are production code paths, not test shims:**
  - `AgentSpawned` in `spawn_agent_core` (after all failure paths, before
    `Ok`) — covers both the HTTP route and the MCP spawn tool, which both
    route through the core; detail: name/task_id/compute_target(resolved
    label)/branch/spawned_by.
  - `AgentCompleted` in the complete handler after MR creation + state
    transitions (task_id/mr_id/branch); the idempotent 202 path correctly
    does not re-emit.
  - `AgentKilled` in the admin kill endpoint (reason/killed_by from the
    authenticated caller/pid from the process-registry handle or workload
    attestation, null when unknown — never fabricated).
  - `AgentDead` in the stale detector's `Abort` branch — the only branch
    that transitions to Dead (Pause → Stopped, ContinueOffline leaves
    running), so the spec trigger is fully covered; detail:
    last_heartbeat_at + pid_alive measured via `/proc`.
  - `AgentHeartbeat` in the heartbeat handler, emitted on both the
    attestation-ok and attestation-failed paths (handler only warns);
    pid/pid_alive from the liveness check already performed. The quality
    test proves the value is measured: live test-process PID → true, exited
    child PID → false.
  - `AgentTokenRevoked` at every revocation site that matches the spec
    trigger (agent complete, operator stop, admin kill) via the shared
    `revoke_agent_token`, which reads the token before deleting it and
    attributes the real `jti` (test asserts the audited jti equals the jti
    embedded in the minted token). Minted JWTs now carry unique `jti`
    claims; `extract_jti_unverified` is audit-detail-only and marked as
    such — all auth paths still go through full `validate()`.
  - `AgentReassigned` in the admin reassign handler, one event per moved
    task, with `reassigned_by` from the authenticated caller.
- **CEF severities** (`siem.rs`): kill 7, dead 6, revoke 5, reassign 4,
  spawned/completed/heartbeat 3 — exactly the task plan; tested by parsing
  the severity field out of real `format_cef` output.
- **Enforcement/persistence is real:** emitters record through the
  `AuditRepository` port (asserted via `state.audit.query` in tests, i.e.
  the same port production queries use) and broadcast the full envelope on
  `audit_broadcast_tx` (same pattern as `procfs_monitor.rs`). The admin-kill
  test also asserts the token is actually deleted from `agent_tokens`.
- **Coverage matrix honestly updated**: row 5 remains `task-assigned`
  (16/34, remaining container/file-network/source-control/access-control
  events owned by task-104) — no premature `verified` claim.
- Admin kill now also revokes the killed agent's token (a real hardening,
  documented in `docs/api-reference.md`).

## Non-blocking notes

- `AgentReassigned` records `old_task_id == new_task_id` because task ids
  are stable across reassignment in Gyre (only the assignee changes); the
  assignee move is recorded alongside as `from_agent_id`/`to_agent_id`.
  Honest representation of the spec's fields given the platform model —
  fabricating a differing "new" id would be worse.
- `gate_executor.rs::revoke_gate_token` removes internal gate-agent tokens
  without an `AgentTokenRevoked` event; gate-executor credentials are
  ephemeral internal machinery, not agent-lifecycle tokens, and the spec
  trigger is "revoked on complete or admin action". Out of scope here.
- `agent_audit::emit` swallows audit-record errors (best-effort, documented
  in the module header) — consistent with the established broadcast
  pattern; persistence itself is real and test-asserted.
- Lifecycle events leave the envelope's `user_id` null for admin actions
  (the admin identity lives in the `killed_by`/`reassigned_by` detail
  fields the spec table actually requires). Cosmetic, not a contract gap.

Full workspace suites and all-target clippy remain with the verification
stage, per review scope.
