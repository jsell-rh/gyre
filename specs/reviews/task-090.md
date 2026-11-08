# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (+ `specs/system/explorer-canvas.md` §3/§4 lens definitions).
Candidate: `64598aca63e620eabc9c906fbb2fbd1ac7ac3f17` (branch `pipeline/task-090/a54b5cb1824c4e75b36aa8ca0125441c-1`), base `98a5299b905609adf4f309bceb855e5938d4405e`.
Verdict: **needs-revision** (1 blocking finding — shipped artifact, not source).

## Probes run (evidence: /tmp/stage/review-evidence)

- `npm ci` + full `npm test` in `web/`: **85 files, 1899 passed / 41 skipped, exit 0** (npm-test.log).
- `/tmp/stage/bin/cargo test -p gyre-server --lib api::gates`: **7 passed** incl. the new `list_spec_approvals_includes_rejection_fields` (cargo-gates-test.log).
- Focused: `npx vitest run lens-rendering.test.js ExplorerViewLensUrl.test.js` → 33 passed; `ExplorerCanvas.test.js -t "Three Lenses"` → 8 passed.
- Mutation tests (temporarily disabling production code, then restored; working tree verified clean):
  1. Neutered the per-spec gate edge coloring in `drawEdges` (`gateStatusByNodeId.get(srcId)` → `null`) → the differential edge-stroke test FAILS (mutation1-gate-edge.log). Test is real.
  2. Disabled the approval-icon draw block → the ✓/⏳/✗ icon test FAILS (mutation2-approval-icon.log). Test is real.
- E2E baseline verification: committed `toolbar-evaluative-linux.png` (919x133) is **sha256-identical** to the actual PNG captured by the failing CI run 42503691204 at PR head 55064c8dc (`74f3c722…`), vs the stale expected 919x103. `web/tests/e2e/` spec file unchanged in the diff (same locator, same `maxDiffPixelRatio: 0.02`); `toolbar-default-linux.png` untouched. The CI repair is legitimate — the toolbar legitimately grew because the task-090 evaluative legend (Coverage ok/partial/low, Gate pass/fail, Approved/Pending/Rejected — asserted by unit tests) wraps onto an extra flex row.
- `bash scripts/check-task-commit-attribution.sh` → OK.

## Verified working (source level)

- Lens selector in the ExplorerCanvas toolbar: Structural (default/active), Evaluative, Observable `aria-disabled` with "requires production telemetry integration" tooltip + ObservableBanner. Observable normalizes back to structural (`normalizeLens`) — cannot be activated, per spec "future".
- Evaluative overlay is real and composed on the structural base (drawn on top of unchanged fill/spec border/label): coverage badge (green ≥70 / yellow ≥30 / red, from `GraphNode.test_coverage`), gate status ring + edge colors (green pass / red fail, most-recent-MR-per-spec-path recency join via `spec_ref.split('@')[0]`, latest run per gate), approval icons ✓/⏳/✗ from the ledger (`GET /api/v1/specs/approvals`; `SpecApprovalResponse` now serializes `rejected_at`/`rejected_reason` so rejection ≠ revocation — server round-trip test included). API shapes verified against the server (`ListMrsQuery.repository_id`/`per_page`, gate result `status`/`gate_id`/`started_at`, `spec_ref` "path@sha").
- URL persistence: `?lens=evaluative` restored on mount, synced via `history.replaceState` on switch, structural default drops the param, other params + `#drill=` hash preserved. Deep-link restore, toggle, and drop are component-mount tested.
- No re-layout on lens switch: test asserts identical node label draw positions across the switch plus legend change (re-render really happened).
- Repo scoping: the evaluative MR query uses `repository_id` (never the silently-ignored `repo_id`), `per_page: 10`, newest-first with client-side sort — tested in ExplorerViewLensUrl.test.js.
- Spec assertion pass/fail badges (spec table's "spec assertion status") pre-existed (`assertionBadges`, §9) — unchanged by this diff, still genuine.
- Coverage matrix row 23 (HSI §3 Three Lenses) honestly updated task-assigned → implemented with accurate evidence. (Nit: the note says "5 URL persistence tests"; the file has 6.)

## Findings

- [ ] **F1 (blocking, shipped artifact): the candidate's committed `web/dist` is the pre-task-090 BASE bundle — every task-090 UI feature is absent from the served SPA.** `git diff base..HEAD -- web/dist` is empty; HEAD's `index-B6rm8sbX.js`/`elk.bundled-DfJETqTK.js` are byte-identical (sha256 `7a96bfc3…`/`ff6f92e7…`) to base's. Commit `54c0b32ab` ("build(web): regenerate verified assets (task-090)") had committed the feature-complete bundle (`index-QXRGsaeH.js` — which a fresh `npm run build` of HEAD source still produces, and which the failing CI run rendered); checkpoint commit `86b6f3006` (in the task's `commits:` frontmatter) then reverted dist to the base bytes. The HEAD bundle has 0 occurrences of `gateStatusBySpec`/`specApprovals`/`approvalRecordsByPath` (vs 4/2/2 in the feature bundle), no `searchParams.get("lens")`/`delete("lens")` URL persistence, no Coverage ok/partial/low legend labels, no approval icons. `gyre-server` embeds `web/dist` via rust-embed (`spa.rs`), so the documented `SKIP_WEB_BUILD=1` production build mode (AGENTS.md) serves a UI with **no lens features at all**; and the regenerated `toolbar-evaluative-linux.png` baseline (919x133, expecting the task-090 legend) is inconsistent with the committed dist, so any `SKIP_WEB_BUILD=1` e2e/release run fails `evaluative_lens_toggle_changes_toolbar`. CI passes only because both workflows rebuild web assets. Remedy: re-run the verified asset build from HEAD source (restore the `54c0b32ab` dist state) so the committed bundle matches the source. Full evidence: `/tmp/stage/review-evidence/stale-dist-finding.md`.

## Non-blocking observations (recorded for completeness)

- `GraphNode.test_coverage` has no production writer (all extractors set `None`; only tests set values), so the coverage badge is data-driven but dark until a coverage-extraction producer lands. Pre-existing condition, outside this diff; the spec names the field itself as the data source and the frontend handles `None` correctly (no badge). Populating it belongs to a backend extraction task (cf. task-068's graph-summary coverage work).
- `loadEvaluativeData` fetches the spec-approval ledger unscoped (per spec-path key, not repo-scoped); spec paths are the ledger's key per the spec's data source, so this matches the contract, though same-named spec paths across repos in a tenant would share approval status.
