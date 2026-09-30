# Coverage: The Ralph Loop

**Spec:** [`system/ralph-loop.md`](../../system/ralph-loop.md)
**Last audited:** 2026-09-30 (full spot-check of all 27 `n/a` rows. Supersession verified genuine: spec header declares Superseded by agent-runtime.md §1/§5; agent-runtime.md:486 confirms replacement. Requirement mapping confirmed: inbox→message-bus.md (10 verified rows), lifecycle/spawn/orchestrator/concurrency/reviewer/error-recovery→agent-runtime §1 Phases 4-8 (coverage rows 7-11, task-118, audited 2026-09-30), max_iterations→agent-runtime:140, prompt freezing→§5 (task-116), provenance→authorization-provenance.md (100%). Row 22 RalphStep removal verified DONE in code: enum absent from gyre-domain; only historical migration (sqlite/migrations.rs:240) and an AIBOM removal-note comment (aibom.rs:83) remain — the spec's required end state. No status changes.)
**Coverage:** 0/27 (27 n/a — superseded)

> **Entire spec superseded by [`agent-runtime.md`](../../system/agent-runtime.md) §1 (Agent Lifecycle) and §5 (Agent Prompt Structure).** All sections marked n/a — requirements are tracked via the agent-runtime coverage matrix.

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Origin | 2 | n/a | - | Superseded by agent-runtime.md. |
| 2 | Position in the SDLC | 2 | n/a | - | Superseded by agent-runtime.md. |
| 3 | Core Concepts | 2 | n/a | - | Superseded by agent-runtime.md. |
| 4 | Agent vs. Session | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 5 | The Message Inbox | 3 | n/a | - | Superseded by agent-runtime.md §1 + message-bus.md. |
| 6 | One Agent, One Task | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 7 | Properties | 2 | n/a | - | Superseded by agent-runtime.md. |
| 8 | 1. Same prompt, every session | 3 | n/a | - | Superseded by agent-runtime.md §5. |
| 9 | 2. Fresh context, every session | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 10 | 3. Durable state between sessions | 3 | n/a | - | Superseded by agent-runtime.md §1 + source-control.md. |
| 11 | 4. Terminal conditions | 3 | n/a | - | Superseded by agent-runtime.md §1 + agent-gates.md. |
| 12 | 5. Git commit as session boundary | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 13 | 6. Max iterations as safety net | 3 | n/a | - | Superseded by agent-runtime.md §4. |
| 14 | Integration with Gyre | 2 | n/a | - | Superseded by agent-runtime.md. |
| 15 | Agent lifecycle | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 16 | Spawn API | 3 | n/a | - | Superseded by agent-runtime.md §1 Phase 4. |
| 17 | Orchestrator interaction | 3 | n/a | - | Superseded by agent-runtime.md §1 Phases 2-3. |
| 18 | Concurrent loops | 3 | n/a | - | Superseded by agent-runtime.md §1. |
| 19 | Reviewer agent behavior | 3 | n/a | - | Superseded by agent-runtime.md §1 Phase 6. |
| 20 | Error recovery | 3 | n/a | - | Superseded by agent-runtime.md §1 Phase 7. |
| 21 | Provenance Integration | 2 | n/a | - | Superseded by agent-runtime.md + authorization-provenance.md. |
| 22 | Required code changes (RalphStep removal) | 3 | n/a | - | Superseded by agent-runtime.md. |
| 23 | Quality Standards and Meta-Specs | 2 | n/a | - | Superseded by agent-runtime.md §2. |
| 24 | Relationship to Other Specs | 2 | n/a | - | Cross-reference section. |
| 25 | Supersedes | 3 | n/a | - | Cross-reference section. |
| 26 | What Gyre Adds | 2 | n/a | - | Rationale/comparison — no implementable requirement. |
| 27 | Open Questions | 2 | n/a | - | Design questions — no implementable requirement. |
