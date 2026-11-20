# Review: task-268 — Pre-PR local verification (agent-local quality gates before MR)

- Candidate: `4411241202a97531168cb76f764aaa3223fda5d7` (commits `950a65a0`, `44112412`)
- Base: `e9f6a31c3903ff61597af88d0b70b41d94bb911a`
- Verdict: **needs revision** (2 findings; see `/tmp/stage/verdict.json`)
- Evidence: `/tmp/stage/review-evidence/task-268-review.md` + probe logs

## What was verified as real

All four required behaviors are implemented with production code, not prose:

1. **Server-side gate delivery** — `spawn.rs` `launch_agent_process`
   resolves pre-merge `TestCommand`/`LintCommand` gates via
   `QualityGateRepository::list_by_repo_id_and_phase` (real repo gate rows,
   never hardcoded), fails the launch closed on repository error, and
   injects `GYRE_LOCAL_GATES` JSON (`[]` when none). Type-checked
   (`cargo check -p gyre-server`, exit 0); the two new spawn tests mirror
   the existing pod-manifest env-assertion pattern (verifier must run
   them — gyre-server test compilation exceeds the review sandbox budget).
2. **Runner-side local execution** — `agent-runner.mjs` runs each gate as
   a real subprocess in the agent work dir after the query loop and before
   any completion call, with a bounded timeout (process-group SIGKILL plus
   pipe-destroy settle so the promise always settles).
3. **Failure feedback loop** — no completion / no MR on failure; failing
   gate name + captured output fed back as follow-up SDK queries; bounded
   retry cap (`GYRE_LOCAL_GATE_MAX_RETRIES`, default 2); nonzero exit with
   no completion call when exhausted or when `GYRE_LOCAL_GATES` is
   malformed; error-path force-completion removed (upload-only on error).
4. **Protocol prompt delivery** — the runner now reads
   `GYRE_PROTOCOL_PROMPT` into the LLM query prompt (meta-spec → protocol
   → task order asserted).

Test quality probes (real runner as subprocess, stub SDK recording every
query prompt, recording HTTP server proving whether POST /complete was
reached):

- Pristine suite: 10/10 pass.
- Mutation A (remove the `GYRE_PROTOCOL_PROMPT` read): 2 tests fail — the
  prompt-assembly tests are not self-confirming.
- Mutation B (hollow version — gates prose kept, `runLocalGates()` returns
  `[]`): failing-gate and hanging-gate tests fail — the exact prompt-only
  "implementation" the verification intent targets is caught.

## Findings (blocking)

**F1 — heartbeat starvation during local gate execution.** Heartbeats are
sent only inside the SDK message loop; the new gate-execution phase (up to
300 s per gate by default, sequential, across remediation rounds) sends
none, and `POST /agents/:id/logs` does not refresh liveness. The server's
stale-agent detector (`HEARTBEAT_TIMEOUT_SECS = 60`, job every 30 s) then
applies `disconnected_behavior` mid-verification for any gate run over
60 s — the normal case for a real test suite, i.e. this feature's primary
scenario: default `Pause` terminally stops the agent and revokes its
task-scoped secrets (breaking subsequent remediation MCP calls) with a
false `agent.failed` event; `Abort` marks it Dead, cleans worktrees and
blocks the task; `complete_agent` then still creates the MR for a
Stopped/Dead agent. This contradicts the implemented spec section
("slow or absent feedback is a platform bug"). Fix: heartbeat on a timer
(or at least per gate) while local gates execute. Reproduction:
`GYRE_LOCAL_GATES='[{"id":"g","name":"t","command":"sleep 90; true"}]'`.

**F2 — CI fmt gate fails on changed lines.**
`python3 scripts/check-rustfmt-diff.py e9f6a31c` exits 1 (spawn.rs lines
8315, 8386, 8424, 8434–8441 — all inside the new task-268 tests). Run
`cargo fmt` on the changed code.

## Verifier must run

- `cargo test -p gyre-server spawn_delivers_local_gates_from_repo_gate_config`
- `cargo test -p gyre-server spawn_local_gates_empty_when_repo_configures_none`
- CI fmt job on the merged diff (currently failing, F2)
- `cd docker/gyre-agent && node --test agent-runner.test.mjs`
