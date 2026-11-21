# Review — task-305 (Publish reviewed spec backlog for specs/system/agent-runtime.md)

Spec: `specs/system/agent-runtime.md` missing-work plan `6a0d0dc0bb14` (12 gap sections, 8 planned tasks).
Candidate under review: `813f8c8d` (branch `pipeline/task-305/870476d1f7fc49be8a4c30a4169ccbfd-1`, base `077d942`; publication commit `93a834af`, merge of base `80c672d9`, docs `813f8c8d`).
Verdict: **approved** — no findings. Evidence under `/tmp/stage/review-evidence/` (summary.md with all probe commands and outputs).

## What was verified

- **Scope.** The candidate's net diff from the assigned base is exactly 10 spec files: `specs/task-spec-inputs.json` and `specs/tasks/task-297.md`–`task-305.md`. No product code, canonical spec, coverage matrix/status, script, gate, or existing task is touched. The merge commit brings in only the base's own content.
- **Faithful publication.** All eight task documents (297–304) match the embedded reviewed plan verbatim — titles, `spec_ref` gap keys, `depends_on` (298→[302], 302→[301], 303→[298]), `progress: not-started`, empty `commits`, and the full required-behavior / acceptance / negative-probe / covered-sections bodies (field-by-field programmatic comparison against the plan JSON extracted from the assignment). The merge left all eight files byte-identical to the publication commit.
- **Registry integrity.** The `specs/task-spec-inputs.json` merge resolution is the exact parsed union of the base's and the publication commit's sections and bindings — zero entries dropped from either side, binding values consistent, file valid JSON.
- **No duplicate or missing assignment.** Each of the 12 audited gap sections is covered by exactly one planned task; registry bindings match each task's `covers` exactly; no other task or spec binds any of the 12 backlog section ids; task numbering 297–305 collides with nothing on the base (which carries ≤296 plus 306/307). Dependency graph is acyclic and matches the plan.
- **Grounding of the audit the plan publishes.** All 12 gap section headings and requirement quotes were verified present in the canonical `specs/system/agent-runtime.md`, and the audit-challenge spot checks all hold at this base (verified independently by grep): `gyre_spawn_worker` dispatched (mcp.rs:5252); meta-spec CRUD at `/api/v1/meta-specs-registry` with blast-radius occupying the specced `/api/v1/meta-specs` prefix (mod.rs:937/977-991); `/api/v1/personas` still routed (mod.rs:1002); archive/unarchive in the frozen exemption file (lines 30/34); `specs/system/ralph-loop.md` live with a cross-reference in `authorization-provenance.md`; `Workspace` has no `max_agent_iterations` (Repository does, repository.rs:57); unscoped `agents://` list-all fallthrough (mcp.rs:4664); `docker/gyre-agent/Dockerfile` exists with build guidance at spawn.rs:1772; workspace/tenant meta-spec-set routes live (mod.rs:821/854); registry write gate tenant-Admin for every scope (meta_specs.rs:8674).
- **Attribution hygiene.** The `docs(task-305)` commit is docs-only (not product surface), so it is out of scope for `check-task-commit-attribution.sh` by that check's own rule; the `chore: publish` commit is not task-labeled.

## Non-blocking observations (not findings)

- `specs/task-spec-inputs.json` carries a whitespace-only merge artifact on one line (`"Structural (default)"      ]` — the closing bracket of main's explorer-canvas heading array lost its newline). The file is valid JSON and the parsed entry is identical to the base's; content-neutral formatting blemish that any registry rewrite would normalize.
- No runtime probes were applicable: the candidate contains no executable code. The standard mechanical check suite runs in the verifier against a code tree identical to the assigned base.
