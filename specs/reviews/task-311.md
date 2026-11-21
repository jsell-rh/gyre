# Review — task-311 (Publish reviewed spec backlog for specs/development/architecture.md)

Spec: `GOAL.md — real implementations and meaningful verification` (publication of the independently reviewed missing-work plan `68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e` for `specs/development/architecture.md`).
Candidate under review: `1cd387bf96eeeb14560437e9c4c083ae9d407fff` (branch `pipeline/task-311/bbd32a6fe2894690a1bee42c2bc823a6-1`, assigned base `f0f2ba5da6355cd7ed059f095de03848f3bada50` — main carrying the task-305 agent-runtime.md backlog).
Verdict: **approved** — no findings. The publication is faithful, minimal, and preserves every pre-existing artifact; the durable merge-conflict finding against main is properly resolved as an exact union.

## What the candidate changes

`git diff --name-only f0f2ba5d..1cd387bf` is exactly: `specs/task-spec-inputs.json` (+30), `specs/tasks/task-308.md`, `task-309.md`, `task-310.md`, `task-311.md` (new). Nothing else — no product file, spec, coverage status, or gate touched.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- **Plan fidelity (deep compare, exit 0):** the plan JSON embedded in `specs/tasks/task-311.md` is deep-equal to the assigned plan on every key (`document`, `plan_id`, `audit`, `plan`, `assigned_names`). Tasks 308/309/310 match `plan.tasks` field-by-field: titles, `required_behavior`, every acceptance criterion (5/5/4), `negative_probe`, `covers`, `dependencies: []` — all verbatim; frontmatter `progress: not-started`, `commits: []`, criteria unchecked.
- **Registry union (three-way dict compare, exit 0):** `specs/task-spec-inputs.json` at HEAD vs main (`f0f2ba5d`) vs the pre-merge pipeline side (`0e95f2de`): no key missing from either parent, no extra keys, no value changes. Sections 306→309, bindings 252→255 (+3 each, exactly this plan's entries); `shared`/`version` unchanged. The rebase resolution claim in the Shipped note is independently confirmed.
- **Scope preservation:** tasks 297–307 byte-identical to main; tasks 308–311 do not exist on main (no numbering collision — main tops out at task-307); `specs/development/architecture.md` is byte-identical at the audit base `678a8ac3` and HEAD.
- **Section bindings:** three `backlog/<section-id>` entries map to the correct headings in the canonical spec (Tech Stack / Storage / API under "Architecture & Engineering Standards"); each is bound by exactly one `Spec gap spec-f907eda3bd014fe546b838e2 / <key> / 4156114b8040` key whose requirements-hash prefix matches the plan's `audit.requirements`. No duplicate active assignment; the three new `spec_ref`s are unique across all task docs.
- **Pipeline suites:** `cd scripts && python3 -m unittest test_pipeline_spec_backlog` (40 tests), `test_pipeline_requirements` (23), `test_pipeline_decisions` (5) — all OK, exit 0.

## Audit challenged against the canonical spec (re-read in full)

- All seven section decisions re-verified against the current tree: gap evidence confirmed (web/package.json has svelte ^5.0.0 but no shadcn-svelte/tailwindcss/bits-ui; `agent-runner.mjs` has zero WebSocket usage with all interactions HTTP fetch; `build_state` selects storage only from `GYRE_DATABASE_URL` with unset ⇒ in-memory fallback via `store!`, and `docs/server-config.md:60` documents "Unset = in-memory (default, stateless)"; `web/playwright.config.js` boots the server with only `GYRE_PORT`/`GYRE_AUTH_TOKEN`); satisfied decisions confirmed (route audit of `api/mod.rs` + `lib.rs`: 332 `/api/v1` registrations, no `/api/v2`, non-versioned remainder only the allowed classes incl. 7 `/scim/v2` routes canonically mandated by `identity-security.md:116`; `check-arch.sh` wired as pre-commit `arch-lint` and CI job); context decisions are sound (title-only and philosophy sections carry no implementable requirement). Every quoted evidence line was grepped and found verbatim.
- **Decomposition/dependencies:** three independent gaps → three tasks with `depends_on: []`, one section per task, no cycles; the storage, UI-stack, and agent-transport surfaces touch disjoint file sets. Coordination cross-references are real (task-272, task-285, task-294, task-296, task-307 all exist; the ui-layout backlog is bound in the registry).
- **Duplicate work:** no other task document or registry binding mentions the shadcn-svelte adoption, SQLite-default-storage, or WebSocket-agent-transport subjects; each gap section is bound exactly once; `existing_task: null` verified.
- **Canonical-conflict handling is correct:** the Rust-"agent runtime" parenthetical vs `agent-runtime.md`'s Node.js image conflict is published as an open controller escalation inside task-309 (explicitly not closed by code or coverage claim), exactly as the review rules require.

## Non-blocking observations (recorded, not findings)

- The Shipped verification-evidence bullet cites `python3 scripts/test_pipeline_spec_backlog.py ... all exit 0`; that direct invocation exits 0 without running tests (the file has no `unittest.main()` guard — the suite must be run as `python3 -m unittest test_pipeline_spec_backlog` from `scripts/`). Run correctly, all 40 tests pass, so the claim's substance is true; the invocation form is vacuous.
- The same section's final bullet points at `/tmp/stage/review-evidence/task-3....txt` "(controller artifact path; not a sandbox path)" — an unresolvable placeholder pointer. Documentation blemish in the task's own Shipped prose only; no plan or binding content is affected.

## Verifier notes

No Rust or frontend compilation is needed for this candidate (no product source changed). The deterministic verifier should re-run the three pipeline suites above from `scripts/` and its usual registry/guard checks on the merged tree.
