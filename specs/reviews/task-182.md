# Review — task-182 (Concept Views — cross-cutting architecture views by concept)

Spec: `specs/system/system-explorer.md` §4 "Concept Views" (+ ui-layout.md §4 data-source
distinction, view-query-grammar.md `concept` scope).
Candidate under review: `38d018e7` (base `ccfd4bc5`; rebase-repair round for durable
finding `dc8e7bcd` — merge concluded as `87184fca`, parents `14ac124d` + `ccfd4bc5`).
Verdict: **approved** — no blocking findings. One generated-output observation and one
minor note, recorded below as non-blocking.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cd web && npm ci` then `npx vitest run` (full suite): **112 files / 2271 passed,
  41 skipped, exit 0** (`vitest-full.txt`).
- Focused: `ExplorerConceptViews.test.js` + `filter-categories.test.js` +
  `generated-view.test.js` + `view-query-scope.test.js` → 62/62 (`vitest-concept.txt`);
  `ExplorerViewNavigableArchitecture.test.js` → 12/12 (`vitest-navigable.txt`).
- Mutation probe 1 — concept fade disabled in `ExplorerCanvas.svelte` draw loop
  (`drawOp *= 1.0`): the two fade tests FAIL (manifest-concept fade test, discovered-
  concept fade test) — the fade assertions pin real draw-path behavior, not structure
  (`vitest-mutation-fade.txt`). Source restored (verified no diff).
- Mutation probe 2 — `seed_match: 'substring'` removed from `viewSpecToViewQuery`:
  4 tests FAIL including the end-to-end Ask concept path — substring translation is
  load-bearing (`vitest-mutation-seedmatch.txt`). Source restored.
- Mutation probe 3 — Rust resolver `let substring = false;` in
  `view_query_resolver.rs`: 2 of 4 `seed_match` tests FAIL. Source restored.
- `/tmp/stage/bin/cargo test -p gyre-domain --lib view_query_resolver` → **145 passed**
  (includes the 4 new seed_match tests: substring seeds all name matches, expands from
  all seeds, warns on no match, validate rejects invalid seed_match).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean, exit 0 (7m01s).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh` (both new `/graph/concepts`
  routes have RouteResourceMapping entries), `check-byte-slice-truncation.sh`,
  `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-relative-path-defaults.sh` → all exit 0.
- `scripts/check-task-commit-attribution.sh` → **OK against the candidate tree**
  (`attribution-candidate.txt`; the harness had reverted the working-tree task file to
  `commits: []`, which fails the script — an environment artifact, not a candidate
  defect; the candidate commit records all 11 product SHAs).

## Verified against acceptance criteria

- **Concept browser lists predefined concepts from manifest** — new
  `GET /repos/:id/graph/concepts` + `GET /workspaces/:id/graph/concepts` endpoints read
  the repo's `specs/manifest.yaml` `concepts:` block via the same `read_git_file`
  mechanism as the existing projection endpoint; panel shows a "Predefined" section
  distinct from "Discovered". Server tests drive the real router with a real git
  manifest fixture (init_manifest_repo), including no-manifest → empty list and
  workspace union/dedupe (deterministic, order-independent winner).
- **Selecting a concept filters canvas / non-related nodes fade (not removed) /
  edges highlighted / filter chip** — `ExplorerView.svelte` fetches the manifest
  projection (`getGraphConcept`) and unions its node ids with discovered-concept
  substring matches into `explorerConceptNodeIds`; `ExplorerCanvas.svelte` fades
  non-concept nodes to 0.15 draw opacity (drawn, never removed — the hiding branch was
  deliberately removed from `hiddenNodeIds`/`treeGroupFilterHidden`, with tests pinning
  that concepts never hide), highlights concept-node edges (accent color, ≥0.8 alpha,
  bundle path too), gives concept nodes/containers an accent border, bypasses
  semantic-zoom hiding for concept nodes, and shows a clearable control-bar chip.
  All asserted through actual canvas draw output (label/stroke alphas), and the
  fade-disabled mutation probe proves the tests fail when the behavior breaks.
- **Multi-select + right-click → Create concept / saved as explorer views** —
  Shift+Click multi-select (pre-existing) + context-menu item (≥2 selected) opens a
  naming dialog; saving POSTs a ViewQuery-grammar concept scope (exact seeds = selected
  nodes' qualified names, expand_depth 0) to `POST /workspaces/:id/explorer-views`;
  the server's `parse_and_validate` accepts the ViewQuery grammar and `vq.validate()`
  passes (scope/emphasis/zoom/annotation are all ViewQuery fields). Reload round trip
  via `viewDocumentToQuery` (grammar classification, deep copy) is tested end to end.
- **Ask input handles concept questions via LLM** — prompt template updated in both
  `llm_defaults.rs` and `specs/prompts/explorer-generate.md` with concept-question
  guidance and a `data.concept` example; `data.concept` translates to a
  `seed_match: 'substring'` concept scope consistently across `generated-view.js`,
  `view-query-scope.js`, `ExplorerCanvas.svelte`, and the Rust resolver (validated:
  `seed_match ∈ {exact, substring}`, default exact). The end-to-end test feeds an SSE
  `complete` event with `data.concept: 'auth'` over a graph where no node is NAMED
  auth and asserts the substring fade on the canvas.
- **Data-source distinction** — both mechanisms implemented and kept distinct:
  manifest projection endpoints (exact concept-name match, 404 on undefined) vs
  ViewSpec `concept` substring matching (seeds ALL name/qualified_name matches).
  `view-query-grammar.md` amended to document `seed_match`.
- **Rebase repair** — merge `87184fca` brings base `ccfd4bc5` in as second parent;
  the merged tree passes the full frontend suite, focused domain tests, cargo check,
  and all mechanical scripts. The one both-sides source file (`ExplorerView.svelte`)
  correctly carries task-164's `onRefresh={loadWsDepGraph}` prop pass (DependencyGraph
  tests pass in the full run).

## Generated-output observation (non-blocking, for the verifier)

The committed `web/dist/` at candidate HEAD is the upstream/base bundle (the merge
resolved the generated dist conflicts toward main's asset names): it contains no
concept-view code (`graph/concepts`, `concept-dialog`, `concept-filter-chip` all
absent from `index-CWyIRXUQ.js`), while the prior task head `14ac124d`'s verified
build did. Per the review contract this is verifier-owned: the deterministic verifier
rebuilds `web/dist/` from the merged source after integration and binds publication
to that tested tree. Confirming buildability, the freshly built (uncommitted,
background-process) bundle in this sandbox's working tree does contain all three
concept markers. Not a source defect; recorded so the verifier regenerates and
commits the assets.

## Minor note (non-blocking)

- `api.workspaceGraphConcepts` (web client) is currently unconsumed by components —
  the concept browser panel fetches repo-scoped concepts (boundary mode is
  repo-scoped). The workspace endpoint itself is real and server-tested; the spec's
  interaction mode is the manifest browser, which is wired. No action required.

## Server tests for the verifier to execute

- `cargo test -p gyre-server --lib list_graph_concepts` (manifest listing, empty-list
  on no manifest)
- `cargo test -p gyre-server --lib list_workspace_graph_concepts` (union + dedupe)
- Standard full workspace suite + `cd web && npm test` + e2e
  `explorer-visual.spec.js` (filter-preset baselines were legitimately regenerated for
  the matched-container emphasis fix — `treeGroupHasAny` now matches by id, fixing the
  dead object-reference recursion).
