# Agent Development Workflow Requirements

## Immediate Feedback

- Every agent must get **immediate, unambiguous feedback** on whether its work succeeded or failed. No guessing, no waiting.
- Tests, lints, type checks, snapshot comparisons - all run locally in the agent's environment before any PR is opened.
- If feedback is slow or absent, that's a platform bug to fix.

## Parallel Work via Worktrees

- Agents work in **git worktrees** - isolated copies of the repo per task.
- Multiple agents work in parallel without stepping on each other's branches or files.
- Worktrees are ephemeral - created at task start, cleaned up on completion.

## PRs as the Collaboration Primitive

- All code submission and collaboration happens via **pull requests**. No direct pushes to main.
- A **PR hygiene agent** (or background job) continuously monitors for stale PRs - unreviewed, unmerged, conflicted, or abandoned - and takes action: nudge reviewers, rebase, escalate, or close with explanation.
- PRs should be short-lived. If a PR is open for too long, that's a signal something is wrong.

## Fix the Environment, Not the Agent

- When an agent hits an error, the **first response is never "try again."**
- Instead: diagnose the root cause and **engineer the environment** to catch or prevent that class of error in the future (lint rule, pre-commit hook, better error message, test fixture, etc.).
- Only after the environment is hardened does the agent retry.
- This is the interrupt ledger philosophy: every error is a signal. 93% of recurring errors are structurally fixable. Fix them once, benefit forever.
