# Review — task-088 (HSI Explorer Default Views)

Spec: `human-system-interface.md` §3 "Default Views (Automatic, No LLM)".
Candidate: `909c04e1c` (base `2100127c4`); feature commits `8e025c257`
(views + selector), `3463e2858` (MR-timeline delta-count fix), `4be0f8b2a`
(docs), rebases `531ca48b7`/`cd37fa928`/`d4e72e32`, baseline repairs
`8407bd1cf`/`dc0539434`, attribution records `a517589f2`/`c6d1174b2`/`909c04e1c`.
Full evidence: `/tmp/stage/review-evidence/evidence.md`.

## Round 1 — independent review (approved)

All four default views are real, data-derived implementations behind a
view-selector tablist in the Explorer control bar (Boundary default); no LLM
anywhere in the paths.

Verified working (independent probes at the exact candidate head):

- **Data paths are real end-to-end.** `X-Gyre-Last-Seen` is genuinely stamped by
  `last_seen_middleware` (middleware.rs:104-175, 60s debounce, header only on
  tracked responses) and captured centrally in `api.js request()` →
  `last-seen.js`, which snapshots the pre-visit value once per page load so
  "since last visit" anchors to the *previous* visit (24h fallback on first
  visit, matching the Briefing's since-resolution in graph.rs). ChangeView
  queries the real `GET /repos/:id/graph/timeline?since=` handler
  (graph.rs:687-703 → `list_deltas`). SpecRealizationView links via
  GovernedBy edges + spec_path/spec_paths against the real repo graph;
  SystemTraceView consumes the real `GET /merge-requests/:id/timeline` and
  resolves View Conversation through the attestation bundle's
  `conversation_sha` (gyre-domain attestation.rs:82) → `GET /conversations/:sha`.
  Boundary Level 1 counts come from the real workspace graph
  (`GET /workspaces/:id/graph`) + `GET /repos/:id/dependencies`; Level 4 spec
  linkage/churn/complexity/last-modified render in NodeDetailPanel
  (:249-265, :314-335, :832-835).
- **Backend fix is correct and necessary.** `graph_extraction.rs:362-399`
  serialises `nodes_added`/`nodes_removed` as arrays (and `nodes_modified` as
  array in the agent-ctx branch, bare count otherwise); the old
  `parse_delta_counts` accepted only bare numbers and probed nonexistent
  `added`/`modified` keys, so real GraphExtraction events reported 0/0. The
  fix handles both shapes; the producer-shape regression test kills the
  reintroduced bug (verified by mutation: array-count → 0 fails the test).
- **Tests are real, not inflated.** Focused vitest on the five new suites: 34
  passed. Mutation probes: `since` → 0 fails the ChangeView since-propagation
  test; default view → 'change' fails two ExplorerDefaultViews tests; each
  killed and reverted.
- **Full acceptance suite:** `npm test` in web/ — 81 files, 1839 passed,
  41 skipped (exit 0), including ExplorerCanvasDrillDown (Levels 2-4).
- **Rust:** `cargo test -p gyre-server --lib api::mr_timeline` — 13 passed.
- **Visual baseline repairs are legitimate.** No locator/tolerance/test-logic
  changes accompany the PNG updates (diff is PNG-only for tests/e2e); baseline
  dimensions are consistent with the documented toolbar drift. Independent
  server-backed run (fresh `npm run build`, debug gyre-server on :2222,
  Playwright chromium): all 16 explorer-visual tests pass at the candidate
  head. Mutation probe (hiding `.explorer-view-tabs`, rebuilt web): the
  regenerated baselines fail with the inverse size mismatch — they genuinely
  pin the merged toolbar.
- **Mechanical gates:** `check-task-commit-attribution.sh` exit 0 (the
  d4e72e32 cross-attribution recorded in task-083.md is the sanctioned fix,
  frontmatter + provenance note only, no product change). No new routes,
  migrations, MCP tools, or MessageKind variants in the diff — no additional
  registry obligations.

Observations (non-blocking, recorded for the next cycle):

- ChangeView: modified entries with field_changes toggle the field-change
  expansion on click and do not navigate to Boundary even when resolvable
  (tooltip says 'View in Boundary View' for that case). The task plan itself
  specifies expansion for modified entries, the behavior is explicitly pinned
  by a test, and all other entry kinds navigate — cosmetic affordance nuance,
  not a contract breach.
- Coverage row 20 (HSI §3 Default Views) remains `task-assigned`; the flip to
  implemented follows the post-review pipeline convention (cf. task-084/085).

Verdict: **approved** — independent evidence supports the task contract;
`/tmp/stage/verdict.json` records `approved: true` with no findings.
