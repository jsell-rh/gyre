# Review — task-090 (HSI Explorer Three Lenses — Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (lines 810-819), with
`specs/system/explorer-canvas.md` §4 "Lens Definitions" for lens detail. Task:
`specs/tasks/task-090.md`.

Assigned base `72ad2a128` → candidate `15e01f35c` (HEAD of the review branch, verified
`git rev-parse HEAD`). Cumulative diff touches 9 files (+1506/−6): the new
`web/src/lib/lens-rendering.js` (pure decoration pipeline), evaluative additions to
`web/src/lib/ExplorerCanvas.svelte` (node decorations, gate edge coloring, legend,
tooltip) and `web/src/components/ExplorerView.svelte` (`loadEvaluativeData`,
`?lens=` URL persistence), `SpecApprovalResponse` rejection fields in
`crates/gyre-server/src/api/gates.rs`, three test files, and the task/coverage docs.
All five `commits:` frontmatter SHAs exist and are ancestors of the candidate.

Verdict: **approved** (round 3; the two prior rounds' findings are closed, see below).

## Prior-round findings closed

1. **Unresolved merge-conflict markers** (round 1, candidate `8adb4b914`): grep for
   `<<<<<<<`/`=======`/`>>>>>>>` in `ExplorerCanvas.svelte` and `ExplorerView.svelte`
   at the candidate finds none; both files compile and every Explorer suite runs
   (evidence below). The seven conflict blocks were resolved in `47fca1998` so the
   task-090 lens code and the task-086 liveness code coexist.
2. **`check-task-commit-attribution.sh` exit 1** (round 2, commit subject referencing
   bare `task-086`): the script now exits 0 at the candidate
   (`r18-attribution-check.txt`: "OK: every task-labeled product-surface commit is
   recorded in its task's commits: frontmatter"). The subject rewrite is
   `47fca1998` ("...with the liveness-indicators base").
3. **Mutation-surviving per-spec gate edge coloring** (round 2): re-running the
   reviewer's exact mutation (`if (false && lens === 'evaluative')` on the drawEdges
   `gateStatusByNodeId` block) now FAILS exactly one test — the new differential
   edge-stroke test `evaluative lens colors the edge leaving a governed node by
   per-spec gate status (edge strokes in isolation)` (1 failed/157 passed in
   ExplorerCanvas.test.js; `r18-mutation1-edge-coloring-disabled.txt`). The test
   captures EDGE strokes only, discriminated by path geometry (drawArrow shafts =
   one `lineTo`, zero `quadraticCurveTo`; rings/borders go through `roundRect`), so
   node-ring colors cannot satisfy it.

## Evidence (all at candidate `15e01f35c`, after `npm ci` in `web/`)

- **Focused suites** (`npx vitest run` on lens-rendering, ExplorerCanvas,
  ExplorerCanvasLiveness, ExplorerViewLensUrl, ExplorerViewLivenessWiring):
  **209/209 passed, exit 0** (`r18-focused-suites.txt`).
- **Full `npm test` in `web/`**: **80 files, 1865 passed / 41 skipped, exit 0**
  (`r18-full-npm-test.txt`) — acceptance criterion met.
- **Rust**: `/tmp/stage/bin/cargo test -p gyre-server --lib api::gates` →
  **7 passed** including the new `list_spec_approvals_includes_rejection_fields`
  (`r18-cargo-gates.txt`).
- **Mechanical**: `check-arch.sh` exit 0; `check-task-commit-attribution.sh` exit 0.
- **Mutation probes** (each applied to a working-tree copy, run, then restored;
  `git status` clean and files byte-identical to the candidate afterwards):
  1. per-spec gate edge coloring disabled → differential edge-stroke test FAILS
     (killed);
  2. coverage badge draw disabled → `evaluative lens draws a color-coded coverage
     badge` FAILS (killed, `r18-mutation2-coverage-badge-disabled.txt`);
  3. approval icon draw disabled → `evaluative lens draws spec approval icons`
     FAILS (killed, `r18-mutation3-approval-icon-disabled.txt`);
  4. `lensToUrl` never writes the param → 4 tests FAIL across lens-rendering +
     ExplorerViewLensUrl, including the deep-link restore (killed,
     `r18-mutation4-url-persistence-broken.txt`);
  5. `normalizeLens` hardcodes structural → 6 tests FAIL including deep-link
     restore and param read (killed, `r18-mutation5-deeplink-restore-broken.txt`).

## Acceptance criteria verified

- **Lens selector** in the ExplorerCanvas toolbar (`.lens-group`): Structural,
  Evaluative, Observable — the last `aria-disabled="true"` with the
  "requires production telemetry integration" title/inline text
  (`ExplorerCanvas.svelte:5730-5734`); asserted by the LensUrl suite.
- **Structural default** from knowledge graph: pre-existing base rendering, plus a
  test that the coverage badge and approval icons are absent in the structural lens
  (overlay is evaluative-only) — composition is additive, not a replacement.
- **Evaluative overlays**: coverage badge (green ≥70% / yellow ≥30% / red below, from
  `GraphNode.test_coverage`, a 0–1 fraction per `gyre-common/src/graph.rs:127` and
  the `test_coverage >= 0.8` assertion predicate), gate status ring + edge colors,
  approval icons ✓/⏳/✗. Each is guarded by a mutation-killing test (probes 1–3).
- **Gate status edge colors**: the per-spec recency join (`gateStatusBySpec` from
  `loadEvaluativeData`: repo-scoped `mergeRequests({repository_id, per_page: 10})`
  — `ListMrsQuery` supports both and sorts newest-first, `merge_requests.rs:760-766`
  — then per-MR `mrGates`, latest run per `gate_id` via `started_at`, fail wins)
  overrides the task-086 record-aggregate coloring for edges leaving a governed
  node; response shapes (`gate_id`, `status` ∈ passed/failed, `started_at`,
  `spec_ref`) match `GateResultResponse`/`MrResponse` serialization.
- **Spec approval**: `SpecApprovalResponse` now serializes `rejected_at`/
  `rejected_reason` (domain fields pre-existed in `gyre-domain/src/spec_approval.rs`),
  distinguishing rejection (✗) from revocation (⏳ pending again); server
  round-trip test asserts both branches.
- **Lenses compose / no re-layout**: the label-position test asserts identical
  `fillText` coordinates for the same node across a structural→evaluative switch
  while the evaluative legend appears (the re-render really happened).
- **URL persistence**: `?lens=evaluative` written via `history.replaceState` on
  switch, dropped for the structural default, restored from deep links on mount;
  other params and the `#drill=` hash preserved (mutations 4–5).
- **`npm test` passes in `web/`**: see above.

## Notes (non-blocking)

- `loadEvaluativeData` is best-effort by design (ledger/MR fetch failures leave the
  structural base rendering); this matches the overlay's role and HSI §3's data
  sources, and the repo-scoping contract (`repository_id`, never `repo_id`, ≤10
  fan-out) is pinned by the LensUrl wiring test.
- `approvalStatusForRecord` treats a record that is both revoked and rejected as
  pending (revocation checked first). The domain API exposes reject/revoke as
  separate transitions and the spec assigns no semantics to the both-set corner;
  the tested single-transition behaviors (active→✓, rejected→✗, revoked→⏳) are
  faithful to the ledger contract.
- The explorer-canvas.md §4 "Evaluative" definition (trace particles, span heat) is
  the task-065 lineage and coexists with the HSI §3 overlays; both render under the
  same lens without conflict (full suite green).
