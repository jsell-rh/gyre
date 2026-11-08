# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (lines 810-820); cross-referenced `specs/system/explorer-canvas.md` §3 "Lens Definitions".

Assigned base `637be718`, candidate head `13693fba` (branch `pipeline/task-090/4bc383b9ab8241cfafa02ab4af25c73e-1`). Product-surface commits attributed in `commits:` frontmatter: `0766a167` (implementation) and `7bb23bca` (gates.rs lint fix); the rest are process/merge commits. Working tree clean at review time.

Verdict: **not approved** — one code finding (gate-status data source is not repo-scoped). Everything else in the task contract is genuinely implemented and well-tested; the fix is a one-line parameter rename.

## Probe evidence (all logs under /tmp/stage/review-evidence)

- `npm ci` in `web/` (locked versions), then `npx vitest run` on the six suites covering both merge-conflicted files (lens-rendering, ExplorerViewLensUrl, ExplorerCanvas, ExplorerCanvasTrace, causal-trace, ExplorerViewLargeGraph): **238/238 passed, exit 0** (`task090-review-vitest-lens-suites.log`).
- Full `npm test` in `web/` (the acceptance criterion): **75 files, 1798 passed | 41 skipped, exit 0** (`task090-review-npm-test-full.log`).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::gates`: **7 passed, exit 0**, including `list_spec_approvals_includes_rejection_fields` — the ledger rejection-fields round-trip survives the merge with task-193's ledger changes (`task090-review-cargo-gates.log`).
- Live server probe (`task090-probe.sh`, `task090-req.mjs`, `task090-probe-output.log`, `task090-probe-server.log`): debug server built via `/tmp/stage/bin/cargo build -p gyre-server` (`task090-review-server-build.log`), started on port 39017 with an isolated DB, two repos seeded in one tenant, one MR with `spec_ref specs/system/auth.md@aaa…` created in repo-alpha only:
  - `GET /api/v1/merge-requests?repo_id=<repo-beta-id>` → **200, returned repo-alpha's MR** (param ignored; all tenant MRs returned).
  - `GET /api/v1/merge-requests?repository_id=<repo-beta-id>` → 200 `[]` (correctly scoped).
  - `GET /api/v1/merge-requests?repository_id=<repo-alpha-id>` → 200, alpha's MR (correctly scoped).
  - `GET /api/v1/merge-requests?repo_id=<repo-alpha-id>&per_page=0` → 200, still returned the MR (`per_page` ignored).
  - Server stopped afterwards; tree verified clean.

## What is genuinely implemented (verified, no findings)

- **Lens selector** (`ExplorerCanvas.svelte` toolbar, ~line 5531): Structural (default, `aria-pressed`), Evaluative, Observable (`aria-disabled="true"`, tooltip "requires production telemetry integration", clicking only shows the ObservableBanner). Verified in markup and by component tests.
- **Evaluative node decorations** (`drawLeafNode`, ~3307): coverage badge (green ≥70% / yellow ≥30% / red below, `GraphNode.test_coverage`), gate status ring (green/red), approval icon (✓/⏳/✗) — all drawn on top of the unchanged structural fill/spec-border/label. Tests are differential draw-call captures: badge drawn green at 85% in evaluative, **NOT drawn in structural**; approval icons ✓/⏳/✗ asserted by glyph+color and absent in structural; gate red vs green asserted via stroke-color deltas against a gate-less baseline. These are real regression killers, not mirrored logic.
- **No re-layout on lens switch**: test captures `fillText` positions for a node in structural, clicks Evaluative, flushes rAF, asserts identical x/y plus legend change proving the re-render happened.
- **Gate edge colors** (`drawEdges`, ~3679): edges leaving a node governed by a spec with a concluded gate run are green/red, precomputed per node (`gateStatusByNodeId`), O(1) per edge.
- **Spec approval**: `SpecApprovalResponse` now serializes `rejected_at`/`rejected_reason` (gates.rs), with a genuine server round-trip test distinguishing rejected (✗) from revoked (back to ⏳ pending). `approvalStatusForRecords` semantics (any active → approved; else latest decides) unit-tested including revoked→pending.
- **URL persistence**: `lensFromUrl`/`lensToUrl` pure helpers; ExplorerView restores from `?lens=evaluative` deep links on mount and syncs via `history.replaceState` (Back not polluted; structural default drops the param; other params and `#drill=` preserved). Five component tests exercise real clicks and read back `window.location.href`.
- **Node→spec join** (`nodeSpecPaths`): `spec_path` + N:M `spec_paths` + GovernedBy edge targets (soft-deleted edges ignored), unit-tested.
- **Spec assertion badges** on governed nodes: pre-existing genuine implementation (`assertionBadges`, ExplorerCanvas ~5374/3577), unchanged by this task.
- **Merge with assignment base 637be718** (task-193's spec-approval attestation work): both import groups kept in the two conflicted files; lens data flow (`loadEvaluativeData` → `gateStatusBySpec`/`specApprovals` → props) and trace data flow both intact in the merged tree; all 238 focused tests pass on the merged source.
- Coverage matrix row 23 (Three Lenses) updated to `implemented` with an accurate description; task acceptance checkboxes left unchecked for independent review, as assigned.

## Finding (blocking)

**`loadEvaluativeData` is not repo-scoped — gate-status overlay can attribute other repos' gate outcomes to the viewed repo.** `web/src/components/ExplorerView.svelte` ~line 1039 (product commit `0766a167`):

```js
const mrs = await api.mergeRequests({ repo_id: repoId, per_page: 50 });
```

`GET /api/v1/merge-requests` accepts only `status` / `repository_id` / `workspace_id` (`ListMrsQuery`, `crates/gyre-server/src/api/merge_requests.rs:40-45`; documented in `docs/api-reference.md`; every other frontend call site — DetailPanel.svelte:890, RepoMode.svelte:212, RepoSettings.svelte:191, ExplorerCodeTab.svelte:165 — uses `repository_id`). Serde ignores the unknown `repo_id`/`per_page` params, so the handler takes the `(None, None)` arm and returns **all tenant MRs**. The subsequent join (`byPath`) is keyed purely by spec path with no repo discrimination, and the Explorer shows one repo's graph at a time. Consequences in any multi-repo tenant where repos share spec paths (the platform's own convention — every repo carries `specs/system/*.md`):

