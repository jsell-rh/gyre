# Forge Advantages: Why Gyre Owns the Source Control Layer

Gyre is not integrated with a code forge -- it _is_ the forge. This document defines eight
capabilities that only a forge-native agent platform can provide, explains why each is
impossible with an external forge, and shows how each accelerates the Ralph loop.

---

## The Core Insight

External forges (GitHub, GitLab) are built for humans. Every capability surfaces through webhooks
and REST APIs: push arrives, forge calls out, your system responds, adds a status check, maybe
triggers a CI run. Each step adds latency, failure modes, and configuration surface. Agents running
tight feedback loops at machine speed pay this tax on every iteration.

Gyre eliminates the seam. The forge, the agent runtime, and the identity system share a process.
Post-receive hooks are function calls. Commit metadata is a foreign key. The merge queue can talk
directly to an agent's message queue. No webhooks. No eventual consistency.

---

## Capability 1: Speculative Merging

**What it does:**
The merge queue continuously attempts to merge every in-flight feature branch against the current
main, creating ephemeral merge commits in `refs/speculative/{branch}`. Conflicts are detected the
moment they arise -- not when an agent pushes their final commit.

**Why impossible externally:**
An external forge merges on demand. You can simulate conflict detection via CI, but that requires:
a custom runner, a webhook trigger, clone + merge + report in a separate process, and a status
check round trip. Each step adds 10-30 seconds. At agent commit frequency (multiple pushes per
minute), this is untenable.

Gyre runs speculative merging as a background task in the same process. It shares the in-memory
git object store. The merge attempt costs microseconds, not seconds.

**Ralph loop acceleration:**
Conflict notification arrives before the agent finishes -- often before the agent begins the
implementation phase. The agent can redirect effort immediately rather than discovering the conflict
at MR submission time and having to rebase or rewrite.

**Agent identity integration:**
Speculative merges are attributed in the conflict report: "your branch conflicts with changes made
by agent `worker-42` on task TASK-019 at commit `abc123`." The agent can message the conflicting
agent directly to coordinate. No detective work required.

---

## Capability 2: Atomic Agent Operations

**What it does:**
`POST /api/v1/agents/spawn` is a single transaction:
1. Create agent record
2. Mint per-agent OIDC token (scoped to task + repo)
3. Provision git worktree
4. Assign task, mark in_progress
5. Record initial branch ref

If any step fails, all prior steps roll back. The agent either starts clean or does not start.
Similarly, `POST /api/v1/agents/{id}/complete` atomically opens the MR, marks the task done,
removes the worktree, and marks the agent idle.

**Why impossible externally:**
An external forge cannot participate in transactions that span your application database, the git
object store, and identity token issuance. Each is a separate API call. Partial failures produce
zombie states: worktree exists but agent record is missing, or MR is open but task is still
in_progress. These require manual cleanup and cause subtle bugs in downstream coordination.

**Ralph loop acceleration:**
The orchestrator can spawn hundreds of agents per minute without building complex compensating
logic. When the loop restarts after a failure, the state is clean. No "is this agent actually
running?" diagnostic step required.

**Agent identity integration:**
The spawn transaction generates and embeds the OIDC token in the same operation that records the
agent session. The token `sub` claim (`agent:{name}`) and the session record share the same
UUID -- no out-of-band key distribution, no secret storage.

---

## Capability 3: Zero-Latency Feedback

**What it does:**
Post-receive hooks in Gyre are synchronous function calls in the server process. When an agent
pushes a commit, Gyre:
1. Validates the push (auth, branch policy, pre-accept checks)
2. Updates agent-commit tracking records
3. Evaluates merge queue readiness
4. Broadcasts domain events to all WebSocket clients
5. Returns the push result

All of this happens before the `git push` command returns to the agent's shell.

**Why impossible externally:**
GitHub's post-receive equivalent is a webhook, which is asynchronous. The push completes, GitHub
calls your endpoint some seconds later. During that window, the agent may already be doing more
work based on stale state. "Did my push trigger CI?" requires polling or subscription. The feedback
round trip is 2-15 seconds minimum.

**Ralph loop acceleration:**
The agent knows immediately whether its push was accepted, whether it triggered gate checks, and
whether it is now first in the merge queue. The spec-implement-review-merge loop closes in seconds
rather than minutes.

**Agent identity integration:**
The post-receive hook has direct access to the authenticated agent session. Push validation
uses the agent's OIDC token claims (task scope, repo scope) without an additional API call.
Sigstore signing can be initiated inline, ensuring every accepted commit is signed before the ref
is updated.

---

## Capability 4: Server-Side Pre-Accept Validation

