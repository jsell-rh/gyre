# The Ralph Loop

> **Status: Superseded.** This spec is superseded by [`agent-runtime.md`](agent-runtime.md) §1 (Agent Lifecycle) and §5 (Agent Prompt Structure). All cross-references should point to `agent-runtime.md`.

## Origin

The Ralph (Wiggum) loop is a technique for autonomous software implementation. In its simplest form:

```bash
while :; do cat PROMPT.md | agent ; done
```

A single prompt is run repeatedly in a loop. Each iteration gets a fresh context window. State persists in files and git commits, not in agent memory. The loop terminates when a condition is met (tests pass, review approved). The technique was named by OpenAI's Codex harness team and popularized by Geoffrey Huntley.

The Ralph loop solves context rot: long-running agent sessions degrade as the context window fills with stale tool results. By giving each iteration a fresh context and persisting state to durable storage, the agent's reasoning quality stays constant across hundreds of iterations.

## Position in the SDLC

Gyre's autonomous SDLC has three distinct phases with different primitives:

```
Design (human + specs)  -->  Decomposition (orchestrator)  -->  Implementation (Ralph loops)
```

**Design** is the spec lifecycle. Humans write and approve specs. Meta-specs encode organizational judgment -- personas, principles, coding standards. This is where direction is set. It involves human judgment and is not a Ralph loop. See: vision.md, spec-lifecycle.md, meta-spec-reconciliation.md.

**Decomposition** is the orchestrator's job. The repo orchestrator reads approved specs, decomposes them into tasks with clear acceptance criteria, and delegates tasks to worker agents. See: platform-model.md (Two-Level Orchestration).

**Implementation** is where the Ralph loop lives. A task exists. An agent is created for it. The agent runs sessions -- each with a fresh context window -- until the task is done. Each session reads the codebase, the task's spec refs, and any messages in the agent's inbox, does work, commits, and exits. The server checks terminal conditions. If not met, a new session starts. The loop iterates until gates pass and the reviewer approves.

The Ralph loop is the implementation primitive -- the thing that turns a task into working, reviewed, tested code.

## Core Concepts

### Agent vs. Session

An **agent** is a persistent identity. It has an inbox, a task assignment, a worktree, and a history of sessions. An agent is created when a task is delegated, and it persists until the task converges or is abandoned.

A **session** is one ephemeral execution of an agent. One context window, one fresh start. A session reads from the agent's inbox and the worktree, does work, commits, and exits. The context window is discarded.

A **Ralph loop** is one agent running multiple sessions until convergence. The loop is the relationship between the agent (durable) and its sessions (ephemeral).

```
Agent (persistent)
  |-- inbox: [Message]           # gate results, review feedback, orchestrator directives
  |-- task_id: Id                # one agent, one task
  |-- persona_id: Id             # frozen at creation
  |-- prompt: String             # frozen at creation
  |-- worktree: branch           # persistent across sessions
  |-- loop_config: LoopConfig    # terminal conditions, max_iterations
  |-- iteration: u32             # current iteration count
  |-- status: AgentStatus        # active, converged, failed, stopped
  |-- sessions: [Session]        # execution history
       |-- Session (ephemeral)
            |-- started_at, ended_at
            |-- cost, tokens_used
            |-- messages_read: [MessageId]
```

### The Message Inbox

Every agent has a message inbox. Messages are how state flows between sessions and how the platform communicates with agents.

Message types:

| Type | Source | Content |
|---|---|---|
| `GateResult` | Gate runner | Gate name, pass/fail, output (error messages, test failures) |
| `ReviewFeedback` | Reviewer agent | Approve/reject with specific feedback |
| `OrchestratorDirective` | Orchestrator | Priority change, scope adjustment, abort |
| `SystemEvent` | Platform | Budget warning, spec change notification |

Messages have a read/ack lifecycle:
1. Message arrives in inbox (unread)
2. Next session starts, reads all unread messages
3. Session processes messages (addresses feedback, responds to failures)
4. Session marks messages as acknowledged
5. Acknowledged messages remain in inbox for audit but are not re-delivered

