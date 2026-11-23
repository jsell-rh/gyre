# Review — task-342 (Publish reviewed spec backlog for specs/system/dependency-graph.md)

Spec: `specs/system/dependency-graph.md` §Dependency Entity (gap 4390a1b94a03e318854cbbec — `Orphaned, // Target repo was deleted or archived`).
Candidate under review: `c1ad6b981f7c195cabe6dd3bf7d682bf5c46e025` (branch `pipeline/task-342/b1e71c2e3c81475a953be4a9f238d6ae-1`, base `0b33cb3a4a586c587e2566ecbcd82c03da94d84b`; audit base `87377fe145ebddf27ef975fba7829fb10f4bb136` is an ancestor of the base).
Verdict: **approved** — no findings. Backlog publication is faithful, complete, and non-destructive; the published audit's gap finding was independently re-verified against the canonical spec and the unchanged production code.

## What shipped

Task definitions and bindings only, no product implementation claimed (consistent with "Task creation is not implementation"):

- `specs/tasks/task-341.md` (new) — the planned task `dep-edge-orphaned-on-target-archive-delete`, `progress: not-started`, `depends_on: []`.
- `specs/tasks/task-342.md` (new) — the publication task itself, `progress: ready-for-review`, embedding the reviewed plan verbatim.
- `specs/task-spec-inputs.json` (+10 lines) — one `sections` entry `backlog/4390a1b94a03e318854cbbec` (file `specs/system/dependency-graph.md`, heading `[Cross-Repo Dependency Graph, Dependency Entity]`) and one `bindings` entry `Spec gap spec-cab97078efffe0fbfb7339be / dep-edge-orphaned-on-target-archive-delete / ed9f34316476` → that backlog key.

Diff is 3 files, 650 insertions, 0 deletions. No product file, spec, coverage status, registry entry, script, or gate touched. Task numbers 341/342 are the next unused (base tops out at task-340); neither file overwrites an existing task.

## Probes run (docs-only candidate — no compilation warranted; evidence under `/tmp/stage/review-evidence/`)

- **Plan fidelity (programmatic).** `task-341.md` reproduces the assignment plan's `required_behavior` verbatim, all six acceptance lines verbatim, and the `negative_probe` verbatim (as "Verification intent"); title and `covers` exact. `task-342.md`'s embedded JSON deep-equals the assignment's plan object (json raw_decode + `==`). `specs/task-spec-inputs.json` parses; the new entries match the format of the 84 pre-existing backlog sections and the prior dependency-graph bindings (same spec id `cab97078…`, same requirements-hash prefix `ed9f343…` as task-278/279/280's keys); the new backlog key occurs exactly twice in the raw file (sections + bindings) — no duplicate binding.
- **Audit challenge — the gap is real.** Spec line 70 defines Orphaned as "Target repo was deleted or archived" under §Dependency Entity. Re-verified in the unchanged code: `archive_repo` (repos.rs:365–540, read in full) cancels tasks/stops agents/closes MRs but never touches dependency edges; `delete_repo` (repos.rs:570–618, read in full) deletes the repo row, spec links, assertion results, and git dir — dependency edges absent; `dep_staleness.rs:64` skips Breaking/Orphaned and never orphans (unresolvable target yields `target_version: None`, no status change); the only Orphaned transition is reconcile's no-longer-detected check on a push to the SOURCE repo (git_http.rs ~4434), and the reconcile un-orphan path (~4441) would resurrect an edge whose archived target is still matched — SQLite `RepoRepository::list` (repository.rs:190) filters tenant only, not archived status, so archived targets stay in the match set. Every audit evidence quote checked out.
- **Plan feasibility.** `list_dependents` exists on the `DependencyRepository` port (gyre-ports/src/dependency.rs:21) with both SQLite and Postgres implementations (dependency.rs:211 in each adapter), and `state.dependencies` is wired via `store!` (lib.rs:2128) — the planned fix needs no port-trait changes, exactly as the task states. Spec §4 Reconcile's "mark as orphaned (don't delete - might be a detection gap)" supports the plan's choice to mark rather than delete edges in `delete_repo`.
- **Duplicate-work sweep.** No existing task covers target-archive/delete orphaning: task-048/278/285 cover the distinct declaration-disappearance (source-push) orphaning; task-199 covered Dependency Entity persistence (a different defect in the same section, already wired). Dependencies `[]` is correct — the task is self-contained.
- **Preservation.** Coverage matrices, specs, scripts, product code all untouched by the diff; new task remains `not-started` as required.

## Verifier notes

No server build or test execution is bound to this candidate (no code changed); the standard mechanical guards and matrix regeneration suffice. The implementation task `task-341` carries its own storage-backed acceptance criteria and negative probes for the verifier of that future candidate.
