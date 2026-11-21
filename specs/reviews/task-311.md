# Review — task-311 (Publish reviewed spec backlog for specs/development/architecture.md)

Spec: `specs/development/architecture.md` (all sections). Candidate under review: `fe91bf67` (branch `pipeline/task-311/d470dc5645ef4148b540e67097f57be3-1`, base `3ee882f1`). Verdict: **approved** — no findings.

## Scope of the diff

`git diff 3ee882f1..fe91bf67` is purely additive, specs-only (754 insertions, 0 deletions):

- `specs/tasks/task-308.md` (sqlite-default-storage), `task-309.md` (shadcn-svelte-ui-stack), `task-310.md` (websocket-agent-transport) — the three backlog tasks, all `progress: not-started`, `depends_on: []`, `commits: []`
- `specs/tasks/task-311.md` — the publication task document embedding the assigned plan verbatim
- `specs/task-spec-inputs.json` — three new `backlog/<section-id>` section entries and three new `Spec gap spec-f907eda3bd014fe546b838e2 / <key> / 4156114b8040` bindings, purely additive

No product file, spec text, coverage matrix, gate, or existing task/registry entry was touched. Task creation only; no implementation claimed (the "Shipped" section states exactly that).

## Checks performed (evidence: `/tmp/stage/review-evidence/`)

**Plan fidelity (programmatic).** The plan JSON embedded in `specs/tasks/task-311.md` is byte-identical to the plan in the assigned task body. For each of the three tasks: `required_behavior` verbatim, acceptance criteria verbatim (5/5, 5/5, 4/4), negative probe reproduced verbatim under "Verification intent" (plus the standard anti-inflation guard paragraph also present in prior published tasks, e.g. task-247..254), `depends_on: []` matches the plan's empty dependencies, and the discovery-evidence section id equals the plan's `covers` entry.

**Registry correctness.** `specs/task-spec-inputs.json` parses; the three new section entries point at `specs/development/architecture.md` with heading paths `["Architecture & Engineering Standards", "Storage"|"Tech Stack"|"API"]` that match the actual spec headings exactly; each new binding key equals the corresponding task doc's `spec_ref` exactly; the 12-hex hash suffix `4156114b8040` equals `audit.requirements[:12]`; no other binding references the three backlog section ids (no duplicate assignment); no existing entry was modified or removed.

**Duplicate-work analysis.** No existing task other than 308–311 references `specs/development/architecture.md`; task numbers 297–307 do not exist on this branch (assigned to other in-flight plans — no collision); the commit shape matches the established publication pattern (cf. `d8f37938` for task-296's publication: same three-file class, specs-only). The three tasks are genuinely independent (storage default, UI styling stack, agent transport), so empty `depends_on` is correct.

**Audit challenge (all gap claims re-verified against the code at the candidate):**

- *Storage gap* — confirmed: `lib.rs:1742` reads `GYRE_DATABASE_URL` from env only; the `store!` macro (`lib.rs:1776`) falls back to a mem implementation when unset; `docs/server-config.md:60` documents "Unset = in-memory (default, stateless)"; `web/playwright.config.js` sets only `GYRE_PORT`/`GYRE_AUTH_TOKEN` (the harness-migration concern in task-308 is real).
- *shadcn-svelte gap* — confirmed: `web/package.json` dependencies are `{d3-force, d3-scale, elkjs, svelte-i18n}`; devDependencies contain `svelte ^5.0.0` (Svelte 5 satisfied) but no tailwindcss, bits-ui, shadcn-svelte, or clsx/tailwind-merge.
- *WebSocket agent-transport gap* — confirmed: zero `WebSocket` matches in the 778-line `docker/gyre-agent/agent-runner.mjs`; server-side support framing in task-310 is accurate (`ws.rs:46` Auth-first authentication, `ws.rs:558` `Destination::Agent` delivery).
- *Plan precision* — the "112 call sites" claim for `build_state` in `crates/gyre-server/tests/` is exactly 112.
- *Satisfied/context claims spot-checked* — `#[serde(tag = "type")]` + Unknown-variant additivity in `protocol.rs`; `/scim/v2` routes with the canonical sanction in `identity-security.md:116`; `check-arch.sh` wired in both `.pre-commit-config.yaml` and CI; the Philosophy and title sections are rationale-only (no implementable requirement) — no task created for satisfied/context sections, correctly.

**Canonical conflict handling.** The Rust-"agent runtime" parenthetical conflict between `architecture.md` and `specs/system/agent-runtime.md` is published as an open controller escalation inside task-309 (not closed by code or coverage claim), exactly as the plan requires; task-309 scopes itself to the shadcn-svelte half only.

## Acceptance criteria

- Every planned task faithfully implements its assigned canonical requirements — verified (verbatim plan fidelity; requirement quotes match `specs/development/architecture.md` exactly).
- No duplicate active assignment or missing dependency — verified.
- No product behavior, spec requirement, coverage status or enforcement changed — verified (specs-only additive diff).
- New tasks remain not-started — verified (frontmatter of 308/309/310).

## Notes for the verifier (non-blocking)

- No compilation or server probes were run: the diff contains no production code, so there is nothing to compile or execute beyond the structural checks above; `specs/task-spec-inputs.json` JSON validity and markdown frontmatter well-formedness were checked directly.
- task-309/task-310 coordination text references task-307 (explorer-canvas), which does not yet exist on this branch — it is verbatim controller plan text referencing an assigned task in another in-flight plan, not a candidate defect.
- The verifier regenerates `specs/coverage/SUMMARY.md` from the merged matrices; no coverage matrix exists yet for `specs/development/` (consistent with the other development specs tracked via `task-spec-inputs.json` backlog entries).
