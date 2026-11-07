# Review: TASK-034 — Explorer Agent Self-Check Dry-Run Loop

**Reviewer:** Independent review (stage)
**Commit:** `fb4ca9f7` (candidate), base `233664cf8`; fix commit `eff64e31d`
**Verdict:** `approved`

---

## Scope

Diff base..candidate touches exactly the task's four files:
`crates/gyre-server/src/explorer_ws.rs` (best-version tracking in both agent
paths), `crates/gyre-adapters/src/llm/mock.rs` (scripted `complete_with_tools`
mode), `crates/gyre-server/tests/explorer_ws_integration.rs` (two WebSocket
integration tests), `specs/tasks/task-034.md` (task doc repair). The base merge
did not touch the three code files (diff `eff64e31d..fb4ca9f7` over them is
empty; the merge brought only unrelated task-087/web/trace files).

## Acceptance criteria verification

- **Extraction / dry-run / feedback / 3-turn cap / statuses** — pre-existing
  production code (`parse_view_query_from_text`, `self_check_dry_run`,
  `actionable_dry_run_warnings`, `MAX_REFINEMENT_TURNS = 3`, synthetic
  tool_use→tool_result feedback on the native path, refinement-question
  feedback on the SDK path, STATUS_REFINING/ready on the wire). Code-read and
  exercised end-to-end by the new tests.
- **Best version at the cap** (this round's fix) — both loops track
  `best_view_query: Option<(actionable_warning_count, query, text)>`,
  strictly-fewer-warnings wins, ties keep the earlier attempt; the sent
  query/explanation at the cap (and on the clean path, where current == best)
  come from the tracker. Symmetric in `run_explorer_agent` and
  `run_explorer_agent_sdk`.
- **Integration tests** — `explorer_ws_self_check_refinement_flow` (warned
  first attempt NOT forwarded, "refining" emitted, second LLM call's refined
  query is the only one forwarded, terminal "ready") and
  `explorer_ws_self_check_sends_best_version_after_cap` (exactly 3 refinement
  turns, first/best attempt forwarded after the cap). On-wire status values
  are constrained to the three contract values.

## Probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --test explorer_ws_integration` — 11 passed
  (`task-034-integration-tests.txt`, `task-034-final-clean-run.txt`).
- `cargo test -p gyre-adapters --lib llm::mock` — 9 passed
  (`task-034-mock-tests.txt`); full `-p gyre-adapters --lib` — 356 passed,
  0 failed (`task-034-adapters-lib-full.txt`, no regression from the mock
  change; script mode is opt-in and default behavior is unchanged).
- `cargo test -p gyre-server --lib explorer_ws` — 43 passed
  (`task-034-lib-tests.txt`).
- **Mutation 1** (send latest instead of best at the cap, native loop):
  `explorer_ws_self_check_sends_best_version_after_cap` FAILED with
  "got 3 callouts" — the test kills the regression it targets
  (`task-034-mutation1-no-best-version.txt`).
- **Mutation 2** (disable the refinement trigger): both new tests FAILED
  (0 refinement turns; warned query forwarded)
  (`task-034-mutation2-no-refinement.txt`).
- Mutations were temporary experiments; the working tree was restored to the
  exact candidate (`git status` clean, final clean run green).

## Notes (non-blocking)

- The cap test's comment claims attempt 3 produces "3 warnings (unresolved +
  2 substring)", but the "Coord" callout resolves via prefix match against
  `CoordinatorService` (no warning), so attempts 3/4 actually carry 2
  actionable warnings. The test's behavior and discriminating power are
  unaffected (attempt 1 remains strictly best; mutation 1 still fails it),
  but the comment overstates the premise.
- The scripted mock ignores conversation content, so the tests prove the
  loop's server-side contract (withholding, re-prompting, status, best-version
  selection) but not the feedback payload's textual content reaching the LLM.
  The feedback injection is code-verified on both paths (synthetic
  tool_use/tool_result pair; SDK refinement question).
- Cosmetic: when the best version is an earlier attempt, the current
  attempt's streamed text was already shown to the user while the view_query
  explanation carries the earlier attempt's text. Spec speaks to the query
  version, not the explanation; not a contract violation.
