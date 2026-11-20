# Review — task-277 (Publish reviewed spec backlog for specs/system/view-query-grammar.md)

Spec: backlog publication for `specs/system/view-query-grammar.md` (plan_id `eec79eddc83f`, audit base `25497b9e`, requirements `78cf53a9a787…`).
Candidate under review: `1c088af5e9db3184c59078cf704a70599fd999be` (branch `pipeline/task-277/c4eca291f2ae448da8bead60f84746bb-2`, base `00669baa`).
Verdict: **approved** — exact, additive, docs-only publication; no findings.

## What the candidate is

A single commit `1c088af5` ("chore: publish reviewed spec backlog eec79eddc83f"), +609 lines across exactly three files:

- `specs/tasks/task-276.md` (new) — the planned implementation task `diff-scope-commit-range`.
- `specs/tasks/task-277.md` (new) — this publication task, embedding the full reviewed plan JSON.
- `specs/task-spec-inputs.json` — two additive entries: the backlog section `backlog/629ddb2f3b21a81f49e78ca3` (file + heading path) and its binding under `Spec gap spec-57c2a110e6b0a8363571ac4d / diff-scope-commit-range / 78cf53a9a787`.

No product file, spec, coverage row, registry entry, gate, or existing task was touched (`git show --stat` confirms the three-file additive shape).

## Probes run

- Structural comparison (python, `plan-fidelity.txt` under `/tmp/stage/review-evidence/`): the plan JSON embedded in `specs/tasks/task-277.md` was extracted and compared field-by-field with the plan in the assignment prompt — document, plan_id, audit base/requirements, all 17 section decisions (context/satisfied/gap, detail text, evidence quotes), gaps array, source_evidence, plan.audit, the single task definition (all 8 fields), and `assigned_names` all match exactly. No rewording, no dropped section, no softened decision.
- `specs/tasks/task-276.md` vs the plan's `diff-scope-commit-range` task: `required_behavior` exact string match; all 8 acceptance bullets exact; `negative_probe` reproduced verbatim under "Verification intent" (plus the repo's standard coverage-row boilerplate, consistent with existing task documents); `spec_ref`/`depends_on`/`progress: not-started`/`commits: []` frontmatter matches the plan (`dependencies: []`, `existing_task: null`) and the assigned name mapping (`diff-scope-commit-range → task-276`).
- Registry validation: `specs/task-spec-inputs.json` parses as JSON; the new section's heading path `["View Query Grammar", "Primitives", "2. Scope — What Subgraph to Show"]` matches the canonical spec's actual `#`/`##`/`###` headings; the gap's requirement quote `| \`diff\` | Changes between two commits |` exists at `specs/system/view-query-grammar.md:47`; the binding-key format (`Spec gap spec-<id> / <task-key> / <12-hex>`) matches the 22 pre-existing entries and its short hash is the first 12 chars of the audit's requirements hash; each new key appears exactly once (sections) / once (bindings target) — no duplicate registry keys, no other binding claims the new section.
- `PYTHONPATH=scripts python3 -m unittest scripts.test_pipeline_spec_backlog` → 40 tests OK (validates the publication shape the pipeline enforces: only `specs/tasks/*` + `specs/task-spec-inputs.json` change).
- Audit challenge (gap is real in the current tree): `crates/gyre-domain/src/view_query_resolver.rs:792` carries the conceding comment about intermediate commits being missed; `:830` is the boundary-SHA predicate `(created_matches_to || modified_matches_to) && !(created_matches_from && modified_matches_from)`; `:801` emits the "[info] SHA-based diff shows an approximate result" warning; `web/src/lib/ExplorerCanvas.svelte:2380/2386` mirrors the heuristic ("Matches server heuristic including its limitations"). Feasibility corroboration holds: `crates/gyre-domain/src/extractor.rs:93` (`extract(&self, repo_root, commit_sha)`) and `crates/gyre-adapters/src/git2_ops.rs:152` (`async fn diff(&self, repo_path, from, to)`). Spot-checks of the "satisfied" evidence quotes also verify (`pub enum Scope {` in `gyre-common/src/view_query.rs:13`; `## View Query Grammar` and `fn parse_view_query_from_text` in `explorer_ws.rs`; `resolveComputed` in `ExplorerCanvas.svelte:2173`; `case 'view_query'` in `ExplorerChat.svelte:359`). The audit is accurate against the current tree, not just its base.
- Duplicate-work analysis: no active task covers the view-query diff scope — the only tasks referencing it are the new task-276/277; `graph-diff-two-commits` (realized-model plan `b26f707c…`) has not yet been published as a task document, and task-276's definition contains the required shared-core contingency and cross-reference instructions, so no duplicate active assignment or missing dependency is introduced. Existing view-query-adjacent tasks (055, 063/064 surfaces, 208, 210, 224) are all `complete`.
- Task numbering: task-276/task-277 are the next unused numbers after task-275 (per the `_publication: task-277` reservation in `assigned_names`).

## Non-blocking observations

- Coverage matrix row 4 of `specs/coverage/system/view-query-grammar.md` still reads "implemented" (task-063, R3) even though the independent audit — correctly, per the code evidence above — classifies the `diff` scope row as a gap. This is pre-existing and untouched by the candidate, as the assignment requires ("Preserve every … coverage status"); task-276's required behavior explicitly mandates updating the coverage rows only after implementing the behavior, so the stale row is owned by the implementation task, not this publication.
- The publication commit is not task-NNN-labeled and touches no product surface, so it is outside `scripts/check-task-commit-attribution.sh` scope; `commits: []` on the publication task matches the established convention (cf. task-272, and the identical `chore: publish reviewed spec backlog` precedent `16158031`).
- `scripts/check-task-commit-attribution.sh` timed out in this sandbox (needs full-history traversal; 120s limit) — irrelevant to this candidate for the reason above; the verifier's CI run covers it.

## Verdict

The publication is exact and additive: the published task documents and section bindings reproduce the independently reviewed plan verbatim, the assigned names are honored, the new task remains `not-started`, and nothing else changed. Approved.
