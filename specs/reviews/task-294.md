# Review — task-294 (Publish reviewed spec backlog for specs/system/spec-links.md)

Spec: `specs/system/spec-links.md` — publication of the independently reviewed missing-work plan `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7` (audit base `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`, requirements `5a915155d9bc…`).
Candidate under review: `03d6013e5b74ca12a075e24a6517c38a645f6ff8` (branch `pipeline/task-294/1c6e822d59d94490ae9d554464549106-1`, parent = assigned base `678a8ac3`).
Verdict: **approved** — documentation-only publication, faithful to the plan, no findings. Evidence saved under `/tmp/stage/review-evidence/` (diff-stat.txt, audit-claims.txt).

## What the candidate contains

`git diff 678a8ac3..03d6013e --stat`: 10 files, 1553 insertions, 0 deletions —

- New task docs `specs/tasks/task-286.md` … `task-293.md` (the eight planned tasks, named exactly per the plan's `assigned_names`), each `progress: not-started`, `commits: []`.
- `specs/tasks/task-294.md` — the publication record embedding the full plan JSON verbatim (parsed and validated: plan_id, audit base, 9 gap sections, 8 tasks with covers/deps all match the assigned plan).
- `specs/tasks`… plus `specs/task-spec-inputs.json`: 9 new `backlog/<section-id>` section entries (headings verified against the canonical spec's heading tree) and 8 new task→sections bindings keyed by the `spec_ref` strings used in the task docs.

No other file is touched — no product code, script, spec, coverage matrix, registry entry, or gate is modified. This matches the established publication pattern (cf. `1c088af5` for task-276/277, `cae04d59` for task-277).

## Verification performed

**Diff faithfulness (acceptance 1, "publish exactly"):**
- Each task doc's frontmatter (title, spec_ref key, depends_on, progress) matches the plan's task list mapped through `assigned_names`; dependencies resolve: task-288 → task-287, task-289 → task-286, task-293 → task-287; 286/287/290/291/292 have none.
- Required behavior, acceptance criteria, and verification-intent (negative probes) sections are verbatim from the plan's `required_behavior` / `acceptance` / `negative_probe` fields — spot-compared task-286…293 in full.
- Section bindings are exact and complete: each of the 9 gap section ids appears in exactly one task doc's Discovery evidence (task-289 carries its two sections dcd440115bac… + cb0d56cefcb5…); `task-spec-inputs.json` binds the same 9 ids under the 8 task keys with no duplicates. JSON parses clean; the embedded plan JSON in task-294.md parses clean.
- Headings bound in `task-spec-inputs.json` (e.g. `Link Types`, `Cross-Repo and Cross-Workspace Links`, `Forge-Maintained Spec Graph → Link Status`, `Mechanical Gates → Approval Gates/Merge Gates/Cycle Detection`, `Querying the Graph → API/UI`, `Accountability Agent Integration`) all exist in `specs/system/spec-links.md` at the cited levels.

**Audit challenge (required by the task contract):** the load-bearing factual claims underpinning each published gap were independently re-verified against the code at the candidate (grep evidence in `/tmp/stage/review-evidence/audit-claims.txt`):
- `SpecLink.target_sha: Option<String>` (spec_registry.rs:72); staleness job `let Some(pinned_sha) = … else { continue; }` (spec_link_staleness.rs:36-38); push-time outbound check `if let Some(pinned_sha)` (spec_registry.rs:684) — SHA-pinning gap is real.
- `resolve_repo_by_name` returns None when `workspace_id` is None (`let ws_id = workspace_id?;`, spec_registry.rs:1068); fabricated `Id::new("default")` tenant in the staleness re-resolution (spec_link_staleness.rs:157) — cross-repo resolution gap is real.
- Broken-status ratchet skip guard (`Already known stale/broken — skip`, spec_link_staleness.rs:40-43); no `"conflicted"` status producer anywhere in gyre-server/gyre-adapters (grep: zero matches) — status-lifecycle gap is real.
- `implements` gate checks only `parent.approval_status != Approved` (api/specs.rs:529-537), never the pinned SHA; no `conflict-resolution` task creation (repository-wide grep: zero matches); merge Check 3 predicate is `target.approval_status != Approved` (merge_processor.rs:2402) — approval/merge gate gaps are real.
- `detect_link_cycles` adjacency built solely from `manifest.specs` of the pushed manifest (spec_registry.rs:1257-1258) — cycle-detection scope gap is real.
- `get_spec_graph(State…)` with no Query/repo parameter (api/specs.rs:1742); `get_stale_links`/`get_conflicts`/`patrol_spec_links` signatures carry no auth extractor (api/specs.rs:1841/1858/1905); exemption file lines 29, 37-41 list the patrol + four query endpoints — API/authorization gap is real.
- `SpecLinkResponse` has no `direction` field (api/specs.rs:1648-1658) while DetailPanel.svelte:3674-3675 filters on `l.direction` — UI link-direction gap is real.
- jobs.rs registers `spec_link_staleness::run_once` (:493) but no spec patrol — accountability integration gap is real.
- Requirement quotes in the audit match the canonical spec text verbatim (Link Types intro, Link Status table, Approval/Merge Gates tables, Cycle Detection, API/UI bullets).

All audit dispositions ("gap" and "satisfied") that I re-checked are factually accurate at this tree; none of the published gap claims is hollow or contradicted by the code.

**No duplicate/missing assignment (acceptance 2):** every prior spec-links task (016, 019, 023, 025, 045, 051, 198) is `progress: complete`; the other open tasks (094, 199, 228, 267, 271, 273) target other specs. No task number collision (existing tasks end at 277; 278-285 are not defined in this tree, and the plan's assigned names 286-294 are used exactly).

**No product change (acceptance 3):** diff is additions-only within `specs/tasks/` and `specs/task-spec-inputs.json`; nothing under `crates/`, `web/src/`, `scripts/`, `specs/coverage/`, `specs/system/`, or `specs/personas/` is modified. The new `task-spec-inputs.json` entries are additive — no existing section or binding was altered or removed.

**Not-started discipline (acceptance 4):** all eight new task docs declare `progress: not-started` and empty `commits`; the publication commit is not task-labeled, so no commit-attribution requirement is triggered.

## Notes for the verifier (non-blocking)

- This is a docs-only publication; no compilation or test surface is affected. The verifier's standard mechanical checks (`check-arch.sh`, ABAC registry/exempt-handler checks, migration checks) operate on product files untouched by this diff.
- The task numbers 278-285 are absent from this tree (presumably reserved by the controller for other plans); if any parallel branch assigns those numbers, the merge order should be checked there — not a defect of this candidate.
