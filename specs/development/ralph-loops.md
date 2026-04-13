# Ralph Loops

> **Superseded by [`specs/system/agent-runtime.md`](../system/agent-runtime.md) §1.** This file is kept for historical context only. The canonical definition of the agent lifecycle, signal chain, and Ralph loop lives in the agent runtime spec.

## Definition

A **Ralph (Wiggum) Loop** (per OpenAI's Codex harness team): agent reviews its own changes, requests additional agent reviews, responds to feedback, and **iterates until all reviewers are satisfied**. This is the core development feedback cycle.

## Core Value Proposition

**The loop is the primitive. It is here to stay.** Gyre's job is twofold:
1. **Automate the loop** - make each individual Ralph loop as fast and frictionless as possible.
2. **Solve coordination between loops** - when multiple agents are running concurrent Ralph loops, Gyre orchestrates: sequencing, conflict resolution, shared state, escalation, and convergence.

This is the core value proposition. Everything else is infrastructure in service of these two goals.

## Meta Ralph Loops

**Meta Ralph loops** become possible because Gyre owns networking, compute, and source control - agents can spin up sub-agents, observe results, and feed back into higher-order loops.

## See Also

- [`manager-agent.md`](manager-agent.md) - How the Manager Agent defines and orchestrates Ralph loops
- [`../prior-art/gas-town.md`](../prior-art/gas-town.md) - NDI pattern: persistent, crash-surviving workflow steps
