# Review — task-186 (Ghost Overlays, system-explorer.md §3)

**Candidate:** `9188fc2982626bc9d9b9651d768ad6f9344c60a0` (base `6082be2f`)
**Round:** 0b52dfad (repair round for findings F1/F2 of round 74d337d3)
**Verdict:** ❌ not approved — one new test finding (F3)

## Repairs verified (round 74d337d3 findings)

- **F1 (fixed):** the thorough-preview test now expects `preview/search-md-`
  — matches the spec-stem sanitizer's char map (verified by direct
  simulation: `'search.md'` → `search-md`). Commit `44310fa1`.
- **F2 (fixed):** the missing-`spec_path` case now asserts 422
  Unprocessable Entity, matching axum 0.7.9 `Json<T>` extractor rejection
  (same contract as release.rs). Commit `44310fa1`.
- **fmt:** `cargo fmt -p gyre-server -- --check` clean (the candidate commit's
  ten unformatted regions were repaired in the same commit).

## What was independently verified

- **Server (type-level, per budget rules):** `SKIP_WEB_BUILD=1 cargo check
  -p gyre-server --lib --tests` clean (8m21s, log:
  `/tmp/stage/review-evidence/t186-cargo-check.log`). Server test execution
  is the verifier's gate — required list is in the task's Shipped notes and
  matches the four new tests plus the predict/spawn regressions.
- **Mechanical guards:** ABAC route registry, architecture lint, mem port
  contracts all pass; the new `thorough-preview` route is registered in the
  resolver with `graph:write` (no exemption-file entries added).
- **Frontend:** `npm ci` + focused suites — `ghostOverlay.test.js` +
  `GhostOverlay.test.js` (42 passed), `EditorSplit.test.js` +
  `ArchPreviewCanvas.test.js` + `DetailPanel.test.js` (136 passed). Logs under
  `/tmp/stage/review-evidence/`.
- **Mutation probes (tests kill real bugs):** constant opacity mutation →
  5 failures; add-stroke color mutation → failure. Both restored.
- **Wire-shape cross-checks:** spec-links response fields match what the
  impact bar consumes; `list_by_spec_path` exists on the TaskPort;
  `spec_links_store` is the pre-existing durable-backed hot cache; the
  predict rate limit is pre-existing in the base.

## Finding F3 (blocking) — thorough-tier poll proven only via fictional statuses

`pollPreviewTask` completes on `'completed' || 'review' || 'done'` and fails on
`'failed' || 'blocked' || 'cancelled'`, but `GET /tasks/{id}` serializes status
only as `backlog/in_progress/review/done/blocked/cancelled`
(`task_status_str`, api/tasks.rs:123). Every thorough-preview test mocks
`{status:'completed'}` / `{status:'running'}` — statuses that never appear on
the wire. Mutation probe: deleting `'review' || 'done'` from the completion
condition leaves all 60 EditorSplit tests green — i.e. the only wire-real
completion path is untested, and its regression would break the acceptance
criterion (ghosts never become solid; poll times out into failPreview) for
every real user while the suite passes. The failure path is the same
(`'failed'` fictional; `'blocked'`/`'cancelled'` untested).

Evidence: `/tmp/stage/review-evidence/finding-F3-fictional-task-statuses.md`.

**Repair:** mock the real vocabulary in the thorough-tier tests —
`{status:'in_progress'}` then `{status:'review'}` and/or `{status:'done'}` for
completion, `{status:'blocked'}`/`{status:'cancelled'}` for failure.

## Non-blocking observations

- The coverage matrix row for system-explorer.md §3
  (specs/coverage/system/system-explorer.md line 19) still reads
  `task-assigned`; the shipped-notes convention used by sibling tasks (e.g.
  task-209) flips the row at the process commit. Understatement, not fakery —
  safe direction; flagging for the controller/verifier's matrix handling.
- `GhostOverlay.test.js` and `ghostOverlay.test.js` differ only in case — fine
  on Linux/CI, would collide on case-insensitive checkouts.
- Generated `web/dist/` was not rebuilt in this checkpoint (verifier-owned);
  no source defect follows from that.

## Verifier must execute (server)

- `cargo test -p gyre-server --lib api::graph::tests::predict_graph_spec_edit_returns_ghost_contract_and_link_context`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_creates_task_and_spawns_agent_on_throwaway_branch`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rejects_missing_fields_and_unknown_repo`
- `cargo test -p gyre-server --lib api::graph::tests::thorough_preview_rolls_back_preview_task_when_spawn_fails`
- `cargo test -p gyre-server --lib api::graph::tests::predict_graph`
- `cargo test -p gyre-server --lib api::spawn`
