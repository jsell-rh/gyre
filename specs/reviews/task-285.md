# Review — task-285 (Publish reviewed spec backlog for specs/system/dependency-graph.md)

Spec: `specs/system/dependency-graph.md` (canonical, unchanged by the candidate).
Candidate under review: `3b69bd78` (branch `pipeline/task-285/46cc2171a5114a76bfeeccf1c0e4cfa0-1`, base `678a8ac3`).
Verdict: **approved** — the publication is exactly the reviewed plan, nothing else.

## Scope of the diff

Pure additions, 1431 insertions / 0 deletions across 9 files
(`git diff --stat 678a8ac3..3b69bd78`, evidence `01-diff-scope.txt`):

- `specs/tasks/task-278.md` … `specs/tasks/task-284.md` — the seven planned
  backlog tasks (dep-detection-extraction-forms, breaking-change-contract-route,
  cascade-test-execution, impact-cascade-results-ui, deps-spec-route-paths,
  orchestrator-merge-ordering-budget, aibom-stale-dependency-flags).
- `specs/tasks/task-285.md` — the publication record embedding the full reviewed
  plan JSON (plan_id `1745ed1044f3…`, audit base `678a8ac3`, assigned_names).
- `specs/task-spec-inputs.json` — 10 new `backlog/<section-id>` inputs and 7 new
  binding groups, all keyed `Spec gap spec-cab97078efffe0fbfb7339be / <task-key> /
  ed9f34316476`.

No product file, spec, coverage row, gate, script, or existing task was touched.

## Plan fidelity

Each of task-278…284 carries the plan's `required_behavior`, `acceptance`
(item counts 7/5/3/4/4/4/4 match the plan arrays), `negative_probe`, and
`Discovery evidence` (the audit's per-section JSON) verbatim. Dependencies match
the plan: task-281 → [task-280], task-282 → [task-281], all others empty. All
seven are `progress: not-started`; task-285 is `ready-for-review` with an honest
"Shipped" note ("Task definitions and explicit bindings only; no product
implementation claimed") — no fake completion.

## Audit challenged against the canonical spec and the code

The task contract requires re-reading the canonical spec and challenging the
audit before approving. Every audited gap claim was verified against the tree
(production files are byte-identical at base and candidate; evidence
`02-audit-claims-code-verification.txt`):

- **§Automatic Detection parent (12ff1c4b):** `detect_dependencies_on_push`
  call site sits inside the `applied_updates.iter().filter(|u| u.refname ==
  default_ref)` loop (git_http.rs:1601→1630) and early-returns unless a
  dependency file changed — every-push trigger genuinely missing.
- **§1 Parse Dependency Files (e07d32e2):** `extract_path_value`
  (git_http.rs:3812) is the only Cargo reference extractor — `path`-key values
  only; zero `git =` key parsing anywhere in the Cargo detection path; only
  root-level `Cargo.toml` is read while the trigger matches nested paths.
- **§3 Parse API Contracts (8244b7d9):** `content.contains(repo_name)` substring
  scans at git_http.rs:4137/4182; root-only `["openapi.yaml", "openapi.yml",
  "swagger.json"]` list at :4644.
- **§Breaking Change Detection (c8dab087):** `detect_breaking_commits` over the
  git log (:5096) is the sole detection input — no API-contract-diff route; no
  breaking-change notify path in mcp.rs (grep exit 1).
- **§Cascade Testing (7014f11):** `trigger_cascade_tests` creates the
  cascade-test task with no `task_type` (count 0 in the function body), and
  signal_chain.rs:799 skips untyped tasks ("untyped → never spawned").
- **§Version Drift Tracking (551185c):** api/aibom.rs contains no
  stale/dependency content (grep exit 1).
- **§Impact Analysis (74607a9):** `BlastRadiusResponse` carries only
  repo_id/direct/transitive/total (dependencies.rs:220-225); no endpoint serves
  cascade test results.
- **§API (79be3a4):** zero matches for the literal `api/v1/deps` across crates/
  and web/src (grep exit 1) — none of the ten spec table rows (spec lines
  203-212, verified) is served on its literal path.
- **§UI (810eac2):** `web/src/lib/api.js` contains zero `cascade` occurrences
  while ImpactAnalysisModal.svelte:130 calls
  `api.cascadeTestResults?.(repoId) ?? Promise.resolve(null)` — dead wiring
  confirmed.
- **§Workspace Orchestrator Integration (589b5be):** `budget.rs` has no
  `list_dependents` call (exit 1); the only writer of `MergeRequestDependency`
  is the manual `set_dependencies` handler (merge_deps.rs:149).

All cited requirement quotes exist verbatim in the canonical spec (spec lines
76, 110, 124, 176, 191, 195, 203-212, 241-242), and every section binding in
task-spec-inputs.json maps to the real heading containing its quote. The
satisfied/context decisions (capability thesis via store! wiring, manual/API/CLI
declaration paths, spec-link detection, reconcile logic, policy enforcement,
CLI commands) are consistent with the code and were spot-checked.

## Duplicate-work and dependency analysis

- The 10 new backlog inputs and 7 binding groups are pure additions; no
  pre-existing input or binding references any of the ten section ids.
- All prior dependency-graph tasks (020/021/022/024/046/048/050/052/163/164/199)
  are `complete` — no duplicate active assignment.
- The only other active tasks (094, 271, 273) target other specs.
- Dependency chain (280 → 281 → 282) is acyclic and matches the plan's stated
  reasons (durable cascade results before the impact surface; impact surface
  before the spec-literal routes that must include it).

## Non-blocking observations

- The pre-existing coverage matrix rows for the audited gap sections still read
  `verified` (rows 5/6/10/15 carry path-divergence and mem-store notes). The
  publication contract explicitly preserves coverage status, and each new task's
  verification intent instructs updating coverage rows only after implementing —
  correct division of labor, not a defect of this candidate.
- task-284's coordination note references the staged supply-chain plan
  (`b2576f0bb5cd…`, task `release-artifacts-aibom-sbom`), which lives in the
  pipeline's active plan index rather than in-repo task docs; the note is quoted
  verbatim from the reviewed plan and is advisory partition text, not a binding.
  The AIBOM-content vs release-artifact partition it describes is sound.

## Probes

No compilation needed — the diff adds Markdown task documents and JSON bindings
only; no Rust, web, or script source changed, so there is no behavior to
cold-compile. Evidence saved under `/tmp/stage/review-evidence/`
(`01-diff-scope.txt`, `02-audit-claims-code-verification.txt`,
`03-spec-and-duplicate-work.txt`, `run-probes.sh`). The verifier's normal
mechanical checks are unaffected by Markdown/JSON additions; nothing
deferred.
