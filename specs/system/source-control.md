# Source Control

## Core Decision

Gyre **is the source control host** - not an integration with an external forge.

- **Git** as the baseline VCS.
- **Jujutsu (jj)** as the agent-facing VCS layer on top of git - atomic per-action changes, anonymous WIP, automatic rebasing, operation log for full undo. (See Loom's "Spool" system for reference.)
- Owning the SCM layer unlocks deep integration: agents can interact with repos at the protocol level, audit every push/merge, enforce policy inline, and eliminate the seams between "code host" and "agent platform."

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