This replaces plan files, review feedback files, and the gate failure WebSocket timing problem. Gate failures don't need a live session -- they write to the inbox. The next session reads them.

### One Agent, One Task

A Ralph loop exists to complete a single task. The task has:
- Acceptance criteria (what "done" means)
- Spec refs (the design intent behind the task)
- A branch/worktree (where the work happens)

The agent iterates -- session after session -- making progress toward the task's acceptance criteria. There is no plan file. The task IS the plan. The specs define what to build. The acceptance criteria define when it's done. The agent reads these, reads the codebase, and works toward completion.

Multiple tasks means multiple agents means multiple concurrent Ralph loops. The orchestrator manages this concurrency -- sequencing tasks with dependencies, parallelizing independent ones, and detecting conflicts.

## Properties

### 1. Same prompt, every session

The prompt is frozen when the agent is created. It does not change between sessions. What changes is: the codebase (via commits), the inbox (via messages), and the worktree state. The prompt tells the agent HOW to work. The task, specs, and messages tell it WHAT to work on and what feedback to address.

### 2. Fresh context, every session

Each session starts with an empty context window. There is no conversation history carried forward. This is a feature, not a limitation. Context rot -- the degradation of agent reasoning as the context window fills -- is the primary failure mode of long-running agent sessions. Fresh context eliminates it architecturally.

### 3. Durable state between sessions

State persists in three places:

- **Git commits**: Each session that produces valid work ends with a commit. Commits are the primary checkpoints. If the server crashes, the loop resumes from the last commit.
- **The message inbox**: Gate results, review feedback, and orchestrator directives accumulate in the inbox. Each session reads unread messages and addresses them.
- **The worktree**: The branch state reflects all prior sessions' work. Each new session reads the current code, not some cached version.

The agent's context window is ephemeral. Everything else is durable.

### 4. Terminal conditions

A loop terminates when ALL configured terminal conditions are met:

**Gates (always enforced):** All quality gates configured on the repo must pass on the loop's branch. This includes test commands, lint commands, architecture checks, and any other configured gates. Gates are the floor -- they are never optional. Gate results are delivered as messages to the agent's inbox.

**Agent review (default, opt-out):** After gates pass, a separate reviewer agent evaluates the accumulated work. The reviewer has fresh context and a review-oriented persona. If the reviewer rejects, their feedback is delivered as a message to the implementing agent's inbox. The next session reads the feedback and addresses it. If the reviewer approves, the terminal condition is met.

Agent review is enabled by default because:
- Self-review is insufficient -- same biases, same blind spots
- Fresh context on the reviewer prevents rubber-stamping
- The reviewer persona encodes repo-specific quality standards via meta-specs
- It implements the "no shortcuts" principle

Agent review can be disabled for specific agents (e.g., simple formatting tasks), but the default is on.

**Custom conditions (optional):** Additional terminal checks:
- Command exit code (e.g., `cargo test --features=integration`)
- Manual human approval for sensitive changes

### 5. Git commit as session boundary

Each session that produces valid work ends with a git commit. This means:
- Every session's work is individually revertable
- The loop's convergence is visible in the git log
- If the server crashes, the last commit is the resume point
- Humans can watch the loop converge by reading commit history

### 6. Max iterations as safety net

Every Ralph loop has a `max_iterations` limit. This is a safety net, not a design target. If an agent reaches max iterations without converging, the loop stops and the orchestrator is notified. The orchestrator diagnoses: is the task too large? Is the spec unclear? Is the agent persona wrong? It then re-decomposes, adjusts, or escalates to a human.

## Integration with Gyre

### Agent lifecycle

