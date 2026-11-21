# Review — task-311 (Publish reviewed spec backlog for specs/development/architecture.md)

Spec: publication of the independently reviewed missing-work plan
`68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e` for
`specs/development/architecture.md` (tasks 308/309/310 + registry bindings).
Candidate under review: `c42f3374` (branch
`pipeline/task-311/72773c1335954dcb996a6255b8b2e802-1`, base `edf3ed90` =
origin/main tip, which already carries main's task-307 explorer-canvas backlog).
Verdict: **approved** — no findings. The publication is exactly the assigned
plan: three new task documents verbatim-faithful to the reviewed plan, additive
registry bindings in the established format, and nothing else changed.

## Probes run (checkout at candidate `c42f3374`)

- `git diff --name-status edf3ed90..c42f3374` → only `specs/tasks/task-308.md`,
  `task-309.md`, `task-310.md`, `task-311.md` (added) and
  `specs/task-spec-inputs.json` (modified, purely additive hunks). No product
  file, spec, coverage file, gate, script, or workflow touched.
- `python3 scripts/test_pipeline_spec_backlog.py` → exit 0.
- `python3 scripts/test_pipeline_requirements.py` → 23 tests, OK.
- `python3 scripts/test_pipeline_decisions.py` → exit 0.
- `bash scripts/check-task-commit-attribution.sh` → OK.
- Registry structural check (python): JSON parses; the three new
  `backlog/<section-id>` entries exist with headings (`Storage`, `Tech Stack`,
  `API`) that appear verbatim in the canonical spec; all 247 binding keys
  reference existing sections; the new `Spec gap spec-f907eda3bd014fe546b838e2
  / <key> / 4156114b8040` bindings map 1:1 to the new backlog sections,
  matching the explorer-canvas (`spec-82658b0b…`) precedent.
- Fidelity check (python, mechanical): task-308/309/310 `required_behavior`,
  acceptance criteria (5/5/4 items), and negative probes are verbatim
  reproductions of the plan embedded in the assignment; `plan_id` matches;
  assigned names match `assigned_names`.
- Evidence under `/tmp/stage/review-evidence/task-311-*.txt`.

## Audit challenge (independent re-read of the canonical spec)

`specs/development/architecture.md` has exactly the seven sections the audit
covers (title, Tech Stack, Invariants, Storage, API, API Versioning,
Philosophy); every requirement quote in the audit matches the spec verbatim.
The three gap decisions were re-verified against current source:

- **Tech Stack / shadcn-svelte**: `web/package.json` contains zero matches for
  shadcn/tailwind/bits-ui (svelte `^5.0.0` present) — gap is real. The
  Rust-agent-runtime half is correctly recorded as a controller escalation
  (canonical conflict with `specs/system/agent-runtime.md`'s Node.js image),
  not disposed of by the task.
- **Storage / SQLite default**: `crates/gyre-server/src/lib.rs` `build_state`
  constructs `SqliteStorage`/`PgStorage` only from `GYRE_DATABASE_URL`; unset
  (documented default) → every port falls back to in-memory — gap is real, and
  task-308 carries the controller's playwright in-memory-opt-in feedback.
- **API / WebSocket agent transport**: `docker/gyre-agent/agent-runner.mjs`
  (778 lines) has zero WebSocket matches; all agent traffic is HTTP fetch —
  gap is real.
- Satisfied decisions spot-checked and upheld: `/api/v1/` literal route
  prefixes (327 in `api/mod.rs`), `/scim/v2` sanctioned by
  `specs/system/identity-security.md`, arch-lint wired in
  `.pre-commit-config.yaml` and `.github/workflows/ci.yml`, `WsMessage`
  serde `tag = "type"`.

## No duplicate work / dependencies

Each of the three new spec_refs is held by exactly one task (308/309/310, all
`not-started`). No other task document covers these architecture.md sections.
The `GOAL.md` spec_ref shared by publication tasks is the established pattern
(all other holders terminal/complete, task-307 precedent). `depends_on: []`
matches the plan; cross-task coordination (task-272, task-285/294/296/307) is
recorded in the task bodies rather than as false dependencies.

## Rebase resolution

Base `edf3ed90` is origin/main's tip (already containing main's task-306/307
explorer-canvas backlog); the candidate diff is purely additive relative to it,
so the claimed union holds trivially: main's `backlog/c3a39b32…` section and
binding and `specs/tasks/task-306.md`/`task-307.md` are all present and
unchanged.

Notes (non-blocking, recorded for the verifier):

- No coverage matrix exists yet for `specs/development/architecture.md` under
  `specs/coverage/` — consistent with publication-only scope; coverage rows are
  to be created/updated by the implementing tasks (308–310) per their
  verification-intent sections, and the verifier regenerates SUMMARY.md.
- The Shipped evidence note references a controller-side artifact path with an
  elided filename; harmless documentation, no product effect.
