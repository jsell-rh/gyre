# Review — task-090 HSI Explorer Three Lenses (Structural, Evaluative, Observable)

**Candidate:** `717fe84224aff292d786fc2651d4bd6ae5b90cdc` (branch
`pipeline/task-090/c1364c8cb0ed4794b70c038274df3b20-1`)
**Base:** `078ec12a5745379c254fe7dd00261f02e28d86cb`
**Verdict:** APPROVED (findings: none blocking; two non-blocking notes below)
**Evidence:** `/tmp/stage/review-evidence/task-090/`

## Scope reality

The assigned base already contains the task-090 implementation: the
independently reviewed checkpoint `d00990bc` is an ancestor of the base, so
the reconciliation merge `7d3313d51` is tree-identical to the base, and the
candidate differs from the base only in `specs/tasks/task-090.md` (record
updates). All 9 commits in the task's `commits:` frontmatter are ancestors
of the candidate. The review therefore validates the lens features as they
exist at candidate HEAD — which is what ships.

## Independent verification

- **Focused lens suites** (`lens-rendering`, `ExplorerCanvas`,
  `ExplorerCanvas.evaluative-lens`, `ExplorerViewLensUrl`): 195/195 passed,
  exit 0 (`npm ci` first, locked versions).
- **Backend**: `cargo test -p gyre-server --lib api::gates` — 12 passed,
  exit 0, including `list_spec_approvals_includes_rejection_fields` (asserts
  `rejected_at`/`rejected_reason` serialization on the real handler).
- **Full frontend gate**: `npm test` in `web/` — 99 files, 2010 passed,
  41 skipped, exit 0.
- **Mutation probe 1** — disabling `evaluativeDecorationsByNodeId` (the
  evaluative node-decoration pipeline) fails 3 tests: coverage badge, gate
  ring, approval icons. Differential proof; not self-confirming.
- **Mutation probe 2** — disabling only the per-spec gate edge-coloring
  block in `drawEdges` fails the edge-stroke test, which discriminates edge
  strokes from node rings by path geometry (1 lineTo, 0 quadraticCurveTo).
  Both mutations were restored; the working tree is byte-identical to the
  controller's `review-before.json` snapshot (content+mode sha256 match;
  `git status` clean).

## Acceptance criteria — all verified

- Lens selector with all three lenses in the ExplorerCanvas toolbar;
  Observable is `aria-disabled` with "requires production telemetry
  integration" text (click shows the ObservableBanner, never activates).
- Structural default; knowledge-graph nodes/edges from
  `GET /api/v1/repos/:id/graph`.
- Evaluative overlays compose on the structural base: coverage badge,
  gate-status ring, approval icon drawn on top of unchanged structural
  fill/border/label; node label positions are byte-identical across a lens
  switch (tested), so switching never re-layouts.
- Gate edge colors green/red from most recent gate results: real join —
  repo-scoped MR list (`repository_id` + server-side `per_page`, newest
  first; `ListMrsQuery` genuinely supports both) → `mr.spec_ref` path →
  latest run per gate decides; a failing gate on any governing spec fails
  the node.
- Approval icons ✓/⏳/✗ from `GET /api/v1/specs/approvals`;
  `SpecApprovalResponse` carries `rejected_at`/`rejected_reason` so
  rejection (✗) is distinct from revocation (back to ⏳).
- `?lens=evaluative` URL persistence: deep-link restore on mount,
  `history.replaceState` on switch, structural default drops the param,
  other params and `#drill=` hash preserved (unit + component tests).
- Visual baseline regeneration `9ebea6a428` changed only the PNG (same
  locator, same `maxDiffPixelRatio`); no assertion weakened.

## Non-blocking notes (for follow-up, not candidate defects)

1. **`GraphNode.test_coverage` has no production populator.** Every
   extractor (`rust_extractor.rs:162`, `python_extractor.rs:138`,
   `typescript_extractor.rs:131`, `go_extractor.rs:137`,
   `graph_extraction.rs:1625/2050/2175`, `otlp_receiver.rs:884`) sets
   `None`; nothing computes per-node coverage. The lens correctly consumes
   the specced data source and correctly renders no badge when the field is
   NULL (realized-model.md §8: "test_coverage REAL — 0.0-1.0, NULL if
   unavailable"; tested "no badge when coverage data is unavailable"), so
   this is a pre-existing extraction-pipeline gap in the base, not a
   fakery in this task — but it means the green/yellow/red coverage badge
   is currently data-starved in production. Recommend a follow-up task in
   the realized-model extraction domain (e.g. derive per-node coverage from
   the existing test-reachability BFS that `graph_summary`/
   `test-reachability.js` already compute).
2. **`approvalStatusForRecord` (lens-rendering.js) classifies a durable
   Pending ledger row as 'rejected' (✗)** — `active=false`, no
   `revoked_at`, no `rejected_at` falls through to the rejected branch,
   while the server response's derived `status` string would say "Pending".
   Unreachable in practice (production writers create+approve or
   create+reject within one handler; `reject_spec` only transitions
   existing Pending rows), so impact is nil today, but the frontend should
   prefer the server-provided `status` or handle the all-null case as
   pending.