```
1. Orchestrator decomposes spec into tasks
2. For each task, orchestrator creates an agent:
   - Assigns the task (one agent, one task)
   - Sets the persona and prompt (frozen for the loop's lifetime)
   - Configures terminal conditions (gates + review + custom)
   - Sets max_iterations
   - Creates a worktree/branch for the task
3. Loop begins:
   a. Server starts a new session for the agent
   b. Session reads: inbox messages, task spec refs, codebase state
   c. Session does work, commits
   d. Session ends (context discarded)
   e. Server runs gates on the branch
   f. Gate results delivered as messages to agent inbox
   g. If gates fail: go to 3a (next session addresses failures via inbox)
   h. If gates pass AND agent_review enabled:
      - Reviewer agent spawned with reviewer persona
      - Reviewer reads branch diff, spec refs, task acceptance criteria
      - Reviewer delivers verdict as message to agent inbox
      - If rejected: go to 3a (next session addresses review feedback)
      - If approved: loop converges
   i. If gates pass AND no review (or review approved):
      - Check custom terminal conditions
      - If all met: loop converges
4. On convergence: mark task done, open MR automatically
5. On max_iterations: mark task blocked, notify orchestrator
```

### Spawn API

The Ralph loop is activated when creating an agent:

```
POST /api/v1/agents/spawn
{
    "task_id": "TASK-42",
    "repo_id": "...",
    "persona_id": "...",
    "prompt": "...",
    "loop": {
        "agent_review": true,
        "reviewer_persona_id": "...",
        "max_iterations": 50,
        "max_review_rejections": 5
    }
}
```

When `loop` is present, the server manages the session cycle automatically. When absent, the agent runs a single session (backward-compatible with existing behavior).

### Orchestrator interaction

The repo orchestrator creates tasks and spawns agents for them. Its job:

1. Read approved specs
2. Decompose into tasks with acceptance criteria
3. For each task, create an agent with an appropriate prompt and persona
4. Monitor agent convergence (via agent status)
5. When an agent converges, verify the MR and merge
6. When an agent fails (max iterations), diagnose and re-create or escalate
7. Manage concurrency -- parallelize independent tasks, sequence dependent ones

The orchestrator itself may run as a Ralph loop: an agent that reads spec state, creates tasks, monitors other agents, and iterates until all specs are implemented. Meta Ralph loops -- loops that manage loops -- fall out naturally from this design.

### Concurrent loops

Multiple tasks mean multiple agents running concurrently. This is the normal operating mode. The orchestrator is responsible for:

- **Sequencing dependent tasks**: Task B depends on Task A's interface definition. B's agent doesn't start until A converges.
- **Parallelizing independent tasks**: Tasks touching different modules run simultaneously.
- **Conflict detection**: If two agents modify the same files, the orchestrator sequences them or assigns non-overlapping worktrees.
- **Branch management**: Each agent works on its own branch. Merges flow through the merge queue.

Concurrent loops are not a special feature -- they are simply multiple tasks delegated by the orchestrator to multiple agents.

### Reviewer agent behavior

When agent review is enabled, the reviewer is spawned as a separate agent after gates pass. The reviewer:

1. Reads the branch diff (all accumulated changes across sessions)
2. Reads the spec refs from the task
3. Reads the task acceptance criteria
4. Evaluates: does the implementation satisfy the spec? Are there bugs, gaps, or quality issues?
5. Delivers a verdict as a message to the implementing agent's inbox:
   - **Approve**: work satisfies the spec and quality standards
   - **Reject**: specific, actionable feedback

The reviewer is a single-pass evaluation -- it does not iterate. If it rejects, the implementing agent's next session reads the feedback and addresses it. After the next session, gates re-run, and if they pass, the reviewer is spawned again.

To prevent reviewer-worker oscillation, a `max_review_rejections` parameter caps how many times the reviewer can reject before the loop escalates to the orchestrator.

The reviewer uses a different persona than the worker. Reviewer personas are typically defined as meta-specs -- organizational standards for code quality, spec compliance, and review rigor.

### Error recovery

**Session crash (before commit):** Dirty worktree state may be present. The server starts the next session from the last committed state. Uncommitted work is lost, which is acceptable -- sessions are designed to be small and atomic.

**Budget exhaustion mid-session:** The platform delivers a `BudgetWarning` message to the inbox. If the budget is hard-exceeded, the session is terminated. The next session reads the budget warning and can adjust scope or signal to the orchestrator.

**Server crash:** On restart, the server finds all agents with `status: active` and `loop_config` present. For each, it resumes the loop from the current worktree state (last commit). No special recovery logic needed -- the loop is designed to be resumable from file state.