**What it does:**
Gyre can reject a push before the ref is updated. Pre-accept checks run in the receive-pack
handler before the packfile is written to disk. Default checks:
- Architecture lint: no `gyre-domain` importing `gyre-adapters`
- Conventional commit format validation
- No em-dash characters (agent-specific style rule)
- Spec reference: commit message references a known TASK-id if branch is `feat/` or `fix/`
- Format: `cargo fmt --check` on changed `.rs` files (configurable)

Additional checks are pluggable per-repo via gate definitions (M12.1).

**Why impossible externally:**
GitHub pre-receive hooks exist but run in an isolated sandbox with a time limit, no network access,
and no access to your application database. You cannot enforce "commit message must reference a
known task" because the task database is external. You cannot enforce architectural rules that
require understanding your codebase structure unless you ship a self-contained binary into the
sandbox.

**Ralph loop acceleration:**
Agents get immediate, specific rejection messages with actionable fixes. No CI round trip to
discover that the commit failed a lint check. The agent corrects and re-pushes in seconds.

**Agent identity integration:**
Pre-accept checks are ABAC-aware: different rules apply based on agent role and task scope.
An `Admin` agent may bypass certain checks. A `ReadOnly` agent is rejected on any write attempt.
Pre-accept is the enforcement point for the identity stack -- by the time a commit is accepted,
its provenance is verified.

---

## Capability 5: Rich Commit Provenance

**What it does:**
Every accepted commit in Gyre is linked by foreign key to:
- Agent session ID (who pushed it)
- Task ID (what it was for)
- Ralph loop step (spec / implement / review / merge)
- User ID (who spawned the agent)
- Model context snapshot (which model, at what revision of the spec, with what tools active)
- Worktree path at time of push
- Parent agent session (orchestrator that spawned this agent)

This data is queryable via API:
- "Show all commits made during the implement phase of TASK-007"
- "Show all commits made by agents spawned by user jsell this week"
- "Show the full commit lineage for this feature, including review iterations"

**Why impossible externally:**
External forges store commit metadata in git objects (author, message, timestamp). Custom metadata
requires commit message conventions (fragile) or out-of-band databases that drift from the actual
commit history. Querying "all commits by this agent" requires grepping messages or maintaining your
own index that can fall out of sync.

Gyre writes provenance as first-class database records, written transactionally with ref updates.
They cannot drift.

**Ralph loop acceleration:**
Orchestrators can reconstruct exactly what happened during a failed loop without reading logs.
"Which commit introduced the failing test?" is a database query, not a bisect. Review routing uses
provenance: if a file was last touched by agent `worker-7`, that agent (or its orchestrator) is
notified first.

**Agent identity integration:**
Provenance is the Sigstore transparency record made queryable. The Sigstore signature proves "this
commit was made by this OIDC identity"; the provenance record links that identity to the full
task/agent/loop context. Together they form an unbroken, tamper-evident chain from user intent
(task creation) to merged commit.

---

## Capability 6: Custom Ref Namespaces

**What it does:**
Gyre manages structured git ref namespaces beyond `refs/heads/` and `refs/tags/`:

| Namespace | Purpose |
|---|---|
| `refs/agents/{agent-id}/head` | Current working branch per agent |
| `refs/agents/{agent-id}/snapshots/{n}` | Point-in-time snapshots for undo |
| `refs/tasks/{task-id}` | Branch tip for this task's implementation |
| `refs/speculative/{branch}` | Speculative merge result (ephemeral) |
| `refs/reviews/{mr-id}/comments` | Review comments as git notes |
| `refs/queue/{entry-id}` | Merge queue state per entry |

These refs are managed by the server, not by agents. Agents push to `refs/heads/` normally.

**Why impossible externally:**
External forges support custom refs but provide no semantic meaning or lifecycle management for
them. You can push to `refs/custom/foo` on GitHub, but GitHub will not garbage-collect stale
entries, will not update them atomically with your application state, and will not expose them
through discovery APIs.

**Ralph loop acceleration:**
An orchestrator can reconstruct the full history of any task by reading structured refs -- no
database query required. `git fetch origin refs/tasks/TASK-007` retrieves the task's branch tip.
Combined with `refs/agents/{agent-id}/snapshots/*`, the full session-by-session history is
reconstructable from git alone.

**Agent identity integration:**
Ref namespace writes are gated by the agent's OIDC token scope. An agent may write to
`refs/agents/{its-own-id}/` but not to another agent's namespace. The `refs/tasks/` namespace is
written by the server at spawn time, not directly by agents -- enforced by the
receive-pack pre-accept layer.

---

## Capability 7: jj as Native Agent VCS

**What it does:**
Jujutsu (jj) is the agent-facing VCS layer colocated on top of git. Every agent tool execution
maps to an atomic jj change:
- "read file" does not create a change
- "write file" creates or extends the current anonymous change
- "run tests" creates a new change if tests pass (marking a stable checkpoint)
- "apply suggestion" is its own change with the suggestion as description

