# Source Control

## Core Decision

Gyre **is the source control host** - not an integration with an external forge.

- **Git** is the storage format and wire protocol. The forge speaks git (smart HTTP).
- **Jujutsu (jj)** is the agent's interface to version control. Agents should never run raw `git` commands - they use jj, which handles the complexity agents are bad at managing manually.
- jj translates to git on push. The forge doesn't need to know or care that agents use jj - it sees clean, conventional git commits arriving via the standard protocol.
- Owning the SCM layer unlocks deep integration: agents can interact with repos at the protocol level, audit every push/merge, enforce policy inline, and eliminate the seams between "code host" and "agent platform."

## Why jj Is Critical for Agents

Git was designed for humans who commit intentionally, name branches deliberately, and resolve conflicts interactively. Agents don't work like that. jj solves the specific problems agents face:

### 1. Every Tool Execution = Atomic Change

When an agent runs a tool (edit file, run command), jj automatically creates a new change. No explicit `git add && git commit` needed. The agent gets a full undo history at the granularity of individual actions, not just when it remembered to commit.

### 2. Operation Log = Crash Recovery

jj records every operation (commit, rebase, squash) in an undo log. If an agent crashes mid-task, the next session can `jj undo` back to a known-good state. With git, a crash mid-rebase leaves a broken state requiring manual intervention. For agents that crash routinely (context window exhaustion, lifetime budget), this is essential.

### 3. Anonymous WIP Changes

In git, every commit needs a branch. In jj, changes exist without branches - they're nodes in the DAG. Agents work without branch-naming overhead, then create a bookmark (jj's branch pointer) only when ready to push. This eliminates an entire class of "branch management" busywork agents are bad at.

### 4. Automatic Rebasing

When the target branch moves (because another agent's MR merged), jj automatically rebases the agent's in-progress work. With git, the agent must manually `git fetch && git rebase`, handle conflicts, and continue. In a system with 20+ agents pushing concurrently, the baseline moves constantly. jj handles this transparently.

### 5. Conflict as State, Not Error

In git, conflicts are an error state that blocks all work until resolved. In jj, conflicts are materialized in the working copy as regular content - the agent can keep working on other files and resolve conflicts when ready. This is critical when multiple agents work in parallel and the baseline moves frequently. An agent doesn't have to stop everything because one file conflicts.

### 6. Speculative Merge Compatibility

The forge's speculative merge feature (try-merge all in-flight branches) maps naturally to jj's model. jj can represent a speculative merge as a change depending on multiple parents without creating a branch or merge commit. If the speculation fails (conflict), it's just another conflicted change that can be discarded.

### 7. Ralph Loop Checkpoints

Each Ralph loop step can be a jj operation boundary. If the agent needs to roll back to "before self-review," that's a `jj undo` to a specific operation - not manually figuring out which git commits to reset. The custom ref namespace `refs/ralph/{task-id}/step-{n}` maps to jj operations naturally.

### The Separation

```
+------------------+          +------------------+
|                  |          |                  |
|  Agent Runtime   |   jj     |   Gyre Forge     |
|                  | -------> |                  |
|  speaks jj       |   push   |  speaks git      |
|  (local VCS)     |  (git    |  (smart HTTP)    |
|                  |  proto)  |                  |
+------------------+          +------------------+
```

- **Agents use jj** for local development (auto-changes, undo, rebase, conflicts)
- **The forge uses git** for storage, protocol, and provenance
- **jj translates** on push - the forge sees standard git commits
- **Conventional commits** are validated at push time regardless of whether the client used git or jj
- **Commit signing** (Sigstore) works on the git objects jj produces

## Merge Requests & Merge Queue as Primitives

- **Merge Requests (MRs)** are a first-class platform primitive - not a UI feature layered on top, but a core domain object with identity, lifecycle, and audit trail.
- **Merge Queue** is built into Gyre. When multiple agents submit concurrent MRs, the queue handles:
  - Sequential, intelligent merging against a moving head
  - Automatic rebasing when the baseline changes under an MR
  - Conflict detection and escalation (to the submitting agent or manager)
  - Quality gates (tests, lints, review approvals) evaluated per-MR before merge
  - Priority ordering (urgent fixes jump the queue)
- Because Gyre owns both the agents *and* the forge, the merge queue can coordinate directly: tell an agent to reimplement against the new head, hold an MR until a dependency lands, or batch non-conflicting MRs together.
- The merge queue is the serialization point where parallel Ralph loops converge into a linear, correct history.

## Agent-to-Commit Tracking

Since Gyre owns the forge, every commit has native metadata linking it to the agent session that produced it:

- **Session-to-commit mapping** - which agent session touched which commits (chronological)
- **Branch lifecycle** - initial branch at agent spawn, current branch, all observed commits
- **Dirty state** - did the agent leave uncommitted work when it terminated?
- **Commit-based queries** - "find all agent sessions that touched this file/commit" for audit and debugging
- **Time-travel** - reconstruct what the repo looked like when any agent session was active

This is richer than external forge integrations (inspired by Loom's git-metadata system) because the data is native, not scraped.

## External Repository Mirroring

- Support **pull mirroring** from external forges (GitHub, GitLab) for repos that need to be brought into Gyre.
- GitHub App integration for authenticated access to private repos (later milestone, not M0).
- Mirror namespace isolation (e.g., `mirrors/github/{owner}/{repo}`).