**Max iterations reached:** The agent is marked `failed` and the orchestrator is notified. The orchestrator's diagnostic framework:
- Is the task too large? Re-decompose into smaller tasks.
- Is the spec unclear? Escalate to human for spec clarification.
- Is the persona wrong? Try a different persona.
- Is the codebase illegible? Spawn a cleanup task first.

## Provenance Integration

The Ralph loop does not define its own commit tracking. It relies on the existing provenance chain, which already provides full traceability from agent to code to release:

- **Git refs**: `refs/agents/{agent-id}/head` records the HEAD at spawn. `refs/agents/{agent-id}/snapshots/{n}` records immutable branch snapshots at each session completion. `refs/tasks/{task-id}` marks the task's implementation branch.
- **Agent-commit mapping**: The forge records which agent authored each commit via `POST /api/v1/repos/{id}/commits/record`. Commits made during a session are attributed to the agent, and by extension to the session's time window.
- **Workload attestation**: The agent's JWT embeds `wl_pid`, `wl_hostname`, `wl_compute_target`, `wl_stack_hash`, and (for container agents) `wl_container_id`, `wl_image_hash`. This proves WHERE and HOW the session ran.
- **Stack attestation**: At push time, the full agent stack fingerprint (persona hash, model, CLI version, hooks, MCP servers) is signed and verified against the repo's `gyre-stack.lock`. This proves WHAT configuration produced the code.
- **Merge attestation**: When an MR merges, a signed `MergeAttestation` is attached as a git note to the merge commit, recording gate results, spec refs, and author agent.
- **AIBOM**: At release time, the AI Bill of Materials aggregates per-commit agent attribution, stack attestation levels, and policy compliance into a single auditable artifact.

The Session model tracks execution metadata (timing, cost, messages read). The provenance chain tracks code attribution (commits, attestation, verification). These are complementary, not overlapping. A session's commits are discoverable by querying the agent-commit mapping for the agent ID within the session's time window.

This design means the Ralph loop inherits the full supply-chain security guarantees (supply-chain.md) without duplicating any tracking infrastructure.

### Required code changes (RalphStep removal)

The existing codebase has a `RalphStep` enum (`Spec | Implement | Review | Merge`) baked into the provenance chain. This 4-phase model is incompatible with the Ralph loop as defined in this spec, where:
- **Spec** is human-driven, not an agent phase
- **Implement** is the only agent activity (the Ralph loop itself)
- **Review** is a terminal condition check, not a commit phase
- **Merge** is automated by the merge queue

The following changes are required:

| What | Change | Rationale |
|---|---|---|
| `RalphStep` enum (`gyre-domain`) | **Remove entirely** | Task status already tracks what the agent is doing |
| `ralph_step` column on `agent_commits` | **Drop** (migration) | Redundant -- `task_id` on the commit + task status history gives the same information |
| `find_by_ralph_step` port method | **Remove** | Use `find_by_task_id` instead (already exists) |
| `refs/ralph/{task-id}/implement` | **Rename to `refs/tasks/{task-id}`** | Simpler, matches domain language, no phase suffix needed |
| `?ralph_step=` provenance query filter | **Remove** | Use `?task_id=` filter instead (already exists) |
| AIBOM `ralph_step` attestation heuristic | **Replace** | Use `stack_hash` or `wl_*` JWT claims to determine attestation level |
| `ralph_step` derivation in `git_http.rs` | **Remove** | Was translating task status → ralph step at push time; no longer needed |

All provenance queries currently served by `ralph_step` are already served by `task_id` combined with the task's status transition history.

## Quality Standards and Meta-Specs

The Ralph loop is deliberately opinionated about structure (fresh context, message inbox, gates + review) but deliberately unopinionated about implementation standards. Standards like:

- "No placeholder implementations"
- "Write tests for every change"
- "Self-review before committing"
- "Capture the importance of tests in documentation"
- "Search before assuming something isn't implemented"

These are NOT platform features. They are **meta-specs** -- organizational standards encoded as personas, principles, and coding standards in the meta-spec corpus. They shape the agent's prompt and the reviewer's evaluation criteria. They are data, not code.

