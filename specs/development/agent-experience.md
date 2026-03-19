# Day-One Agent Experience Infrastructure

Inspired by OpenAI's Codex harness learnings: the bottleneck is never the agent's ability to write code - it's the **environment's legibility and feedback loops**. From day one, this system must ship with:

## Testing & Validation

- Agents must be able to **boot, drive, and validate** the application per worktree/branch.
- Ephemeral, isolated environments per agent task - including their own observability stack.
- **Full sensory access** - every component must be testable by agents with the same fidelity a human would have:
  - **TUI:** visual snapshot testing (rendered terminal output comparison).
  - **Web UI:** agents see the UI at multiple screen sizes/scales, access console logs, DOM state, network requests (via Chrome DevTools Protocol or equivalent).
  - **API:** agents can drive and inspect every endpoint.
  - If an agent can't see it, it can't test it. If it can't test it, it can't ship it.

## Observability for Agents

- Agents query their own **logs, metrics, and traces** (LogQL, PromQL, TraceQL or equivalent).
- Agents can reason about performance, errors, and behavior from instrumentation - not just code.

## Repository as System of Record

- All knowledge lives in the repo - not in Slack, Google Docs, or people's heads.
- Small, structured entry point (like AGENTS.md) as a **map**, not a manual. Progressive disclosure.
- Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo.
- **Mechanical enforcement** that docs stay current: linters, CI, doc-gardening agents.

## Spec Organization & Cache Hit Optimization

- All specs live in `specs/` with a `specs/index.md` entry point.
- Think about spec structure in terms of **cache hits vs. misses**: if an agent tool call (search, read, etc.) finds what it needs on the first try, that's a cache hit. If it fails and requires additional lookups, that's a cache miss.
- Specs should be structured and indexed to **maximize cache hits** - clear naming, cross-linking, predictable paths.
- This is an addendum to the OpenAI blog's "progressive disclosure" concept: it's not just about layering depth, it's about making every lookup **land**.

## Entropy Management / Garbage Collection

- Agents replicate patterns - including bad ones. Drift is inevitable.
- Encode **golden principles** in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs.
- Technical debt is paid continuously in small increments, not in painful bursts.
- Human taste is captured once, then enforced continuously on every line.

## Agent Legibility as a Design Goal

- The codebase is optimized for **agent legibility first** - if an agent can't see it in context, it doesn't exist.
- Favor boring, composable, stable dependencies that agents can fully internalize.
- Custom lint error messages should include **remediation instructions** - they become agent context.
