---
title: "Publish reviewed spec backlog for specs/development/agent-workflow.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/agent-workflow.md",
  "plan_id": "b3acf94c2c31906d0a88cf9be08f87a86c09232cb273c0a110aa5daca9688c95",
  "audit": {
    "base": "fb1424fbac55cd23cb08606f4cd667a96f4dbc43",
    "requirements": "d30f233daf8671f8b8a8673b2325fd98efc07c8b5d805b54b57562f6099dc7ef",
    "sections": [
      {
        "id": "4cd062d3377f45db98acc80b",
        "decision": "context",
        "requirement_quote": "# Agent Development Workflow Requirements",
        "detail": "Title-only parent heading with no body text of its own; the entire canonical content of specs/development/agent-workflow.md lives in the four child sections (Immediate Feedback, Parallel Work via Worktrees, PRs as the Collaboration Primitive, Fix the Environment Not the Agent), each audited individually below. Counting the child requirements again here would double-count them, so this heading is recorded as context/aggregation only.",
        "evidence": []
      },
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
      },
      {
        "id": "346ce862de4b15352f0239c7",
        "decision": "gap",
        "requirement_quote": "Worktrees are ephemeral - created at task start, cleaned up on completion.",
        "detail": "Bullets 1 and 2 are satisfied by real production wiring: POST /api/v1/agents/spawn provisions each agent an isolated working copy — a jj workspace ({repo}/workspaces/{agent_id}) or, as fallback, a plain-git worktree ({repo}/worktrees/{branch_slug}) via git_ops.create_worktree — recorded in the agent_worktrees table linked to agent and task (crates/gyre-server/src/api/spawn.rs:934-1143), so parallel agents work on separate branches/directories without stepping on each other. Bullet 3 is only half-implemented: worktrees are created at task start, but they are NOT cleaned up on completion. The complete handler (spawn.rs:2632-2820, shared by the REST and MCP completion surfaces via create_mr_for_completion) creates the MR, transitions the task to Review and the agent to Idle, emits audit events, revokes the agent token, revokes task-scoped secrets, writes snapshot refs, and notifies the spawner — it never calls git_ops.remove_worktree or worktrees.delete for the agent's worktree. Worktree removal exists only on three other paths: the stale-agent detector's Dead transition (stale_agents.rs:105-126), the admin kill path (admin.rs:405-407), and the manual DELETE /api/v1/repos/:id/worktrees/:id route. A normally-completing agent's worktree and its DB record therefore persist indefinitely after completion (docs/agent-protocol.md even claims complete 'removes the git worktree' — the code does not), contradicting the 'ephemeral, cleaned up on completion' requirement.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": ".create_worktree(&repo_root, &worktree_path, &req.branch)",
            "detail": "Satisfied part: spawn creates a real per-agent git worktree (jj workspace when jj provisioning succeeds, plain-git worktree at {repo_root}/worktrees/{branch_slug} otherwise) and records it in agent_worktrees linked to the agent and its task — isolated copies per task, supporting parallel agents."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "/// Agent signals it has finished its task:\n/// 1. Creates a MergeRequest (source->target)\n/// 2. Transitions task status to Review\n/// 3. Transitions agent status to Idle",
            "detail": "The completion contract itself lists no worktree-removal step. I inspected the full complete_agent handler body (spawn.rs:2632 through the ralph_loop on_agent_mr_created call at the end): it performs MR creation, status transitions, audit events, token revocation, interrogation-policy and conversation-context cleanup, task-secret revocation, snapshot-ref writing, analytics, and notifications — remove_worktree/worktrees.delete are never called on this path (grep confirms remove_worktree appears only in admin.rs, stale_agents.rs, gate_executor.rs, and the git2/mem adapters). The worktree of a successfully completed agent is therefore not cleaned up on completion."
          },
          {
            "file": "crates/gyre-server/src/stale_agents.rs",
            "quote": "// Clean up worktrees\n                if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {",
            "detail": "Worktree teardown is implemented only on the Dead path: the stale-agent detector removes worktrees (including jj workspace teardown) when an agent is marked Dead after heartbeat expiry — not when the agent completes its task. The only other removal paths are the admin kill handler (admin.rs:405-407) and the manual DELETE /repos/:id/worktrees/:id route, so 'cleaned up on completion' has no production implementation."
          }
        ]
      },
      {
        "id": "4a3b61a03d2e9cd037d33c0c",
        "decision": "gap",
        "requirement_quote": "A **PR hygiene agent** (or background job) continuously monitors for stale PRs - unreviewed, unmerged, conflicted, or abandoned - and takes action: nudge reviewers, rebase, escalate, or close with explanation.",
        "detail": "Partially satisfied: MRs are genuinely the submission/collaboration primitive — agent completion auto-creates the MR and hands off to the merge queue and its gates, the protocol forbids agents merging their own work, and reviews/merge-queue/gates (merge_requests.rs, merge_queue.rs, gate_executor.rs) implement the PR flow. Two normative bullets are unmet. (1) 'No direct pushes to main': nothing rejects pushes to the default branch. Git HTTP authorization is repo-level (authorize_repo_access checks a push permission, git_http.rs:148-215), the pre-accept push-gate registry contains only conventional-commit, task-ref, and no-em-dash (pre_accept.rs), and the production post-receive path processes default-branch pushes as normal accepted events (spec-lifecycle task creation, persona changes, ledger sync; git_http.rs tests push directly to refs/heads/main and succeed). (2) The PR hygiene requirement: the only stale-PR monitoring in production is the daily abandoned_branch_check job, which flags only spec-edit/* MRs inactive >7 days and only creates inbox notifications for Admin/Developer members — it never nudges reviewers, rebases, escalates, or closes with explanation, runs once daily rather than continuously, and ignores unreviewed/conflicted/abandoned MRs on ordinary branches. Conflict handling exists only as the merge processor's rebase of in-flight agents when the target baseline moves (merge_processor.rs:290-330), which is not stale-PR hygiene. No background job or agent monitors unreviewed or long-open MRs, and nothing auto-closes stale PRs.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "//! - `conventional-commit` — reject commits that don't match `type(scope): description`\n//! - `task-ref`            — reject feat/ and fix/ branches without a TASK-{id} reference\n//! - `no-em-dash`          — reject commits whose message contains an em-dash (—)",
            "detail": "The complete built-in pre-accept push-gate catalog. None of the gates rejects a push targeting the default branch, so 'No direct pushes to main' is not enforced at the only synchronous push-check point in the platform."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "// Spec lifecycle: auto-create tasks for spec changes on the default\n        // branch — only for ref updates git actually applied.",
            "detail": "Production post-receive handling treats pushes to the default branch as normal, accepted events to be processed (spec-lifecycle scans, persona-change handling, ledger sync all key off applied default-branch ref updates; tests in this file push directly to refs/heads/main and succeed). Combined with repo-level-only push authorization (authorize_repo_access, git_http.rs:148), a caller with push permission can push straight to main with no MR — the spec's 'All code submission happens via pull requests. No direct pushes to main' is not enforced."
          },
          {
            "file": "crates/gyre-server/src/abandoned_branch.rs",
            "quote": "//! Runs daily per workspace. Flags `spec-edit/*` branches where the MR has had\n//! no activity for >7 days (`updated_at` older than 7 days). Creates priority-9\n//! `AbandonedBranch` notifications for each Admin/Developer member of the workspace.",
            "detail": "The only stale-PR monitor in production (registered as the daily abandoned_branch_check job in jobs.rs:471-483). Its scope is a narrow slice of the spec requirement: only spec-edit/* source branches, only the 'abandoned' stale class (not unreviewed, unmerged-conflicted, or general long-open PRs), daily rather than continuous, and its only action is creating a notification — it never nudges reviewers, rebases, escalates, or closes the MR with an explanation."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "When your task is done, commit to your branch, push, and signal completion with `gyre_agent_complete` — this creates a merge request and moves your task to review. Never merge your own work: merging is decided by the merge queue and its gates.",
            "detail": "Satisfied part of the section: MRs are the real collaboration primitive for agents — completion auto-creates the MR (create_mr_for_completion shared by the REST and MCP surfaces), the merge queue runs gates, and agents cannot merge their own work. The gap is the missing enforcement against direct default-branch pushes and the missing continuous stale-PR hygiene actions, not the MR flow itself."
          }
        ]
      },
      {
        "id": "5d7c43ad773e272f9c6f6e44",
        "decision": "gap",
        "requirement_quote": "Instead: diagnose the root cause and **engineer the environment** to catch or prevent that class of error in the future (lint rule, pre-commit hook, better error message, test fixture, etc.).",
        "detail": "The section requires that when an agent hits an error the first response is root-cause diagnosis plus environment hardening (lint rule, pre-commit hook, better error message, test fixture) BEFORE any retry, and names the supporting mechanism: an interrupt ledger that records every error as a signal so recurring error classes are structurally fixed once. No production implementation of this requirement exists. The string 'interrupt ledger' appears nowhere in crates/, web/, scripts/, or docker/ — only in the spec file itself. I inspected the agent error/retry paths — ralph_loop.rs (Phase 7 fail path), gate_executor.rs (gate failure recording), spawn.rs (re-spawn machinery), the jobs registry (jobs.rs), gyre-domain and gyre-ports stores — and found no error ledger, no recurring-error classification, and no environment-hardening step anywhere in the loop. The actual failure path is a retry: on gate failure the MR is closed and the agent is re-spawned with the same persona, same meta-spec set, same task, and fresh context, with failure messages carried into the new context; the protocol prompt tells the agent 'read the failure messages ... fix, commit, push, complete again'. Nothing between iterations requires, verifies, or even records that the class of error was structurally fixed (e.g. a new lint rule or hook) before the retry — the environment is unchanged, which is exactly the 'try again' first response the spec forbids. (The Gyre repository's own development process does use pre-commit hooks and the scripts/check-* suite as environment hardening, but that is this repo's dev tooling for its own contributors, not a platform implementation of the interrupt-ledger requirement for spawned agents.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "//! - Phase 7 fail path: gate failures are written to the agent's inbox as\n//!   messages, the MR is closed, and the agent is re-spawned — same persona,\n//!   same meta-spec set, same task, fresh context (no conversation history),\n//!   iteration counter incremented.",
            "detail": "The production failure path is a re-spawn retry: the environment (gate definitions, hooks, tooling, prompts other than carried failure text) is identical between iterations. No step diagnoses the error class or engineers the environment (lint rule, pre-commit hook, better error message, test fixture) before the agent retries, and no ledger records the error so recurrences can be detected and structurally fixed — the interrupt-ledger mechanism the section names does not exist in production source (grep for 'interrupt ledger' over crates/, web/, scripts/, docker/ matches only the spec file)."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "On gate failure, read the failure messages (carried in `system://context`), fix, commit, push, complete again.",
            "detail": "The protocol the platform actually gives agents on error is 'fix, commit, push, complete again' — a retry with feedback, not 'diagnose the root cause and engineer the environment first'. Combined with the re-spawn path above, the first response to an agent error is precisely the 'try again' the spec forbids; no intervening environment-hardening step exists on any inspected path (ralph_loop.rs on_gates_failed/respawn, gate_executor gate-failure recording, spawn.rs re-spawn machinery, jobs.rs background-job registry)."
          }
        ]
      }
    ],
    "gaps": [
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
      },
      {
        "id": "346ce862de4b15352f0239c7",
        "decision": "gap",
        "requirement_quote": "Worktrees are ephemeral - created at task start, cleaned up on completion.",
        "detail": "Bullets 1 and 2 are satisfied by real production wiring: POST /api/v1/agents/spawn provisions each agent an isolated working copy — a jj workspace ({repo}/workspaces/{agent_id}) or, as fallback, a plain-git worktree ({repo}/worktrees/{branch_slug}) via git_ops.create_worktree — recorded in the agent_worktrees table linked to agent and task (crates/gyre-server/src/api/spawn.rs:934-1143), so parallel agents work on separate branches/directories without stepping on each other. Bullet 3 is only half-implemented: worktrees are created at task start, but they are NOT cleaned up on completion. The complete handler (spawn.rs:2632-2820, shared by the REST and MCP completion surfaces via create_mr_for_completion) creates the MR, transitions the task to Review and the agent to Idle, emits audit events, revokes the agent token, revokes task-scoped secrets, writes snapshot refs, and notifies the spawner — it never calls git_ops.remove_worktree or worktrees.delete for the agent's worktree. Worktree removal exists only on three other paths: the stale-agent detector's Dead transition (stale_agents.rs:105-126), the admin kill path (admin.rs:405-407), and the manual DELETE /api/v1/repos/:id/worktrees/:id route. A normally-completing agent's worktree and its DB record therefore persist indefinitely after completion (docs/agent-protocol.md even claims complete 'removes the git worktree' — the code does not), contradicting the 'ephemeral, cleaned up on completion' requirement.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": ".create_worktree(&repo_root, &worktree_path, &req.branch)",
            "detail": "Satisfied part: spawn creates a real per-agent git worktree (jj workspace when jj provisioning succeeds, plain-git worktree at {repo_root}/worktrees/{branch_slug} otherwise) and records it in agent_worktrees linked to the agent and its task — isolated copies per task, supporting parallel agents."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "/// Agent signals it has finished its task:\n/// 1. Creates a MergeRequest (source->target)\n/// 2. Transitions task status to Review\n/// 3. Transitions agent status to Idle",
            "detail": "The completion contract itself lists no worktree-removal step. I inspected the full complete_agent handler body (spawn.rs:2632 through the ralph_loop on_agent_mr_created call at the end): it performs MR creation, status transitions, audit events, token revocation, interrogation-policy and conversation-context cleanup, task-secret revocation, snapshot-ref writing, analytics, and notifications — remove_worktree/worktrees.delete are never called on this path (grep confirms remove_worktree appears only in admin.rs, stale_agents.rs, gate_executor.rs, and the git2/mem adapters). The worktree of a successfully completed agent is therefore not cleaned up on completion."
          },
          {
            "file": "crates/gyre-server/src/stale_agents.rs",
            "quote": "// Clean up worktrees\n                if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {",
            "detail": "Worktree teardown is implemented only on the Dead path: the stale-agent detector removes worktrees (including jj workspace teardown) when an agent is marked Dead after heartbeat expiry — not when the agent completes its task. The only other removal paths are the admin kill handler (admin.rs:405-407) and the manual DELETE /repos/:id/worktrees/:id route, so 'cleaned up on completion' has no production implementation."
          }
        ]
      },
      {
        "id": "4a3b61a03d2e9cd037d33c0c",
        "decision": "gap",
        "requirement_quote": "A **PR hygiene agent** (or background job) continuously monitors for stale PRs - unreviewed, unmerged, conflicted, or abandoned - and takes action: nudge reviewers, rebase, escalate, or close with explanation.",
        "detail": "Partially satisfied: MRs are genuinely the submission/collaboration primitive — agent completion auto-creates the MR and hands off to the merge queue and its gates, the protocol forbids agents merging their own work, and reviews/merge-queue/gates (merge_requests.rs, merge_queue.rs, gate_executor.rs) implement the PR flow. Two normative bullets are unmet. (1) 'No direct pushes to main': nothing rejects pushes to the default branch. Git HTTP authorization is repo-level (authorize_repo_access checks a push permission, git_http.rs:148-215), the pre-accept push-gate registry contains only conventional-commit, task-ref, and no-em-dash (pre_accept.rs), and the production post-receive path processes default-branch pushes as normal accepted events (spec-lifecycle task creation, persona changes, ledger sync; git_http.rs tests push directly to refs/heads/main and succeed). (2) The PR hygiene requirement: the only stale-PR monitoring in production is the daily abandoned_branch_check job, which flags only spec-edit/* MRs inactive >7 days and only creates inbox notifications for Admin/Developer members — it never nudges reviewers, rebases, escalates, or closes with explanation, runs once daily rather than continuously, and ignores unreviewed/conflicted/abandoned MRs on ordinary branches. Conflict handling exists only as the merge processor's rebase of in-flight agents when the target baseline moves (merge_processor.rs:290-330), which is not stale-PR hygiene. No background job or agent monitors unreviewed or long-open MRs, and nothing auto-closes stale PRs.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "//! - `conventional-commit` — reject commits that don't match `type(scope): description`\n//! - `task-ref`            — reject feat/ and fix/ branches without a TASK-{id} reference\n//! - `no-em-dash`          — reject commits whose message contains an em-dash (—)",
            "detail": "The complete built-in pre-accept push-gate catalog. None of the gates rejects a push targeting the default branch, so 'No direct pushes to main' is not enforced at the only synchronous push-check point in the platform."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "// Spec lifecycle: auto-create tasks for spec changes on the default\n        // branch — only for ref updates git actually applied.",
            "detail": "Production post-receive handling treats pushes to the default branch as normal, accepted events to be processed (spec-lifecycle scans, persona-change handling, ledger sync all key off applied default-branch ref updates; tests in this file push directly to refs/heads/main and succeed). Combined with repo-level-only push authorization (authorize_repo_access, git_http.rs:148), a caller with push permission can push straight to main with no MR — the spec's 'All code submission happens via pull requests. No direct pushes to main' is not enforced."
          },
          {
            "file": "crates/gyre-server/src/abandoned_branch.rs",
            "quote": "//! Runs daily per workspace. Flags `spec-edit/*` branches where the MR has had\n//! no activity for >7 days (`updated_at` older than 7 days). Creates priority-9\n//! `AbandonedBranch` notifications for each Admin/Developer member of the workspace.",
            "detail": "The only stale-PR monitor in production (registered as the daily abandoned_branch_check job in jobs.rs:471-483). Its scope is a narrow slice of the spec requirement: only spec-edit/* source branches, only the 'abandoned' stale class (not unreviewed, unmerged-conflicted, or general long-open PRs), daily rather than continuous, and its only action is creating a notification — it never nudges reviewers, rebases, escalates, or closes the MR with an explanation."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "When your task is done, commit to your branch, push, and signal completion with `gyre_agent_complete` — this creates a merge request and moves your task to review. Never merge your own work: merging is decided by the merge queue and its gates.",
            "detail": "Satisfied part of the section: MRs are the real collaboration primitive for agents — completion auto-creates the MR (create_mr_for_completion shared by the REST and MCP surfaces), the merge queue runs gates, and agents cannot merge their own work. The gap is the missing enforcement against direct default-branch pushes and the missing continuous stale-PR hygiene actions, not the MR flow itself."
          }
        ]
      },
      {
        "id": "5d7c43ad773e272f9c6f6e44",
        "decision": "gap",
        "requirement_quote": "Instead: diagnose the root cause and **engineer the environment** to catch or prevent that class of error in the future (lint rule, pre-commit hook, better error message, test fixture, etc.).",
        "detail": "The section requires that when an agent hits an error the first response is root-cause diagnosis plus environment hardening (lint rule, pre-commit hook, better error message, test fixture) BEFORE any retry, and names the supporting mechanism: an interrupt ledger that records every error as a signal so recurring error classes are structurally fixed once. No production implementation of this requirement exists. The string 'interrupt ledger' appears nowhere in crates/, web/, scripts/, or docker/ — only in the spec file itself. I inspected the agent error/retry paths — ralph_loop.rs (Phase 7 fail path), gate_executor.rs (gate failure recording), spawn.rs (re-spawn machinery), the jobs registry (jobs.rs), gyre-domain and gyre-ports stores — and found no error ledger, no recurring-error classification, and no environment-hardening step anywhere in the loop. The actual failure path is a retry: on gate failure the MR is closed and the agent is re-spawned with the same persona, same meta-spec set, same task, and fresh context, with failure messages carried into the new context; the protocol prompt tells the agent 'read the failure messages ... fix, commit, push, complete again'. Nothing between iterations requires, verifies, or even records that the class of error was structurally fixed (e.g. a new lint rule or hook) before the retry — the environment is unchanged, which is exactly the 'try again' first response the spec forbids. (The Gyre repository's own development process does use pre-commit hooks and the scripts/check-* suite as environment hardening, but that is this repo's dev tooling for its own contributors, not a platform implementation of the interrupt-ledger requirement for spawned agents.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/ralph_loop.rs",
            "quote": "//! - Phase 7 fail path: gate failures are written to the agent's inbox as\n//!   messages, the MR is closed, and the agent is re-spawned — same persona,\n//!   same meta-spec set, same task, fresh context (no conversation history),\n//!   iteration counter incremented.",
            "detail": "The production failure path is a re-spawn retry: the environment (gate definitions, hooks, tooling, prompts other than carried failure text) is identical between iterations. No step diagnoses the error class or engineers the environment (lint rule, pre-commit hook, better error message, test fixture) before the agent retries, and no ledger records the error so recurrences can be detected and structurally fixed — the interrupt-ledger mechanism the section names does not exist in production source (grep for 'interrupt ledger' over crates/, web/, scripts/, docker/ matches only the spec file)."
          },
          {
            "file": "crates/gyre-server/src/api/prompts.rs",
            "quote": "On gate failure, read the failure messages (carried in `system://context`), fix, commit, push, complete again.",
            "detail": "The protocol the platform actually gives agents on error is 'fix, commit, push, complete again' — a retry with feedback, not 'diagnose the root cause and engineer the environment first'. Combined with the re-spawn path above, the first response to an agent error is precisely the 'try again' the spec forbids; no intervening environment-hardening step exists on any inspected path (ralph_loop.rs on_gates_failed/respawn, gate_executor gate-failure recording, spawn.rs re-spawn machinery, jobs.rs background-job registry)."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "docker/gyre-agent/agent-runner.mjs",
        "sha256": "6cbd3506aaf40b2db42f88526a5d62219ebef2d193d5f128e1263473e1720e62"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "955cdae6f97d41acbfd9b53ba66c5473654ce004c09ef6dd3547c9eedace4fa7"
      },
      {
        "file": "crates/gyre-server/src/ralph_loop.rs",
        "sha256": "fb2c86c50380faa13494ff31656efee895c8a0a1a02ff8c10351cf857c9884c2"
      },
      {
        "file": "crates/gyre-server/src/api/prompts.rs",
        "sha256": "87357e3d498e1934ac1529f7ab9c245a08e179db5be6db4f30836bac60956ff8"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "03f98632204e0c91331d34dfb53d5cbe4a71a7045fa9191ef9efeda3d434afe0"
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "sha256": "221b4c73f788426bbef0bcf597f8bd4d463c993d02aabd6b00f27f8fd6d7eb43"
      },
      {
        "file": "crates/gyre-server/src/pre_accept.rs",
        "sha256": "957d788e4e32cf25197f2665a72efe66349f5b3230eb9e93a22ef0bb4c92539c"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "35dee813aece670bf34875fa6d322ee56de210a9199688e0089f6f696eb4c8ee"
      },
      {
        "file": "crates/gyre-server/src/abandoned_branch.rs",
        "sha256": "4d7b462ae3c8eb1b8417b4c987b0bd726a5191516edd10b7ad8e111ef3494462"
      }
    ]
  },
  "plan": {
    "audit": "6b13cf1772d1aad8ecd7eb378e5f944e1aa27385fefdbc2287b72c55a897c881",
    "tasks": [
      {
        "key": "pre-pr-local-verification",
        "title": "Pre-PR local verification: repo quality gates mechanically executed in the agent environment before any MR is opened",
        "covers": [
          "d5679654c745172ab4f3fe87"
        ],
        "required_behavior": "Per specs/development/agent-workflow.md §Immediate Feedback, tests/lints/type checks (the repo's configured command-based quality gates) must run locally in the agent's environment BEFORE any PR (MR) is opened, with immediate unambiguous success/failure feedback. Today the only mechanical gate execution is server-side in gate_executor.rs after MR creation and merge-queue enqueue, and the reference runner (docker/gyre-agent/agent-runner.mjs) single-shots the LLM, then uploads conversation and calls POST /agents/:id/complete (which creates the MR) with no local verification — it even force-completes on runner error. Implement: (1) Server-side gate delivery — at spawn (crates/gyre-server/src/api/spawn.rs container_env assembly), resolve the repo's locally-runnable command gates via QualityGateRepository (TestCommand/LintCommand-type gates for the repo, from list_by_repo/list_by_repo_id_and_phase — real repo gate config, never hardcoded commands) and inject them into the container as a machine-readable env var (e.g. GYRE_LOCAL_GATES JSON [{id, name, command}]) alongside the existing GYRE_* variables; empty list when the repo configures none. (2) Runner-side local execution — agent-runner.mjs, after the agent query loop and BEFORE uploading conversation/calling complete: execute each GYRE_LOCAL_GATES command in the agent working directory (GYRE_WORK_DIR/cwd) as a real subprocess with a bounded timeout (no unbounded waits — slow/absent feedback is a platform bug per the spec), capturing exit status and output. (3) Failure feedback loop — on gate failure, do NOT call complete and do NOT open an MR; run a bounded remediation loop: feed the failing gate name and captured output back to the agent as a follow-up SDK query ('gate X failed with: ...'), let it fix, re-run the gates; proceed to completion only when all local gates pass. If a retry cap is exhausted with gates still failing, terminate the runner with a nonzero exit and no completion call — an unambiguous failure, never a knowingly-broken MR. Remove the error-path force-completion: a runner error before local verification passes must not open an MR. (4) Protocol prompt delivery — the server already injects GYRE_PROTOCOL_PROMPT (spawn.rs container_env) but the runner never reads it; make the runner read GYRE_PROTOCOL_PROMPT and include it in the LLM query prompt (alongside GYRE_META_SPEC_PROMPT) so the protocol's 'run the repo's quality gates' instruction actually reaches the model in the autonomous path. Prompt text alone is not the implementation — the mechanical subprocess execution in (2)/(3) is the enforcement.",
        "acceptance": [
          "Agent whose change fails a repo-configured test gate: after the runner finishes, no MR exists for the agent/repo (query the MR store — zero MRs created), the runner log contains the failing gate's captured output, and the remediation loop demonstrably fed that failure text back to the agent in a follow-up query before any completion attempt.",
          "Agent whose change passes all configured local gates: the runner proceeds to POST /complete and the MR is created (existing completion flow unbroken).",
          "Runner prompt assembly includes GYRE_PROTOCOL_PROMPT content in the LLM query when the variable is set, and GYRE_LOCAL_GATES is derived from the repo's actual QualityGate repository rows (a repo with a `cargo test` TestCommand gate yields that command; a repo with no command gates yields an empty list).",
          "Local gate subprocess execution is timeout-bounded (a gate command that hangs is killed and treated as a failure with feedback, not an indefinite wait)."
        ],
        "negative_probe": "Configure a repo with a TestCommand gate whose command deterministically fails (e.g. a script exiting 1 with distinctive output), spawn an agent that does not fix it, and let the runner finish. Probe asserts: zero MRs exist for the agent and POST /complete was never reached (nonzero runner exit, remediation loop exhausted). The real defect this must catch: a prompt-only 'implementation' that adds local-gates prose to the task prompt while the runner still mechanically completes and opens the MR — under that hollow version the probe finds a created MR and fails. A second probe: remove the GYRE_PROTOCOL_PROMPT read from the runner and the prompt-assembly test fails, proving the protocol reaches the model rather than dying in an unread env var.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "terminal-worktree-cleanup",
        "title": "Ephemeral worktrees: teardown when work is terminally complete (task terminal states), preserving the Ralph-loop respawn working copy",
        "covers": [
          "346ce862de4b15352f0239c7"
        ],
        "required_behavior": "Per specs/development/agent-workflow.md §Parallel Work via Worktrees, worktrees are ephemeral — created at task start (already real: spawn.rs provisions a jj workspace or plain-git worktree recorded in agent_worktrees) and cleaned up on completion. Today cleanup exists only on the stale-agent Dead transition (stale_agents.rs), the admin kill path, and the manual DELETE route — a normally-completing agent's worktree and its DB row persist indefinitely. CRITICAL placement constraint discovered in code: relaunch_agent_session (ralph_loop.rs) REUSES the agent's existing worktree record and working directory for Phase 7 gate-failure respawns ('agent has no worktree to re-spawn', ralph_loop.rs:684), so cleanup must NOT run at the MR-creation completion (complete_agent/create_mr_for_completion) — it must run when the work is terminally complete: (a) task Done after MR merge (the ralph_loop watcher's task→Done transition, ralph_loop.rs ~218-232); (b) the Phase 7 iteration-ceiling Blocked path (currently revokes the token but leaves the worktree); (c) the fail_agent / task-Cancelled terminal paths that lack cleanup. Cleanup must reuse the same teardown semantics as the stale_agents Dead path (jj workspace teardown or plain-git worktree removal via the git_ops/jj_ops ports, plus worktrees.delete for the DB record) — extract/reuse that helper rather than duplicating it. Ordering: cleanup runs AFTER snapshot-ref writing and AFTER any session-state capture at termination (task-258 — agent-session-commit-tracking, specs/system/source-control.md — records session end state: ended_at, current branch, and dirty state via git status --porcelain in the worktree on these same termination paths; removing the working copy first would destroy its data source, so this task must land after task-258. Note task-260 is the completed backlog-publication task for source-control.md and provides no such ordering guarantee). Only the working copy and its DB record are removed: the pushed branch, MR, commits, and snapshot refs must remain intact server-side. Update docs/agent-protocol.md (which currently claims complete 'removes the git worktree') to describe the actual terminal-cleanup semantics so the doc matches shipped behavior.",
        "acceptance": [
          "Happy path: an agent completes, its MR merges, the task transitions to Done — afterwards the worktree directory no longer exists on disk, worktrees.find_by_agent(agent) returns empty, and the MR, its branch, and the pushed commits are still present and readable.",
          "Iteration ceiling: an agent that exhausts max_iterations with failing gates (task Blocked, agent Failed) has its worktree removed from disk and DB.",
          "Respawn preserved: after a gate failure the agent is completed (MR created and closed) and re-spawned — the respawn succeeds in an existing worktree (cleanup did not fire at MR-creation completion).",
          "Teardown is the real VCS operation (jj workspace removed / git worktree pruned), not just a DB row delete — the directory is gone from the filesystem."
        ],
        "negative_probe": "Drive an agent to completion and merge (task Done), then stat the agent's worktree path and query worktrees.find_by_agent: today (the real defect) the directory and the agent_worktrees row persist indefinitely after completion — the probe must fail on the current tree by finding them, and pass only when both are gone. Companion probe proving correct placement: trigger a gate-failure respawn and assert the worktree still exists at relaunch time — a naive at-complete cleanup implementation (matching the stale docs/agent-protocol.md claim) breaks respawn with 'agent has no worktree to re-spawn' and fails this probe.",
        "dependencies": [
          "task-258"
        ],
        "existing_task": null
      },
      {
        "key": "pr-protection-and-hygiene",
        "title": "PR collaboration invariants: reject agent-principal pushes to the default branch (human push path preserved per spec-lifecycle.md) and continuously act on stale MRs (nudge, rebase, escalate, close)",
        "covers": [
          "4a3b61a03d2e9cd037d33c0c"
        ],
        "required_behavior": "Per specs/development/agent-workflow.md §PRs as the Collaboration Primitive. The MR flow itself is real (completion auto-creates the MR; merge queue + gates decide merges); two normative bullets are unmet. (1) 'No direct pushes to main' — scoped to the document's agent-workflow context: the spec under audit is 'Agent Development Workflow Requirements', and its bullet governs agent code submission. An unconditional rejection of EVERY default-branch push would contradict specs/system/spec-lifecycle.md, which normatively requires the opposite for human pushes: §Automatic Task Creation on Spec Change ('When a push lands on the default branch that modifies files in specs/, the forge automatically creates tasks. This is a server-side post-receive hook' — implemented as process_spec_lifecycle, git_http.rs:1214, which also handles persona changes and spec-ledger sync on applied default-branch ref updates) and §What This Does NOT Do ('Does not block spec changes. Anyone with push access can modify specs. The system reacts, it doesn't prevent.'). Therefore implement, in the git HTTP push path, rejection of pushes whose target ref is the repo's default branch — resolved from the Repository record's default_branch field, never a hardcoded \"main\" — when the caller is an AGENT principal (authenticated via an agent token / agent identity: an AuthenticatedAgent with no backing user — user_id None — and not the system principal), with an explicit rejection reason directing the agent to the MR flow. Human principals (user JWTs / API keys with push permission, and the system principal) pushing to the default branch remain accepted, and their post-receive spec-lifecycle processing (task creation, persona changes, ledger sync) continues to work unchanged. The agent-principal rejection is an unconditional invariant for that caller class, not an opt-in named pre-accept gate (check_pre_accept_gates returns Ok early when a repo has no gate names configured, git_http.rs:1753-1755, so an opt-in gate would leave default repos unprotected); implement it as a direct check in the push handler. It must not break server-side landing: the merge queue and post-merge reverts land via the git_ops/jj_ops ports server-side, not via HTTP push — verify with tests that merge-queue landing on the default branch still succeeds. Update git_http tests that rely on default-branch pushes to authenticate consistently with the intended caller class (system-token test pushes to main keep succeeding; add agent-token rejection coverage). (2) PR hygiene: a new background job (e.g. pr_hygiene_check) registered in jobs.rs on a short interval (minutes, not the daily abandoned_branch_check cadence — 'continuously monitors') that evaluates every open MR per workspace against stale classes using real persisted state (MergeRequest.updated_at, reviewers, has_conflicts / a real merge-base conflict check, review records): unreviewed beyond a nudge threshold → nudge assigned reviewers (notification; escalate to workspace Developers/Admins when an MR has no reviewers); conflicted → trigger the existing rebase machinery where applicable (agent-authored branches) and otherwise raise a rebase-required escalation; inactive/long-open beyond escalation thresholds → higher-priority escalation notification to workspace Admins; abandoned beyond a hard close threshold → close the MR with an explanation (status transition plus an explanatory notification/comment to the author — never a silent close). Thresholds are configurable (server/repo settings) with sane defaults, and every action is recorded (notification rows/audit events) so it is observable. All new routes get ABAC RouteResourceMapping entries (scripts/check-abac-route-registry.sh); handlers running without middleware ABAC enforce per-handler scoped authorization (scripts/check-abac-exempt-handlers.sh). The existing daily abandoned_branch_check job (HSI §8) is left intact — it is specced elsewhere.",
        "acceptance": [
          "Agent-principal push protection: an HTTP push of a new commit directly to refs/heads/{default_branch} authenticated with an agent token is rejected with an explicit error directing to the MR flow, and the ref is not updated server-side; the same agent token pushing a feature branch succeeds.",
          "Human push path preserved (spec-lifecycle.md reconciliation): a push-capable human token pushing a spec change to the repo's default branch is still accepted, and process_spec_lifecycle runs — spec-change tasks are created exactly as before; the system principal's pushes to the default branch are likewise unaffected.",
          "Protection resolves the real default branch per repo: a repo whose default_branch is e.g. \"trunk\" is protected on refs/heads/trunk for agent principals, and an agent push to a branch literally named \"main\" on that repo is NOT wrongly rejected.",
          "Server-side landing unaffected: a merge-queue merge landing on the default branch still succeeds (the rejection lives only in the HTTP push path for agent principals).",
          "Unreviewed nudge: an open MR with zero reviews older than the nudge threshold gets a reviewer-nudge notification on its next evaluation; an MR with no reviewers escalates to workspace Developers/Admins; a fresh MR under the threshold gets no action.",
          "Abandoned close: an open MR with updated_at beyond the hard close threshold is closed by the job with an explanation delivered to the author, and its status transition is persisted.",
          "Conflicted action: a conflicted MR gets a rebase attempt via the existing machinery (or a rebase-required escalation when not applicable) — some real action is taken and recorded, never silence.",
          "Continuous cadence: the job is registered with an interval (not run_at_utc_hour daily) and two evaluations occur within the test window, each idempotent (no duplicate notifications for the same stale MR on consecutive runs)."
        ],
        "negative_probe": "Push probe: authenticate as a spawned agent (agent token from agent_tokens), push a commit to the repo's default branch over git HTTP — today (the real defect) the push is accepted and processed by post-receive (spec-lifecycle scans run); the probe must fail on the current tree by observing the ref update succeed, and pass only when the push is rejected and the default-branch ref is unchanged. Companion probe guarding the scoping against over-blocking: the same push made with a human push-capable token (and a specs/ file change) must still be accepted and trigger spec-lifecycle task creation — an implementation that rejects human or system-principal default-branch pushes and thereby kills specs/system/spec-lifecycle.md §Automatic Task Creation fails this probe. Hygiene probe: seed an open MR whose updated_at is older than the hard close threshold, run the job once — today nothing ever closes it (the only stale-PR monitor, abandoned_branch_check, covers spec-edit/* branches only and only notifies); the probe must fail on the current tree by finding the MR still open, and pass only when the MR is closed with an authored explanation.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "interrupt-ledger-hardening-gate",
        "title": "Interrupt ledger and environment-first retry: record every agent error signal, block blind re-spawns of recurring unhardened error classes, require structural fixes",
        "covers": [
          "5d7c43ad773e272f9c6f6e44"
        ],
        "required_behavior": "Per specs/development/agent-workflow.md §Fix the Environment, Not the Agent: when an agent hits an error the first response is never 'try again' — the root cause is diagnosed and the environment engineered (lint rule, pre-commit hook, better error message, test fixture) before the retry, with an interrupt ledger recording every error as a signal so recurring classes are structurally fixed once. Today the Phase 7 fail path (ralph_loop.rs) closes the MR and re-spawns the agent with an identical environment (same gates, hooks, tooling), the protocol prompt says 'fix, commit, push, complete again', and no error ledger exists anywhere ('interrupt ledger' appears only in the spec file). Implement: (1) Interrupt ledger — a new gyre-ports trait (e.g. interrupt_ledger.rs: create, find_by_class/recurring queries, record hardening action, list by task/repo) implemented by the SQLite adapter (next unused migration sequence number — current latest is 000066), the PostgreSQL adapter, and the mem adapter with every port failure constraint guarded identically in all adapters (scripts/check-mem-port-contracts.sh), wired into AppState via the existing store! pattern (scripts/check-in-memory-state-stores.sh — no bare Arc<Mutex<HashMap>>). Entry: tenant/workspace/repo/task/agent ids, error class signature (normalized identity of the failure: failing gate id/type plus a failure fingerprint such as the gate command and exit code / leading failure output — stable across iterations of the same class), iteration, occurred_at. DB-backed so it survives restart. (2) Recording — on the Phase 7 gate-failure path (and the agent fail path), write a ledger entry with the computed class signature before the respawn decision. (3) Environment-first enforcement — before re-spawn, query the ledger for the class signature's occurrence count for the task; if the class has recurred (>= 2 occurrences, threshold configurable) with NO recorded hardening action for that class, do NOT re-spawn: transition the task to Blocked with a reason naming the recurring error class and its evidence, and create a priority-1 escalation notification to workspace admins requiring an environment fix — this is the enforcement that makes 'first response is never try again' real. (4) Hardening record — a scoped API/MCP surface (e.g. POST environment-hardenings / a gyre_record_environment_hardening MCP tool) recording {class signature, description, evidence (commit SHA, added gate/lint rule/hook, improved error message, test fixture), recorded_by}; new routes get ABAC RouteResourceMapping entries (scripts/check-abac-route-registry.sh), per-handler scoped authorization where exempt (scripts/check-abac-exempt-handlers.sh), and scope fields derived from the caller (scripts/check-forged-scope-fields.sh). Once a hardening record exists for the class, respawn proceeds. (5) Respawn context — when respawn is allowed, the new agent's failure context (inbox/system://context messages) must lead with the diagnosis directive: identify the root cause and engineer the environment to catch/prevent that error class (lint rule, hook, better error message, test fixture) as part of the fix, before retrying. (6) Protocol prompt — amend the on-gate-failure instruction in crates/gyre-server/src/api/prompts.rs from 'fix, commit, push, complete again' to require root-cause diagnosis and environment hardening before the retry, per this canonical spec section. A ledger that is written but never gates the respawn decision is audit-only theater and does not satisfy this task.",
        "acceptance": [
          "Ledger persistence and content: a gate failure on a Ralph-loop agent writes a ledger entry carrying the task/agent/repo scope, iteration, and a class signature that is identical across two failures of the same gate with the same failure mode (and different for a different gate/failure mode); entries remain readable after a server restart.",
          "Environment-first gate: a class that has failed twice on one task with no hardening record produces, on the next failure, NO re-spawn — the task is Blocked with a reason naming the recurring class, a priority-1 escalation notification to workspace admins exists, and the agent's iteration count is not incremented further.",
          "Hardening unblocks: after recording a hardening action for the class (with description and evidence), a subsequent failure re-spawns the agent, and the respawned agent's failure context leads with the diagnose-and-harden-the-environment directive.",
          "Adapters: the port's failure constraints are enforced identically in the SQLite, PostgreSQL, and mem adapters (scripts/check-mem-port-contracts.sh passes with no new exemptions), and AppState wiring uses the store! pattern (scripts/check-in-memory-state-stores.sh).",
          "The hardening-record endpoint/tool enforces caller scoping: a caller outside the entry's workspace cannot create or read another workspace's hardening records (Forbidden)."
        ],
        "negative_probe": "Configure a repo whose test gate fails deterministically with the same output signature, let the loop iterate past the recurrence threshold with no hardening record, then probe: assert the agent was NOT re-spawned again (no new active session, iteration frozen, task Blocked, escalation notification present). The real defect this must catch: the current tree re-spawns unconditionally on every failure until max_iterations with an environment that never changes — on the current tree the probe finds iteration still climbing and no escalation, and fails. Second probe killing the audit-only hollow version: comment out the respawn gate (ledger written but decision unchanged) — the same probe must fail, proving the ledger result actually gates the decision rather than merely recording it.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "pre-pr-local-verification": "task-268",
    "terminal-worktree-cleanup": "task-269",
    "pr-protection-and-hygiene": "task-270",
    "interrupt-ledger-hardening-gate": "task-271",
    "_publication": "task-272",
    "_base": "25497b9e3cc16c7e405c641870ff3c3b1905cf6b"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.

### Reconciliation round

The published branch conflicted with main (task-274's admin-panel backlog
publication had landed and appended entries to the same
`specs/task-spec-inputs.json` regions). Resolved as merge commit
`249facf13d83495d325ff401eb81b616d61d848a` (parents: candidate `2459036a` +
main `ee03c299`): both conflicts were purely additive, so the resolution keeps
this branch's four agent-workflow backlog sections and spec-gap bindings and
main's admin-panel section and binding — the union of both sides. All task
documents (task-268 through task-274), registry entries, coverage statuses, and
product files from both sides are preserved; no product behavior changed.

### Verification evidence

- Resolved `specs/task-spec-inputs.json` parses as JSON; all five newly added
  keys (four agent-workflow, one admin-panel) present; every binding target
  resolves to a section entry (no dangling references).
- `git diff ee03c299` on the registry shows only this branch's four additions —
  no deletions or rewrites of main's entries; final tree vs main differs by
  exactly the six task-272 publication files (628 insertions, 0 deletions).
- Canonical spec `specs/development/agent-workflow.md` is byte-identical to
  both merge parents; task-268..271 remain `not-started` and identical to the
  independently reviewed plan.
- Evidence retained at `/tmp/stage/review-evidence/task-272-reconcile.txt`.