This is by design: different organizations have different standards. A startup may want speed over thoroughness. An enterprise may want exhaustive testing. A security-critical project may require formal verification. The platform provides the loop; meta-specs provide the opinion about what good looks like.

## Relationship to Other Specs

| Spec | Relationship |
|---|---|
| vision.md | The Ralph loop implements Principle 5 (the feedback loop is everything) at the agent level. It is the innermost loop in the system. The human feedback loop (Observe -> Understand -> Decide -> Encode -> Execute -> Observe) drives spec creation. The Ralph loop drives the Execute step. |
| platform-model.md | Ralph loops run within the repo orchestrator's scope. The orchestrator decomposes specs into tasks and creates agents (Ralph loops) for them. Workspace orchestrators coordinate cross-repo work by managing task dependencies across repos. |
| agent-gates.md | Gates are the always-on terminal condition. In the Ralph loop model, gate results are delivered as messages to the agent's inbox rather than as live WebSocket events. The agent is not alive when gates run -- it reads results in the next session. |
| spec-lifecycle.md | Specs are the primary input to Ralph loops via task acceptance criteria. Agents may draft spec changes (e.g., cross-repo escalation), but human approval is always required before a spec change takes effect. The approval gate is the control point. |
| meta-spec-reconciliation.md | Meta-specs shape the prompts, personas, and reviewer standards used in loops. A loop's prompt includes a specific persona version. If the persona is updated mid-loop, the loop continues with the version it was created with (consistent with platform-model.md: "agents run with the version they were spawned with"). |
| design-principles.md | "Engineer the Ralph Loop" -- this spec defines what that means. "Reconciliation as a primitive" -- the loop is convergence toward a desired state (gates pass + review approved). "Single-minded agents" -- one agent, one task. "No shortcuts" -- agent review is on by default. |
| supply-chain.md | The Ralph loop inherits the full provenance chain: stack attestation, agent-commit mapping, workload attestation, merge attestation, and AIBOM. Sessions do not duplicate commit tracking -- they track execution metadata while the provenance chain tracks code attribution. |

### Supersedes

This spec supersedes:
- `specs/development/ralph-loops.md` -- replaced by this comprehensive definition
- `specs/development/manager-agent.md` -- the Manager Agent concept is split into the orchestrator (platform-model.md) and the Ralph loop (this spec)

## What Gyre Adds

The raw `while :; do cat PROMPT.md | agent; done` works for solo developers on greenfield projects. Gyre turns it into a managed, observable platform primitive:

| Raw loop | Gyre Ralph loop |
|---|---|
| Manual prompt file | Frozen prompt from persona + task |
| No inter-iteration communication | Message inbox (gate results, review feedback, directives) |
| Manual terminal check | Automated gate execution + agent review |
| No crash recovery | Server resumes from last commit on restart |
| No budget limits | Per-session and total budget enforcement |
| No observability | Each session tracked: logs, duration, cost, commits |
| No review | Default agent review with configurable reviewer persona |
| Manual MR creation | Automatic MR on convergence |
| Solo developer watches | Orchestrator monitors multiple concurrent loops |
| `git reset --hard` on failure | Server manages branch state, escalation, and diagnostics |
| Standards in the prompt | Standards as meta-specs (data, not code) |

## Open Questions

1. **Reviewer as agent or session?** The current design spawns the reviewer as a separate agent. An alternative: the reviewer could be a special session of a dedicated review agent that handles multiple tasks. This would give the reviewer its own inbox and history.

2. **Partial convergence**: If a loop partially satisfies acceptance criteria (3 of 5 met) but cannot make further progress, should it converge with reduced scope or escalate? Leaning toward escalate -- partial convergence creates debt.

3. **Loop-to-loop handoff**: When one loop's output is another loop's input (e.g., interface definition in repo A, implementation in repo B), coordination is the workspace orchestrator's responsibility. This spec does not define cross-repo loop coordination.

4. **Orchestrator loop config**: Should the orchestrator's own Ralph loop have different defaults (e.g., no agent review, higher max_iterations)? Or should it use the same config as worker loops?