- The "most recent gate result" for a spec path can come from **another repo's MR**, so the viewed repo's nodes show gate rings/edges derived from foreign MRs — including **false-green** when another repo's same-path MR passed more recently than this repo's own failing run. Gate coloring is a decision signal ("a failing gate on any governing spec fails the node"); a false pass is the worst failure mode.
- The intended 50-MR recency window is not applied (all tenant MRs fetched), and gates are then fetched sequentially per MR until each path resolves — unbounded request fan-out in large tenants.

Empirically proven against a running server (probe above): `?repo_id=<beta-id>` returned alpha's MR; `?repository_id=<beta-id>` returned `[]`. The component tests cannot catch this because they mock `api.mergeRequests` and inject `gateStatusBySpec` directly — the query contract is never exercised.

Fix: `repository_id: repoId` (drop or server-support `per_page` separately). Note for context: the identical `repo_id` misuse in `loadTraceData` (ExplorerView.svelte:1001) is pre-existing from #627 and out of this candidate's scope, but the new code copied the broken pattern.

## Minor (non-blocking, recorded for completeness)

- `GraphNode.test_coverage` is never populated by any production extractor today (rust/go/python/typescript extractors and graph_extraction.rs all set `None`); the coverage badge therefore renders only when coverage data has been upserted through the graph API. The spec's designated data source for the coverage overlay is exactly this field, the None case is handled gracefully, and population is a separate analyzer concern — not a defect of this task, but worth a follow-up task if live coverage badges are expected.
- The spec-approvals fetch (`GET /api/v1/specs/approvals`, no path filter) is tenant-wide, matching the ledger's own path-keyed identity model — acceptable, unlike the MR join above.
