# Review — task-294 (Publish reviewed spec backlog for specs/system/spec-links.md)

Spec: `specs/system/spec-links.md` — publication of the independently reviewed
missing-work plan `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`
(audit base `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`, requirements
`5a915155d9bc8230bd5286f504b41d189fdc09e58c54fc28e9df0e763fc36e91`).
Candidate under review: `90507e54fd13652df04ffa2e1c07fd987effbc2f`
(branch `pipeline/task-294/e5492c2a9bdd467581098fe5396998b4-1`, assigned base
`bacba2b29a43014b1f150787d37f159beefe919a`; the candidate is a second rebase
repair merging current main, which had gained the activity-dashboard backlog
task-295/task-296).
Verdict: **approved** — no findings. This is a definitions-and-bindings
publication; task creation is not implementation, and none is claimed.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `git diff --stat bacba2b2..90507e54` → 10 files, 1583 insertions, 0 deletions;
  only `specs/tasks/task-286.md`..`task-294.md` and `specs/task-spec-inputs.json`
  (+94 lines). No product file, script, gate, coverage matrix, or other spec
  touched (`01-diff-scope.txt`).
- Registry union check (mechanical, key-by-key against both merge parents):
  `sections` (293) and `bindings` (243) are exact unions of parent 1 (prior
  candidate) and parent 2 (main `bacba2b2`) — zero lost, mutated, or spurious
  entries, overlapping keys identical in both parents. `task-286..293.md`
  byte-identical to the prior reviewed candidate; `task-295/296.md` unchanged
  from main; task numbering does not collide with main (`03-registry-union.txt`).
- Plan fidelity: the plan JSON embedded in `specs/tasks/task-294.md` is
  structurally identical to the assigned plan (`05-plan-identical.txt`), and the
  recomputed plan digest (sha256 over the canonical JSON of the plan, matching
  `scripts/pipeline/spec_backlog.py`'s `digest`) equals the assigned
  `plan_id 88a83dc3…` (`06-plan-id-digest.txt`) — the published plan is exactly
  the reviewed plan.
- Task documents: all 8 new tasks (`task-286`..`task-293`) carry their plan
  entry's `required_behavior` verbatim, every acceptance criterion verbatim, the
  negative probe verbatim, the exact title, and the exact dependency frontmatter
  (288→287, 289→286, 293→287; others none). All are `progress: not-started`
  (`04-task-docs-verbatim.txt`).
- Bindings: each of the 9 audited gap sections is bound to exactly one new task
  key (`spec-links-approval-gates` covers the two Approval/Merge-gate sections,
  matching the plan); every bound heading exists in the canonical spec's heading
  tree; no other task in the registry owns any of these backlog section ids
  (`07-bindings-ownership.txt`). Prior tasks covering spec-links headings
  (task-016/019/023/025/045/051/198) are all `complete` — no duplicate active
  assignment.
- Audit challenge (spot-checks against candidate source, `08-audit-evidence-spotchecks.txt`):
  every sampled evidence claim reproduces — `SpecLink.target_sha: Option<String>`
  (spec_registry.rs:72), the unpinned-link `else { continue }` skip
  (spec_link_staleness.rs:36), `let ws_id = workspace_id?;` (spec_registry.rs:1068),
  the fabricated `Id::new("default")` tenant lookups (spec_link_staleness.rs:157,409),
  the five registry-exempt routes at exemption-file lines 29/37/38/40/41 with
  State/Path-only handler signatures (api/specs.rs:1776, 1841, 1858, 1905), the
  `l.direction` filtering in DetailPanel.svelte:3674-3675 with no `direction`
  field on `SpecLinkResponse`, and the approval-status predicate of merge-gate
  Check 3 (merge_processor.rs:2402). The canonical spec
  `specs/system/spec-links.md` is unchanged since the audit base `678a8ac`.
- Mechanical suite: `PYTHONPATH=scripts python3 -m unittest test_pipeline_spec_backlog`
  → 40 tests, OK (`02-pipeline-backlog-tests.txt`; the script is unittest-based,
  so it runs without pytest despite the shipped note deferring it).

## Assessment against the acceptance criteria

- **Faithful publication.** The 8 task documents and the registry additions are
  exactly the reviewed plan (cryptographically confirmed via the plan-id digest);
  nothing was added, dropped, or reworded.
- **No duplicate/missing assignment or dependency defects.** Single ownership per
  gap section; dependencies are justified by the plan's own analysis
  (status-lifecycle needs the cross-repo target-manifest lookup; approval-gates
  needs pinned SHAs; tenant-cycle-detection needs cross-repo edge attribution),
  are acyclic, and every referenced dependency task is among the published set.
- **Nothing else changed.** The diff is purely additive in `specs/tasks/` and the
  registry; every existing task, spec, registry entry, coverage status, product
  file and gate is preserved (verified mechanically against both merge parents).
- **New tasks remain `not-started`.** Confirmed in all 8 frontmatters.

No server build is required by this change; nothing for the verifier beyond the
standard pipeline checks already exercised here.
