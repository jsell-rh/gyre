# Review — task-268 (Pre-PR local verification: repo quality gates run in the agent environment before any MR is opened)

Spec: `specs/development/agent-workflow.md` §Immediate Feedback.
Assignment: base `e9f6a31c3903ff61597af88d0b70b41d94bb911a`, candidate `f59daaaa9400bd29a690b8ad8d4a38a114e72244`.
Product commits: `950a65a0` (implementation), `36dab5b4` (repair of review `4fba4dc6`); docs/process: `44112412`, `1701fb3d`, `f59daaaa`.

Verdict: **approved** — all four required behaviors implemented with real mechanical enforcement; both round-1 durable findings repaired with regression tests; no test inflation; no scope drift.

## Round 2 (repair verification, 2026-11-20)

### Prior durable findings — both closed

1. **Heartbeat starvation during gate execution (round-1 F1): fixed.** `agent-runner.mjs` extracts `sendHeartbeat()`/`maybeHeartbeat()` from the SDK message loop and spans the whole local-gate phase (gate subprocess runs and remediation queries) with a `setInterval` keep-alive at `HEARTBEAT_INTERVAL_MS` (default 15s, `<` stale_agents.rs's 60s timeout), cleared in a `finally`. New regression test `keeps heartbeating while local gates execute` observes heartbeats during a 1s gate run with `GYRE_HEARTBEAT_INTERVAL_MS=150` (the stub SDK resolves in milliseconds, so any heartbeat observed must come from the gate phase) and asserts completion still proceeds. Mutation probe (interval disabled → 24h no-op): exactly that test fails (1 failure), 10 pass.
2. **rustfmt (round-1 F2): fixed.** `python3 scripts/check-rustfmt-diff.py e9f6a31c3903ff61597af88d0b70b41d94bb911a` → exit 0 ("changed lines clean"). `36dab5b4`'s spawn.rs hunks are formatting-only (verified by diff inspection).

### Required behaviors — all four implemented

1. **Server-side gate delivery.** `spawn.rs` `launch_agent_process` container_env assembly (single env map used by `dispatch_agent_launch` for every compute-target type): resolves the repo's pre-merge `TestCommand`/`LintCommand` gates via `state.quality_gates.list_by_repo_id_and_phase(&repo.id, GatePhase::PreMerge)` — real `QualityGateRepository` rows through the port (default trait impl filters `list_by_repo_id`; mem + sqlite adapters provide it), never hardcoded commands — filters empty commands, and injects `GYRE_LOCAL_GATES` JSON `[{id, name, command, timeout_secs}]`; `[]` when the repo configures none. Resolution failure fails the launch closed (`ApiError::Internal`). PostMerge gates and non-command gates excluded by design (documented in-code).
2. **Runner-side local execution.** `runGateCommand` executes each command as a real shell subprocess in the gate work dir (`GYRE_WORK_DIR` when it exists — entrypoint cds to the clone — else process cwd) with a bounded timeout (gate `timeout_secs`, default 300s). Timeout kills the whole detached process group (SIGKILL `-pid`) and, to guarantee the bounded wait even if a grandchild holds the pipes, destroys the runner's stdio pipe ends and settles immediately with a timeout failure — never an indefinite hang. Exit status and combined output are captured.
3. **Failure feedback loop.** The only `POST /complete` call lives in `completeAgent()`, reachable solely after all gates pass (or none configured). On failure: no completion, no MR; `gateFailureFeedback` feeds the failing gate's name, exit status/timeout reason, and captured (truncated at 8k chars) output back to the agent as a real follow-up SDK query; gates re-run; bounded retries (`GYRE_LOCAL_GATE_MAX_RETRIES`, default 2); exhaustion → provenance upload + `process.exit(1)` with no completion. Malformed `GYRE_LOCAL_GATES` fails closed the same way. The error-path force-completion is removed: the catch block uploads provenance only, logs "completion skipped", exits 1.
4. **Protocol prompt delivery.** The runner reads `GYRE_PROTOCOL_PROMPT` into the LLM query prompt (assembly order meta-spec → protocol → task). Prompt text is documentation; the subprocess execution in (2)/(3) is the enforcement, as required.

### Probes run (evidence: `/tmp/stage/review-evidence/`)

- `node --test agent-runner.test.mjs` (in `docker/gyre-agent`) → **11/11 pass** (`runner-tests.txt`).
- Mutation A — hollow gates (`if (false && localGates.length > 0)`, i.e. the prompt-only implementation the verification intent targets): **3 tests fail** (`mutation-a-hollow-gates.txt`) — failing-gate/no-complete, hanging-gate-timeout, and gate-log-output assertions all fire.
- Mutation B — `GYRE_PROTOCOL_PROMPT` read removed from prompt assembly: **2 tests fail** (`mutation-b-no-protocol-prompt.txt`) — inclusion and ordering.
- Mutation C — gate-phase heartbeat keep-alive disabled: **exactly the heartbeat-continuity regression test fails** (`mutation-c-no-gate-heartbeat.txt`).
- `python3 scripts/check-rustfmt-diff.py e9f6a31c…` → exit 0 (`rustfmt-diff.txt`).
- `cargo check -p gyre-server --tests` → clean, 9m41s (`cargo-check-gyre-server.txt`) — the two new spawn tests compile against the real router/kubectl-stub path; full test execution deferred to the verifier per the 4 GiB budget rule.
- `scripts/check-task-commit-attribution.sh` → exit 0; task frontmatter at the candidate lists both product commits (`950a65a0`, `36dab5b4`).
- Working tree restored clean after every mutation (`git status` shows only the pre-existing executor-side `specs/tasks/task-268.md` modification; no production source touched).

### Tests the verifier must execute

- `cargo test -p gyre-server --lib api::spawn::tests::spawn_delivers_local_gates_from_repo_gate_config`
- `cargo test -p gyre-server --lib api::spawn::tests::spawn_local_gates_empty_when_repo_configures_none`
- `cd docker/gyre-agent && npm test`
- `python3 scripts/check-rustfmt-diff.py e9f6a31c3903ff61597af88d0b70b41d94bb911a`

### Acceptance criteria assessment

- **Failing gate → no MR, feedback, remediation query:** runner test proves nonzero exit, zero `/complete` requests (recording HTTP server), gate output in the runner log, and the remediation follow-up query carrying gate name + captured output + exit code. Server-side MR creation is unreachable without `/complete`. ✔ (mutation A kills)
- **Passing gates → completion unbroken:** test asserts `POST /agents/:id/complete` reached and exit 0 when all gates pass; no-gates case proceeds directly. ✔
- **GYRE_PROTOCOL_PROMPT in query + GYRE_LOCAL_GATES from real rows:** protocol-inclusion/ordering tests (mutation B kills); spawn tests assert gates delivered from seeded `QualityGate` rows via the real spawn path (TestCommand + LintCommand delivered with configured commands; RequiredApprovals and PostMerge excluded; empty repo → `[]`). Compile-verified here; execution recorded for verifier. ✔
- **Timeout-bounded execution:** hanging-gate test — `sleep 60` with `timeout_secs: 1` killed, reported "timed out", no completion, runner exits well under 30s. ✔

### Test-inflation check

None found. Every new assertion binds to observable production behavior (recorded HTTP requests, subprocess exit codes/output, captured LLM query prompts, elapsed wall time); all three mutation probes kill. The spawn tests drive the real router with a kubectl stub capturing the applied pod manifest, and would fail if the injection were removed or commands hardcoded (assertions reference the seeded per-repo command strings).

### Scope check

Diff touches only `crates/gyre-server/src/api/spawn.rs` (env assembly + tests), `docker/gyre-agent/agent-runner.mjs` + its test, `docs/agent-protocol.md` (accurate env-var + flow documentation), and `specs/tasks/task-268.md` (delivery record). No coverage matrix exists for `specs/development/agent-workflow.md` (`specs/coverage/` covers system/ specs only) — the task's "no rows to update" claim verified accurate. No unrelated spec rows touched.
