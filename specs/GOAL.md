# Close All Spec Gaps — Real Implementations Only

## Goal (authoritative)

Close every remaining spec gap in `specs/coverage/` with **real, production implementations**. No fakery in any form.

**Non-negotiable rules:**

1. **Real work only.** A section is closed only by production code that actually performs the specced behavior: real queries against real storage, real crypto (`verify()` called with real keys, not structural checks), real evaluation against real state, real enforcement that rejects/fails. Every `bail!()` stub, in-memory-only stand-in, audit-only mode where the spec requires enforcement, unimplemented route, or hardcoded value standing in for a looked-up one **is an open gap** — it must be implemented or the section stays open.
2. **No test inflation.** Tests exist to prove correctness of the behavior, not to move coverage numbers. A test that doesn't fail when the specced behavior breaks (self-confirming, mirrored-logic, stub-backed, assertionless, conditional-guard) does not close a section. Prefer one hard test that kills a real bug over ten that verify nothing.
3. **No fake completion.** Never mark a section `verified`/`implemented` without production code satisfying it. Never close a section as a side effect of a broader task without checking it explicitly. Progress is measured by shipped, working behavior — the coverage matrix reflects reality; it does not define it.
4. **Specs are the contract.** If a spec is genuinely wrong or outdated, amend the spec (via spec lifecycle) rather than shipping code that contradicts it. Never "implement" around a spec by reinterpreting it.

**Definition of done (repo-wide):** `grep -c '| not-started |' specs/coverage/system/*.md` returns 0 for every spec, every `task-assigned`/`in-progress` task reaches `complete` with real work behind it, and the spec-fidelity auditor has confirmed no `implemented` section is hollow.

## Out of scope

- `trusted-foundry-integration.md` — reference spec only, not scheduled for implementation.
- Pure context/rationale sections classified `n/a` — no implementable requirement.
- New tests for already-working behavior — not needed, not wanted.

## Standing constraints

- Hexagonal boundaries (enforced by `scripts/check-arch.sh`), conventional commits, existing prompt/workflow machinery.
- The next worker should pick up a task in `specs/tasks/` (prefer `needs-revision` first, then lowest-numbered eligible `not-started` with deps satisfied).
