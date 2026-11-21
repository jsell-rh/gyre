# Review — task-294 (Publish reviewed spec backlog for specs/system/spec-links.md)

Spec: publication of the independently reviewed missing-work plan for
`specs/system/spec-links.md` (plan_id `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`,
audit `36545b6c5836807360743e3a315efdda46941c3b9d51c4042fdca8c01d3b113e`).
Candidate under review: `5ff5ea49` (branch `pipeline/task-294/9c3b5c786c6a44818878d6da281a74bc-1`,
rebase repair: merge of pipeline parent `7666f82e` with main `9e7cf62e`; assigned base
`9e7cf62e4b3318b9e5aba9d71ab2a2fb64dcc6ce`).
Verdict: **approved** — no findings. This is a second-round (rebase-repair) approval of the
same publication; the prior round's approval did not transfer and was re-derived from
scratch against the exact current candidate.

## Probes run (checkout at candidate `5ff5ea49`, clean tree; evidence in /tmp/stage/review-evidence/)

- `git diff --stat 9e7cf62e..5ff5ea49` → 10 files, +1576/−0: `specs/tasks/task-286..294.md`
  and additive `specs/task-spec-inputs.json` changes only. Nothing else in the tree changed.
- Mechanical per-task comparison of all 8 task docs (task-286..293) against the assigned
  plan extracted from the assignment: title, required behavior, acceptance criteria and
  negative probe are **byte-exact** for every task; dependency frontmatter matches the plan
  mapping (status-lifecycle→cross-repo-resolution = 288→287, approval-gates→sha-pinning =
  289→286, tenant-cycle-detection→cross-repo-resolution = 293→287); all eight are
  `progress: not-started` with `commits: []`.
- `specs/task-spec-inputs.json` parses; relative to base: sections 283→292 (9 added, zero
  lost, zero mutated), bindings 234→242 (8 added, zero lost, zero mutated) — an exact union
  of the reviewed publication with main's dependency-graph entries, which is exactly what
  the rebase repair had to achieve. Each of the 9 audited gap sections is bound by exactly
  one task key (approval-gates covers dcd44011+cb0d56ce per the plan's `covers`).
- All 9 bound heading paths exist in the canonical `specs/system/spec-links.md`
  (verified programmatically against the file's heading tree): Link Types,
  Cross-Repo and Cross-Workspace Links, Link Status, Approval Gates, Merge Gates,
  Cycle Detection, Querying the Graph/API, Querying the Graph/UI,
  Accountability Agent Integration.
- The embedded plan JSON in `specs/tasks/task-294.md` is byte-identical to the assigned
  plan object (including audit, tasks and assigned_names).
- Audit challenge: the audited source files are unchanged between audit base `678a8ac` and
  assigned base `9e7cf62e` (empty diff), so the audit is not stale. Spot-checked evidence
  quotes all confirmed in current source: `spec_registry.rs:72` `target_sha: Option<String>`;
  `spec_link_staleness.rs:36` unpinned-link skip; `spec_registry.rs:1068`
  `let ws_id = workspace_id?;`; `spec_link_staleness.rs:157` `Id::new("default")` fabricated
  tenant; `scripts/abac-route-registry-exemptions.txt:29,37-41`; `DetailPanel.svelte:3674-3675`
  direction filtering with no `direction` field in `SpecLinkResponse`;
  `get_spec_graph(State)` only at `api/specs.rs:1742`. The decomposition (8 tasks over the 9
  gap sections, dependencies justified: status-lifecycle and tenant-cycle-detection need the
  cross-repo target-manifest resolution; approval-gates' implements pin check needs SHA
  pinning) and the duplicate-work analysis (`existing_task: null` for all — confirmed: no
  pre-existing task doc references any of the new backlog section ids) both hold.
- Merge integrity vs pipeline parent `7666f82e`: only additions (main's task-278..285 docs
  and registry entries) plus task-294's own progress reset complete→ready-for-review and
  Shipped-note update — self-lifecycle of the publication task for this rebase-repair
  candidate, not a protected change.
- Mechanical checks all pass (exit 0): `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-fabricated-scope-defaults.sh`, `check-task-commit-attribution.sh` (the task-294
  labeled commits touch only `specs/`, not product surface, so `commits: []` is compliant).
- `PYTHONPATH=scripts python3 -m unittest test_pipeline_spec_backlog` → 40 tests, OK —
  the publication chain machinery (validate_audit/validate_plan/validate_manifest/allocate)
  exercised end-to-end in a temp forge. No pytest needed (the module is unittest-based).
- Coverage matrix, `specs/system/spec-links.md`, `specs/GOAL.md`, `specs/index.md` are
  untouched by the diff (empty).

## Acceptance criteria

- Every planned task faithfully implements its assigned canonical requirements — the eight
  task docs are byte-exact publications of the reviewed plan, with controller review
  feedback (status reversion, zero-task merge-warning predicate, exempt-endpoint
  authorization, MCP read-only boundary) carried verbatim into the task texts.
- No duplicate active assignment or missing dependency — each of the 9 gap sections is
  bound exactly once; the dependency edges match the plan; no pre-existing task binds
  these sections.
- No product behavior, spec requirement, coverage status or enforcement changed — the diff
  is purely additive `specs/` documents; the merge resolution lost nothing from either side.
- New tasks remain `not-started` until this backlog publication merges — all eight are
  `not-started` with `commits: []`.

No server build is required: the diff contains no Rust, web, or script source. The verifier
may re-run `PYTHONPATH=scripts python3 -m unittest test_pipeline_spec_backlog` (passes
locally) and the four mechanical checks above; no server tests are bound to this change.