The jj operation log records every operation, including failed ones. `jj undo` reverts to any
prior state. Multi-agent rebasing is handled by jj's automatic rebase-on-top-of-diverged-parent.

**Why impossible externally:**
Standard git requires explicit staging and committing. Agents using git directly either commit too
rarely (large, opaque commits) or too often (noisy, meaningless commits). jj's anonymous change
model matches agent execution semantics naturally.

External forges do not understand jj's operation log. If an agent's session crashes mid-change,
recovery requires reading jj state, which an external forge cannot assist with. Gyre's server
understands jj operation log format and can assist with recovery during agent spawn/complete.

**Ralph loop acceleration:**
Agents can experiment freely -- every file write is undoable. Review feedback maps directly to
`jj describe` and targeted file edits. The "apply review, checkpoint, continue" pattern has zero
overhead. Review history is preserved in the operation log, available for audit.

**Agent identity integration:**
jj change descriptions are generated by the server using the agent's task context:
```
feat(TASK-007): implement rate limiting middleware

Agent: worker-42
Session: sess_abc123
Spawned-by: user:jsell
Loop-step: implement/iteration-2
Signed-by: fulcio:worker-42@gyre.example.com
```

Sigstore signing is applied at `jj squash` time (when a change is finalized), not per write.
This avoids signing ephemeral intermediate states while ensuring every landed commit is signed.

---

## Capability 8: Cross-Agent Code Awareness

**What it does:**
Gyre provides real-time queries across all agent activity:

- `GET /api/v1/repos/{id}/blame?path=src/foo.rs` -- returns per-line agent attribution (not just
  last commit author, but full agent session + task context)
- `GET /api/v1/repos/{id}/coverage?agent_id=X` -- cumulative diff: all lines ever touched by agent X
- `GET /api/v1/repos/{id}/hot-files` -- files with the most concurrent agent activity (conflict risk)
- `GET /api/v1/agents/{id}/touched-paths` -- all file paths an agent has written to in its session
- `GET /api/v1/repos/{id}/review-routing?path=src/foo.rs` -- which agents/agents most recently
  changed this path and should be notified for review

These queries combine git history with the provenance database (Capability 5) to answer questions
no git command can answer natively.

**Why impossible externally:**
This requires joining git history (who changed this line?) with application-level metadata (which
agent, on which task, in which loop step?) in real time. External systems must maintain their own
index and keep it synchronized with git refs. Drift is unavoidable under high commit frequency.

Gyre maintains the index transactionally: every accepted push updates the provenance records
atomically with the ref update. The index is always consistent with git state.

**Ralph loop acceleration:**
Review routing is automatic. When an agent opens an MR, Gyre identifies which other agents have
touched the changed files and routes review notifications accordingly. No manual `@mention` or
CODEOWNERS file required. Conflict risk assessment is available before agents start: "this file has
3 agents working on it -- coordinate before writing."

**Agent identity integration:**
Code awareness queries respect ABAC policies. An agent can query coverage for its own session
without elevated permissions. Querying another agent's coverage requires `Developer` role or higher.
Hot-file alerts are emitted as domain events to all active agent WebSocket sessions -- agents
self-coordinate without orchestrator intervention.

---

## Agent Identity Integration Summary

All eight capabilities build on the same three-layer identity stack:

| Layer | Role in Forge Capabilities |
|---|---|
| **SPIFFE** (workload attestation) | Proves the agent process is running on authorized compute. Pre-accept hooks verify SVID before accepting any push. Speculative merge results are only shared with agents whose SVID matches the owning session. |
| **Gyre OIDC** (agent permissions) | Scopes every operation to task + repo. Atomic spawn mints the token. Pre-accept validation reads token claims inline. Custom ref namespace writes are gated by token scope. Cross-agent queries enforce ABAC from token `role` claim. |
| **Sigstore/Fulcio** (commit signing) | Every accepted commit carries a Fulcio certificate linking the commit to the OIDC identity. Provenance records embed the Rekor log index. jj squash triggers signing at finalization. The full chain -- user intent -> task -> agent session -> commit -> signature -> transparency log -- is queryable via the provenance API. |

**Zero-trust code host:** Gyre treats every push as potentially hostile until the three-layer check
passes. Internal agents and external federated agents use the same verification path. Federation
works by trusting a remote Gyre instance's OIDC issuer -- no static credentials exchanged.

**Audit integration:** All eight capabilities emit structured events to the eBPF audit pipeline
(M7.1). Speculative merge conflict events, pre-accept rejections, custom ref writes, and code
awareness queries all appear in the SIEM feed with full agent identity context.
