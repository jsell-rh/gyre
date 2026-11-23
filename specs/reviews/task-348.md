# Review — task-348 (Publish reviewed spec backlog for specs/development/agent-experience.md)

Spec: `specs/development/agent-experience.md` (all six child sections; five audited gaps, one satisfied, one context parent).
Candidate under review: `f84ae70aff2e447cb20bedb87dc60a15076bffc3` (base `f3ca7db3bf7ee4b35cab550ad18011c13d8680c4`; includes merge `453db337` of main's task-341/task-342 dependency-graph publication and reconcile commit `f84ae70a`).
Verdict: **approved** — zero findings. This is a documentation-only publication (task documents + registry bindings); no product code changed, so no runtime probes apply beyond the durable stage-chain suite.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- `git diff --stat base..candidate` → exactly 7 files: `specs/task-spec-inputs.json` (+50) and six new `specs/tasks/task-34{3..8}.md`. No product file, gate, spec, coverage row, or existing task touched; `specs/development/agent-experience.md` and `specs/coverage/` byte-identical to base.
- `check_union2.py` / `check_dups.py` (programmatic, both parents `0178c025` and `f3ca7db3`) → the merged registry is a per-key verbatim union: sections 329 shared + 5 branch (agent-experience backlog) + 1 main (dependency-graph) = 335; bindings 272 + 5 + 1 = 278. No entry missing, rewritten, or added from neither parent; the duplicate binding values present in the file are byte-identical pre-existing sets in both parents.
- `check_plan.py` / `check_covers.py` (parses the plan JSON embedded in `specs/tasks/task-348.md` and compares each task doc) → all five docs (task-343..347) carry the plan's title, required_behavior, every acceptance criterion, negative_probe, mapped depends_on (task-344 → task-343; task-347 → task-346), and discovery evidence whose section ids/decisions/requirement_quotes match the audit exactly.
- Duplicate-work: each of the five `backlog/<section-id>` ids is bound by exactly one registry key and appears in exactly one registry section; no other task document references agent-experience (grep over `specs/tasks/`); both dependency targets (task-343, task-346) exist in this backlog; no cycles.
- `python3 -m unittest test_pipeline_spec_backlog` in `scripts/` → 40 tests, all pass (exit 0), including `validate_manifest`'s no-rewrite rules.
- `bash scripts/check-task-commit-attribution.sh` → OK (the `process(task-348)` commits are correctly exempt as process rounds).
- Audit challenged against the current tree: `pub agent_logs: Arc<log_compression::AgentLogBuffer>` in-memory (lib.rs:368/2147), zero `fixup` matches across `crates/`, exactly one seeded REQUIRED `MetaSpecKind::Principle` (conventional-commits, lib.rs:2792), zero rows for `ui-navigation`/`repo-lifecycle`/`system/ralph-loop` in `specs/index.md`, zero `deviceScaleFactor`/`setViewportSize` in `web/tests`, and `check-cli-spec-parity` absent from `.github/workflows/ci.yml` — every "gap" decision in the audit is real, and the "satisfied" (Agent Legibility) and "context" (parent heading) decisions carry accurate evidence. The canonical spec's heading structure (intro + the six audited child sections) matches the registry bindings' heading paths verbatim.

## Assessment

- The five planned tasks faithfully decompose the five audited gap sections; each task's required_behavior, acceptance criteria, and negative probe are concrete, enforceable, and pinned to the canonical spec's actual wording (including the controller review feedback `fd6e133d1621c46338a657b0:unauthorized-cross-tenant-fixup-write-path` correction baked into task-346).
- Dependencies are correct and minimal: task-344 → task-343 (sensory access builds on the telemetry store), task-347 → task-346 (doc-garden uses `ensure_fixup_task`); coordination with task-292's frozen patrol contract is explicitly scoped rather than duplicated.
- The merge reconciliation with main's task-341/task-342 publication is a clean verbatim union with no existing binding rewritten and no duplicate active assignment introduced.
- New implementation tasks remain `not-started`; task-348 (the publication process task itself) is `ready-for-review`, matching the sibling task-342 publication pattern.
- Task creation is not implementation — none of the five tasks claims any product work, and no coverage status changed.

## Verifier notes

No server compilation or runtime probes are required for this candidate (docs-only diff). The deterministic verifier should still run its standard mechanical fleet (including `check-task-commit-attribution.sh`, which needs full history) and regenerate `specs/coverage/SUMMARY.md` from the merged matrices.
