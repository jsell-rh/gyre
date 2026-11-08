# Review — task-090 (HSI Explorer Three Lenses)

Spec: `human-system-interface.md` §3 "Three Lenses" (cross-referenced with
`explorer-canvas.md` §3/§4). Candidate `74f0875c` (base `7e693c20`); product
commits `0766a167` (three-lenses evaluative overlay), `7bb23bca` (gates.rs
rustfmt/clippy fix), `f5157456` (repo-scoped gate-status MR query, closing the
prior round's finding). Task frontmatter lists all three.

## Round (this review) — approved, no findings

### Scope of the diff (base → candidate, product surface)

`web/src/lib/lens-rendering.js` (new, pure decoration pipeline), `ExplorerCanvas.svelte`
(composed evaluative node decorations + gate edge coloring + legend/tooltip rows),
`ExplorerView.svelte` (evaluative data loading + `?lens=` URL persistence),
`gyre-server/src/api/gates.rs` (`SpecApprovalResponse` rejected_at/rejected_reason
serialization + round-trip test), three new/extended web test files, coverage-matrix
row for HSI §23. The lens selector itself (Structural/Evaluative/Observable with
`aria-disabled` observable + telemetry tooltip) is pre-existing from task-065 and
unchanged — verified present at the base.

### Independent evidence (logs under /tmp/stage/review-evidence)

- `npx vitest run` lens-rendering + ExplorerViewLensUrl: 33/33 pass (exit 0).
- ExplorerCanvas suite: 157/157 pass (exit 0) — includes the 8 new lens draw-path
  tests asserting on the mocked canvas 2D context (fillText text+fillStyle for
  coverage badge / approval icons; captured strokeStyle assignments for gate
  ring/edge colors, with a gate-less baseline differential and a green-count
  differential for the pass case; structural-lens negative tests confirm the
  overlays are evaluative-only; lens-switch test asserts identical label
  coordinates → no re-layout).
- ExplorerViewLargeGraph + ExplorerViewShortcuts: 8/8 pass (exit 0).
- Full `npm test` in `web/` (acceptance criterion): 75 files, 1799 passed,
  41 skipped, exit 0.
- `cargo test -p gyre-server --lib api::gates`: 7/7 pass, including
  `list_spec_approvals_includes_rejection_fields` (server round-trip of the new
  rejection fields).

### Prior-round finding verification (repo-scope, round 16 → f5157456)

The round-16 finding — `loadEvaluativeData` queried `api.mergeRequests({ repo_id,
per_page })` while `GET /api/v1/merge-requests` (`ListMrsQuery`,
crates/gyre-server/src/api/merge_requests.rs) only honors `repository_id` /
`workspace_id` / `status`, so the unscoped arm returned all tenant MRs and the
path-keyed gate-status join attributed other repos' gate outcomes to the viewed
repo — is genuinely closed:

- `ExplorerView.svelte` now queries `{ repository_id: repoId }` and applies the
  50-MR recency window client-side (`.slice(0, 50)` after most-recent-first sort;
  the endpoint has no `per_page`).
- **Differential probe**: temporarily reverting `repository_id` → `repo_id` at
  ExplorerView.svelte:1043 makes the regression test
  ("loads evaluative gate data repo-scoped") fail (1 failed | 5 skipped, exit 1);
  with the fix it passes. Source restored; tree clean at candidate HEAD.
- The fix is attributed in the task frontmatter (`f5157456`), per
  check-task-commit-attribution requirements.

### Contract checks against the acceptance criteria

- Lens selector with all three lenses in the toolbar; Structural default;
  Observable disabled with "requires production telemetry integration" tooltip
  (component-tested).
- Evaluative overlays: coverage badge (green ≥70% / yellow ≥30% / red, % label,
  from `GraphNode.test_coverage`), gate status node ring + edge colors
  (green/red from the most recent concluded gate run per gate, joined node →
  governing spec paths (`spec_path`, `spec_paths`, live `GovernedBy` edge targets)
  → MR `spec_ref` "path@sha" → per-gate latest result; a failing gate on any
  governing spec fails the node), approval icons ✓/⏳/✗ from the ledger
  (active → approved; inactive+rejected → ✗; revoked → pending again — now
  distinguishable server-side via the new `rejected_at`/`rejected_reason` fields;
  spec-linked with no record → pending). All drawn on top of the unchanged
  structural fill/border/label — composition verified by negative
  structural-lens tests and the no-re-layout position test.
- URL persistence: `?lens=evaluative` written via `history.replaceState` on
  switch (Back not polluted), dropped for the structural default, restored from
  deep links on mount, other params + `#drill=` hash preserved — 6 component
  tests plus 27 pure-function unit tests.
- `npm test` in `web/` passes (see evidence).
- Observable lens: disabled placeholder only, per spec "future" — nothing
  fabricated.

### Non-blocking observations (recorded for future tasks, not defects of this candidate)

1. **`GraphNode.test_coverage` has no production populator.** All extractors
   (rust/go/python/typescript, call_graph_resolve, graph_extraction) set
   `None`; `graph_extraction.rs` documents coverage among "metrics not extracted
   by the RustExtractor". The field is persisted and round-tripped through
   storage and the graph API, and the badge rendering is fully data-driven, so
   the overlay code is real — but a production deployment currently has no
   ingestion path that puts a number in the column, so coverage badges only
   appear once such a pipeline exists. This is a pre-existing, platform-wide
   condition (the structural lens's risk metrics, view queries, and heat maps
   consume the same dead field), and the task plan explicitly designates
   `GraphNode.test_coverage` as the data source, so it is out of this frontend
   task's scope. A coverage-ingestion task should be decomposed separately
   rather than faked here.
2. The identical `repo_id` misuse in `loadTraceData` (ExplorerView.svelte:1001)
   is pre-existing from task-065 (#627), flagged by the prior round as outside
   this candidate, and remains untouched — it still needs its own attributed fix.
3. The candidate does not regenerate `web/dist` (other tasks' "regenerate
   verified assets" commits appear to be produced at the verification stage).
   Verifiers probing the served UI must rebuild web from source; the committed
   bundle predates this task's UI changes.

### Environment notes

`scripts/check-assertionless-tests.sh` / `check-comparative-test-claims-js.sh`
abort with exit 2 in this sandbox because it ships mawk (no gawk 3-arg
`match()`); re-run them under gawk in verification. No server transport probe
was needed this round: the round-16 server probe already established the
merge-requests param semantics, and this round's differential component test
plus handler/query-shape inspection independently establish the fix.
