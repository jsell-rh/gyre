# Review — task-100 (Platform Model Secrets Rotation Background Job)

Spec: `platform-model.md` §7 "Secrets Delivery" (rotation check job, agent-teardown revocation, `secret.accessed`/`secret.rotated`/`secret.revoked` audit events).
Candidate under review (round 3): `dc626153b4378c2760d2b788c2e3a0e746d2da98` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`; task commits `a42d006b8`, `c7f6d13d5`, `afd5ab18b` + docs/process commits).

## Rounds 1–2 (prior, from durable findings)

- Round 1 F1: REST `complete_agent` never revoked task-scoped secrets — fixed in `c7f6d13d5` (verified this round: call present at `spawn.rs:1432`, test `complete_agent_revokes_task_scoped_secrets` passes).
- Round 2 F1/F2/F3: `stop_agent`, `fail_agent`, `reject_spec` cascaded shutdown never revoked — fixed in `afd5ab18b` (verified this round: calls at `spawn.rs:1731`, `spawn.rs:1661`, `specs.rs:1111`; tests pass; negative control below proves the stop test is bug-killing).

## Round 3

Probes (all on the candidate tree, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`; evidence: `/tmp/stage/review-evidence/task-100-round3.md`):

- `cargo test -p gyre-server --lib secret_rotation::` → **8 passed** (handler rotation success/failure/malformed, revoke-only-that-task, skip-unresolvable-workspace).
- `cargo test -p gyre-server --lib -- revoke` → **24 passed** (all teardown-revocation tests: MCP complete, REST complete/stop/fail, admin kill, spec-reject, revoke unit tests).
- `cargo test -p gyre-server --lib` → **1495 passed, 0 failed** — matches the shipped claim exactly.
- Negative control: with the `stop_agent` revoke call temporarily commented out, `stop_agent_revokes_task_scoped_secrets` **fails** ("REST agent stop must revoke the task-scoped secret") — the round-2 tests are bug-killing, not self-confirming. Tree restored, verified clean.
- Mechanical checks: `check-arch`, `check-unbounded-external-http`, `check-lossy-secret-conversion`, `check-fabricated-scope-defaults`, `check-task-commit-attribution` all exit 0.

Verified working (no findings): the `SecretRotationHandler` trait + `OAuthRefreshHandler` performs a real RFC 6749 §6 `refresh_token` grant over the latency-bounded AppState client and stores the rotated document with extended expiry (test asserts the actual grant form posted to a live local endpoint); failed/malformed rotation falls back to the priority-7 admin notification without corrupting the stored value; `revoke_task_secrets` deletes only the Task scope, resolves the tenant from the workspace record and skips (warn) on unresolvable workspace; audit events carry secret name, never value; all round-1/round-2 fix call sites are present and tested.

Verdict: **needs-revision**.

Findings:

- **F1 (major, missing enforcement — same class as rounds 1–2): the stale-agent `DisconnectedBehavior::Pause` branch terminally stops an agent without revoking its task-scoped secrets — and `Pause` is the default.** `stale_agents.rs:124-141`: on heartbeat timeout, an agent with `disconnected_behavior = Pause` is transitioned to `Stopped` — terminal, no exit transitions (`gyre-domain/src/agent.rs:179-200`) — with no `revoke_agent_task_secrets` call, while the sibling `Abort` arm in the same loop (`stale_agents.rs:84`) does call it. `Pause` is the enum default (`agent.rs:16-19`, `#[default]`), and spawn requests set the field explicitly (`spawn.rs:454-455`), so the *default* configuration of every spawned agent that goes heartbeat-silent hits this branch. Reproduced with a temporary probe test (seeded Active agent with a current task and a Task-scoped Ephemeral secret, stale heartbeat, `Pause`; `stale_agents::run_once` → agent `Stopped`, secret still present). The leaked secret is never recovered: `secret_rotation::run_once` scans only Tenant/Workspace/Repo scopes, never Task scope. Contradicts platform-model.md §7 teardown, the acceptance criterion "Task-scoped secrets revoked on agent completion/kill", and the shipped claim that revocation "is called from every path that ends an agent run". Fix: call `revoke_agent_task_secrets` in the `Pause` arm after the status update, with a test mirroring the `Abort`-arm coverage.
- **F2 (moderate, missing enforcement — same class): `PUT /api/v1/agents/{id}/status` (`update_agent_status`, `agents.rs:265`, route `api/mod.rs:238`, documented in `docs/api-reference.md:126` with `Dead`/`Failed`/`Paused` variants) ends an agent's run via a terminal transition without revoking task-scoped secrets.** The handler does `transition_status` + update + event only; the dedicated `stop`/`fail`/`kill` endpoints revoke, this generic status endpoint does not. Reproduced with a temporary probe test (Active agent with current task + Task-scoped secret; `PUT /status {"status":"dead"}` → 200, agent `Dead`, secret still present; nothing ever recovers it). Fix: when the parsed new status is terminal (`Dead`/`Failed`/`Stopped`), call `revoke_agent_task_secrets` alongside the update, with a REST-path test.

Not findings (checked and defensible): task-cancellation via `transition_task_status` (`tasks.rs:315`) does not stop the agent — the run continues and ends via a covered path, so revocation correctly waits for the run-end; spawn-failure rollback (`spawn.rs:502/607`) deletes a never-started agent whose task may be retried with the same secret; `secret.accessed` is logged at the only production `resolve_for_agent` call site (spawn), matching the task plan's wording.

— Reviewer, round 3
