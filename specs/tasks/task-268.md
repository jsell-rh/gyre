---
title: "Pre-PR local verification: repo quality gates mechanically executed in the agent environment before any MR is opened"
spec_ref: "Spec gap spec-4c39ca6ea3e71ce8a53398f3 / pre-pr-local-verification / d30f233daf86"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-workflow.md §Immediate Feedback, tests/lints/type checks (the repo's configured command-based quality gates) must run locally in the agent's environment BEFORE any PR (MR) is opened, with immediate unambiguous success/failure feedback. Today the only mechanical gate execution is server-side in gate_executor.rs after MR creation and merge-queue enqueue, and the reference runner (docker/gyre-agent/agent-runner.mjs) single-shots the LLM, then uploads conversation and calls POST /agents/:id/complete (which creates the MR) with no local verification — it even force-completes on runner error. Implement: (1) Server-side gate delivery — at spawn (crates/gyre-server/src/api/spawn.rs container_env assembly), resolve the repo's locally-runnable command gates via QualityGateRepository (TestCommand/LintCommand-type gates for the repo, from list_by_repo/list_by_repo_id_and_phase — real repo gate config, never hardcoded commands) and inject them into the container as a machine-readable env var (e.g. GYRE_LOCAL_GATES JSON [{id, name, command}]) alongside the existing GYRE_* variables; empty list when the repo configures none. (2) Runner-side local execution — agent-runner.mjs, after the agent query loop and BEFORE uploading conversation/calling complete: execute each GYRE_LOCAL_GATES command in the agent working directory (GYRE_WORK_DIR/cwd) as a real subprocess with a bounded timeout (no unbounded waits — slow/absent feedback is a platform bug per the spec), capturing exit status and output. (3) Failure feedback loop — on gate failure, do NOT call complete and do NOT open an MR; run a bounded remediation loop: feed the failing gate name and captured output back to the agent as a follow-up SDK query ('gate X failed with: ...'), let it fix, re-run the gates; proceed to completion only when all local gates pass. If a retry cap is exhausted with gates still failing, terminate the runner with a nonzero exit and no completion call — an unambiguous failure, never a knowingly-broken MR. Remove the error-path force-completion: a runner error before local verification passes must not open an MR. (4) Protocol prompt delivery — the server already injects GYRE_PROTOCOL_PROMPT (spawn.rs container_env) but the runner never reads it; make the runner read GYRE_PROTOCOL_PROMPT and include it in the LLM query prompt (alongside GYRE_META_SPEC_PROMPT) so the protocol's 'run the repo's quality gates' instruction actually reaches the model in the autonomous path. Prompt text alone is not the implementation — the mechanical subprocess execution in (2)/(3) is the enforcement.

## Acceptance Criteria

- [ ] Agent whose change fails a repo-configured test gate: after the runner finishes, no MR exists for the agent/repo (query the MR store — zero MRs created), the runner log contains the failing gate's captured output, and the remediation loop demonstrably fed that failure text back to the agent in a follow-up query before any completion attempt.
- [ ] Agent whose change passes all configured local gates: the runner proceeds to POST /complete and the MR is created (existing completion flow unbroken).
- [ ] Runner prompt assembly includes GYRE_PROTOCOL_PROMPT content in the LLM query when the variable is set, and GYRE_LOCAL_GATES is derived from the repo's actual QualityGate repository rows (a repo with a `cargo test` TestCommand gate yields that command; a repo with no command gates yields an empty list).
- [ ] Local gate subprocess execution is timeout-bounded (a gate command that hangs is killed and treated as a failure with feedback, not an indefinite wait).

## Verification intent

Configure a repo with a TestCommand gate whose command deterministically fails (e.g. a script exiting 1 with distinctive output), spawn an agent that does not fix it, and let the runner finish. Probe asserts: zero MRs exist for the agent and POST /complete was never reached (nonzero runner exit, remediation loop exhausted). The real defect this must catch: a prompt-only 'implementation' that adds local-gates prose to the task prompt while the runner still mechanically completes and opens the MR — under that hollow version the probe finds a created MR and fails. A second probe: remove the GYRE_PROTOCOL_PROMPT read from the runner and the prompt-assembly test fails, proving the protocol reaches the model rather than dying in an unread env var.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-workflow.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `b3acf94c2c31906d0a88cf9be08f87a86c09232cb273c0a110aa5daca9688c95`.

[
  {
    "id": "d5679654c745172ab4f3fe87",
    "decision": "gap",
    "requirement_quote": "Tests, lints, type checks, snapshot comparisons - all run locally in the agent's environment before any PR is opened.",
    "detail": "The section requires (a) immediate, unambiguous success/failure feedback for every agent, (b) tests/lints/type checks/snapshot comparisons running locally in the agent's environment BEFORE any PR is opened, and (c) treating slow/absent feedback as a platform bug. What production code delivers: push-time pre-accept gates reject bad pushes synchronously with explicit reason strings; agent completion returns the created MR over HTTP; and the Ralph-loop watcher (crates/gyre-server/src/ralph_loop.rs) polls gate results on a 2s interval and, on failure, writes gate-failure messages to the agent's inbox, closes the MR, and re-spawns the agent — so failure feedback exists, but only AFTER the MR (the PR equivalent) is opened. Requirement (b) is not implemented anywhere: the reference autonomous agent runner (docker/gyre-agent/agent-runner.mjs) builds its task prompt as 'implement, commit, and push' with no test/lint/type-check step, and the server never sets GYRE_TASK_PROMPT (the only override for that prompt body; grep over crates/ finds the variable only in agent-runner.mjs and its test). TestCommand/LintCommand quality gates execute as server-side subprocesses in gate_executor.rs, triggered by merge-queue enqueue — i.e. after MR creation, not locally in the agent's environment before the PR. The only production text instructing agents to run gates locally is the protocol prompt ('run the repo's quality gates', crates/gyre-server/src/api/prompts.rs:163), but agent-runner.mjs never reads GYRE_PROTOCOL_PROMPT/GYRE_PROMPT_PROTOCOL into the LLM query (it consumes only GYRE_META_SPEC_PROMPT and GYRE_TASK_PROMPT), so even that prompt-level instruction does not reach the model in the reference runner. No CLI or MCP tool exists for an agent to run the repo's gate set locally pre-PR either.",
    "evidence": [
      {
        "file": "docker/gyre-agent/agent-runner.mjs",
        "quote": "Do NOT call gyre_agent_complete — the runner handles completion after uploading\nconversation provenance. Just implement, commit, and push.",
        "detail": "The reference autonomous runner's built-in task prompt (used whenever GYRE_TASK_PROMPT is unset, which is always — the server never sets it) instructs the agent to implement, commit, and push, with no step that runs tests, lints, type checks, or snapshot comparisons in the agent's local environment before the work is pushed and the MR is opened by the completion call."
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "quote": "GateType::TestCommand | GateType::LintCommand => {\n            run_command(gate.command.as_deref().unwrap_or(\"true\")).await",
        "detail": "Test and lint gates execute as server-side subprocesses spawned by the gate executor when the merge queue runs an MR's gates (post-merge gates additionally run in a detached worktree at the merge head, gate_executor.rs:3578-3615). This is the platform's only mechanical test/lint execution path, and it runs after the MR is created and enqueued — not locally in the agent's environment before any PR is opened, as the spec requires."
      },
      {
        "file": "crates/gyre-server/src/ralph_loop.rs",
        "quote": "Puts the MR on the merge queue (which triggers gate execution\n/// — test, lint, spec review, trace capture per the repo's gate config)\n/// and starts the Ralph-loop watcher.",
        "detail": "The production feedback loop is explicitly post-MR: completion creates the MR, then enqueues it, and only then do test/lint gates run. Success/failure feedback to the agent (gate-failure inbox messages, re-spawn with failure context, MR merge → task Done) is real but arrives after the PR is opened, satisfying bullet (a) only on the post-PR path and leaving bullet (b) — local pre-PR checks in the agent's environment — unimplemented."
      },
      {
        "file": "crates/gyre-server/src/api/prompts.rs",
        "quote": "3. **Act** — implement, run the repo's quality gates, commit with a conventional commit message, push your branch.",
        "detail": "The protocol prompt (single source for GYRE_PROTOCOL_PROMPT / system://protocol) does instruct agents to run the repo's quality gates locally in the Act step. However this is prompt prose, not enforcement, and the reference runner never delivers it: agent-runner.mjs passes only GYRE_META_SPEC_PROMPT plus its built-in task prompt to the SDK query and never reads GYRE_PROTOCOL_PROMPT, so the local-gates instruction does not reach the model in the autonomous path."
      }
    ]
  }
]
