# Review — task-090 (HSI Explorer Three Lenses)

Spec: `human-system-interface.md` §3 "Three Lenses" (Structural / Evaluative / Observable).
Candidate under review: `d50420d5a` (base `2100127c4`, the task-086 liveness base).
Task frontmatter commits: `818eb1c9` (conflict-resolution repair), `f5157456` (repo-scoped MR query), `7bb23bca` (gates.rs lint fix), `0766a167` (implementation).
Verdict: **needs-revision** (2 findings; production code itself is real — both findings are a broken mechanical CI invariant and a missing killing test, not fakery).

## Round N (this review)

### Test runs (evidence: /tmp/stage/review-evidence/)

Environment: no crates.io access and no vendored registry → Rust compilation
impossible (infrastructure restriction, not a code defect). Frontend probed after
`npm ci` in `web/`:

- `npx vitest run lens-rendering.test.js ExplorerViewLensUrl.test.js` → 33 passed.
- `npx vitest run ExplorerCanvas.test.js` → 157 passed.
- `npx vitest run ExplorerCanvasLiveness.test.js ExplorerViewLivenessWiring.test.js liveness.test.js` → 46 passed.
- Full acceptance criterion `npm test` in `web/` → 78 files, 1845 passed / 41 skipped (exit 0).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`,
  `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh` → all OK.
- `bash scripts/check-task-commit-attribution.sh` → **FAIL** (see F1).
- Deferred to verifier (transport restriction): `cargo test -p gyre-server --lib api::gates`.

### Verified working (no findings)

- **Lens selector** (`ExplorerCanvas.svelte:5729-5734`): Structural (default) /
  Evaluative buttons plus Observable `aria-disabled="true"` with the
  "requires production telemetry integration" tooltip — pinned by the LensUrl
  observable test.
- **Evaluative node decorations** (`drawLeafNode`): coverage badge
  (green ≥70 / yellow ≥30 / red, from `GraphNode.test_coverage`), gate status ring,
  approval icon (✓/⏳/✗) drawn on top of the unchanged structural fill/spec
  border/label — lenses compose. Mutation probe (decoration block forced off)
  kills 3 tests: the badge/ring/icon tests are real, and the structural-lens
  differential tests prove the overlay is evaluative-only.
- **Decoration pipeline** (`lens-rendering.js`): pure functions; 27 unit tests cover
  coverage thresholds, node→spec join (`spec_path`, `spec_paths`, GovernedBy
  targets, soft-deleted edges ignored), fail-wins gate resolution, approval status
  from ledger history (active→approved, revoked→pending, rejected→✗,
  no-record→pending), composed decorations, URL param read/write.
- **URL persistence**: `?lens=evaluative` set on switch, dropped for the structural
  default, restored from deep links; `replaceState` (no Back pollution); other
  params + `#drill=` hash preserved. Mutation probe (effect disabled) kills 2 tests.
- **Data wiring** (`ExplorerView loadEvaluativeData`): `mergeRequests({
  repository_id, per_page: 10 })` — both params verified against
  `ListMrsQuery` (`merge_requests.rs:40-49`, newest-first server-side sort);
  per-MR `mrGates` fan-out bounded ≤10; latest-run-per-gate decides; spec approval
  ledger via `GET /api/v1/specs/approvals` (bare array). Mutation probe
  (loader call dropped) kills the repo-scoping test, which also asserts every
  `mergeRequests` call is `repository_id`-scoped with no `repo_id`.
- **No re-layout on lens switch**: `onLensChange` only syncs canvas state and
  schedules a redraw; the label-position identity test plus legend change confirms
  the switch changes encodings, not layout.
- **Ledger API** (`gates.rs`): `SpecApprovalResponse` now serializes
  `rejected_at`/`rejected_reason` (skip-if-none); `From` maps them from the
  pre-existing domain fields (`spec_approval.rs:27-28`, `reject()` at `:62`).
  Statically verified sound (constructor arities match the server test); compile
  deferred to the verifier.
- **Conflict resolution with the task-086 base**: no leftover conflict markers;
  both feature sets coexist (record-aggregate gate edges + per-spec override,
  legend de-duplicated when the per-spec join has data); the task-086 liveness
  suites pass unchanged on the merged candidate.

### Findings

- **F1 (process, CI-red): `check-task-commit-attribution.sh` fails on the candidate.**
  Product-surface commit `818eb1c9` ("fix(task-090): resolve rebase conflicts with
  task-086 liveness code") mentions `task-086` in its subject; the mechanical check
  requires every task-NNN referenced in a task-labeled product-surface commit's
  subject to list that commit in that task's `commits:` frontmatter. `818eb1c9` is
  recorded in task-090's frontmatter only. The check passes at the base
  (`2100127c4`, verified in a temporary worktree, removed after) — the failure is
  introduced by this branch. AGENTS.md freezes the exemption file, so the fix is
  to amend the commit subject (drop the bare "task-086" mention, e.g. "…with the
  liveness-indicators base") or to record the SHA in task-086's frontmatter.
  Repro: `bash scripts/check-task-commit-attribution.sh` at `d50420d5a` → exit 1.
- **F2 (test): the per-spec gate edge coloring has no killing test.** The
  task-090-specific edge behavior (drawEdges `gateStatusByNodeId` override —
  "most recent gate results" joined via node → governing spec → MR `spec_ref`) can
  be removed with every suite still green: mutation probe `if (false && lens ===
  'evaluative')` in that block → 198/198 passed across ExplorerCanvas,
  ExplorerCanvasLiveness, lens-rendering (evidence:
  `/tmp/stage/review-evidence/mutation2-edge-coloring-disabled.txt`). The test
  nominally covering it ("evaluative lens colors edges and node ring red for a
  failed gate, green for a passed gate") is satisfied by the node ring's stroke
  colors alone — ring and edges both stroke `#ef4444`/`#22c55e`, and the
  assertions are stroke-color membership/counts; the liveness suite
  differentially covers only the task-086 record-aggregate path (`gateRecords`
  prop). The shipped notes' "differential red/green stroke assertions" claim is
  inaccurate for the edge component. Fix: assert edge strokes in isolation (e.g.
  a fixture whose governed source nodes draw no ring, or an edge-specific
  stroke-count delta), so removing the edge-coloring code fails a test.

### Informational (non-blocking)

- `GraphNode.test_coverage` is persisted/read end-to-end (SQLite adapter, API
  serialization) but no production writer populates it (all extractors set
  `None`; only tests assign values). The coverage badge degrades honestly
  (renders nothing when the field is null), the field is the data source the
  task plan designates, and the same field backs the pre-existing risk heat map,
  `$where test_coverage` view queries, and the WS context — a pre-existing,
  product-wide data-availability gap, not introduced by this candidate. Noting it
  because the coverage-matrix row's "REAL" claim rests on rendering + storage
  plumbing, not on extractor population.
- Rust probe deferred (no crates.io access in the review sandbox):
  `cargo test -p gyre-server --lib api::gates` must be run by the verifier. Static
  verification found the change sound (fields pre-exist in the domain; `From` maps
  them; test constructor arities match).
