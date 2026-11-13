# Review: TASK-182 — Concept Views (system-explorer.md §4)

**Reviewer:** Independent review agent
**Date:** 2026-10-13
**Comparison base:** 890a70dec844985f778b19f81dadd29a94d19de0
**Candidate:** ef4ff9522c72d9923b943f816eb4e45b5cb3b485
**Verdict:** complete (approved)

---

## Scope

Diff base→candidate: 27 files, +2303/−204. Task-labeled commits match the
`commits:` frontmatter exactly (10/10 product-surface commits listed; the
remaining commits in range are merges, process, and docs bookkeeping).

## Findings

No blocking defects. All §4 acceptance criteria are implemented with real
production code, and the fade behavior is mutation-verified.

### Acceptance criteria verification

- [x] **Concept browser lists predefined concepts from manifest.**
  `GET /repos/:id/graph/concepts` (graph.rs `list_graph_concepts`) reads the
  repo's `specs/manifest.yaml` via the same `manifest_concepts` helper the
  single-concept projection endpoint uses (dedupe of the previously inlined
  read — behavior-preserving refactor). Workspace twin
  `list_workspace_graph_concepts` unions across repos with a deterministic
  case-insensitive dedupe (BTreeMap + lexicographic winner — independent of
  the mem adapter's HashMap iteration order; this was the round-2 CI fix,
  commit 13b99dd7). Both routes are registered in the ABAC resolver
  (`abac_middleware.rs`), so middleware policy evaluation runs —
  `check-abac-route-registry.sh` passes with no new exemption entries.
  `ExplorerFilterPanel.svelte` renders Predefined (manifest) and Discovered
  (substring) sections — both mechanisms per the spec's data-source
  distinction.
- [x] **Selecting a concept filters canvas to show related nodes.**
  `toggleManifestConcept` fetches the `GET /repos/:id/graph/concept/:name`
  projection into a node-id set; `explorerConceptNodeIds` unions it with
  discovered-concept matches; the canvas draw loop fades everything outside
  the set. Concept nodes also bypass the semantic-zoom node-type gates
  (`isConceptNode`), so the concept stays visible at overview zoom.
- [x] **Non-related nodes fade to background (not removed).** The fade is
  `drawOp *= CONCEPT_FADE_ALPHA` (0.15) in the draw loop — the node is still
  drawn. `hiddenNodeIds` (filter-categories.js) was deliberately decoupled
  from concept selection, and `treeGroupFilterHidden` no longer hides
  containers by concept. **Mutation-verified:** setting `CONCEPT_FADE_ALPHA
  = 1.0` in ExplorerCanvas.svelte makes
  `selecting a predefined concept fetches the manifest projection and fades
  non-concept nodes` FAIL (ratio 0.15 assertion) — the test pins real draw
  output, not mirrored logic. Source restored; `git status` clean.
- [x] **Edges between concept nodes highlighted.** Both the per-edge draw
  path and the group-bundle path recolor concept-internal edges to the
  concept accent (#a78bfa, distinct from every legend edge color) at boosted
  alpha/thickness, and dim all other edges ×0.15. Pinned by
  `ExplorerCanvas — concept view edge emphasis` draw-path tests.
- [x] **Active concept shown as filter chip in control bar.** Clearable
  `concept-filter-chip` for both manifest and discovered concepts; clear
  restores baseline opacity (asserted in the fade test).
- [x] **Multi-select + right-click → "Create concept" works.** Shift+Click
  multi-select (task-178 substrate) + context-menu item (visible at
  ≥2 selected) → naming dialog; `selectionNodeIds` resolves tree-group
  selections to real graph node ids (synthetic `__tree__` ids excluded).
  Verified end-to-end in the vitest suite via real canvas hit-testing
  (mousedown/mouseup/click at label positions).
- [x] **Ad-hoc concepts saved as explorer views.** `saveConceptDialog`
  persists a ViewQuery-grammar concept scope (exact seeds, expand_depth 0)
  via `POST /workspaces/:id/explorer-views`. Server-side acceptance
  confirmed: `parse_and_validate` classifies by disjoint top-level field
  sets (pre-existing dual-grammar support in explorer_views.rs), and
  `ViewQuery::validate` accepts the exact document the dialog builds
  (seed_match defaults "exact" via serde). The reload path
  (`viewDocumentToQuery`) passes ViewQuery documents through with the
  concept scope intact instead of flattening to `{type:'all'}` — pinned by
  both unit tests and the saved-view round-trip draw test.
- [x] **Ask input handles concept questions via LLM.** The
  explorer-generate prompt template (llm_defaults.rs +
  specs/prompts/explorer-generate.md) routes "show me everything related to
  X" questions to `data.concept`. The client translation
  (`viewSpecToViewQuery`) now marks the concept scope
  `seed_match:'substring'` (the ui-layout.md §4 Data Layer mechanism) and
  honors `depth: 0` as matched-nodes-only. Implemented consistently in all
  three resolvers (server `view_query_resolver.rs`, `view-query-scope.js`,
  ExplorerCanvas's internal scope matcher) with `seed_match` validated
  server-side (`Invalid seed_match` in `ViewQuery::validate`).
- [x] **Tests pass.** See probes below.

### treeGroupHasAny fix + e2e baseline regeneration (final CI round)

The old `treeGroupHasMatch` compared `cln.parentTreeGroup === ln` by object
reference across Svelte 5 `$state` proxies — never equal, so every container
dimmed during a view query even when holding matched nodes. The fix compares
by id (`cln.parentTreeGroup?.id === lnId`). The four regenerated filter-preset
baselines (filter-endpoints/types/calls/dependencies) are the rendering
consequence of this real bug fix, not baseline laundering: the behavioral
canvas-pixel change-count assertions in `filter_preset_buttons_show_correct_
subsets` (MIN_VISIBLE_CANVAS_CHANGES on zoom, preset click, and preset
transition) are untouched, and PRESET_SNAPSHOT_OPTIONS is unchanged
(maxDiffPixelRatio 0.02, threshold 0.05). Baseline file sizes grew
(~+15-20%), consistent with more lit content, not less.

## Probes (all under /tmp/stage/review-evidence/)

| Probe | Result |
|---|---|
| `npm ci` (web, locked versions) | ok |
| `npx vitest run` ExplorerConceptViews + filter-categories + generated-view + view-query-scope + ExplorerViewNavigableArchitecture | 74/74 pass, exit 0 |
| `npx vitest run` ExplorerCanvas* (7 files) + canvas-filters + ExplorerDefaultViews + ExplorerGeneratedViews + GeneratedViewChat | 268/268 pass, exit 0 |
| `npx vitest run` ExplorerView scope/drill/large/lens/shortcuts/workspace-graph + MoldableViews + RiskMap | 83/83 pass, exit 0 |
| `cargo test -p gyre-domain concept` | 12 pass (incl. 3 new substring-seed tests + invalid seed_match validation), exit 0 |
| `cargo test -p gyre-common viewquery_validate` | compile ok, 0 filtered matches (validation tests live in gyre-domain) |
| `cargo check -p gyre-server` | clean, exit 0 (7m55s) |
| `scripts/check-abac-route-registry.sh` | pass (new routes in resolver, no exemption growth) |
| `scripts/check-arch.sh` | pass |
| `scripts/check-inert-enforcement.sh` | pass |
| Mutation probe: `CONCEPT_FADE_ALPHA = 1.0` → ExplorerConceptViews fade test | FAILS as expected (test kills the disabled behavior); source restored, tree clean |

## Tests the independent verifier must run

- `cargo test --all` (workspace) — includes the new server tests
  `list_graph_concepts_returns_manifest_concepts`,
  `list_graph_concepts_repo_without_manifest_returns_empty_list`,
  `list_workspace_graph_concepts_unions_and_dedupes` (gyre-server graph.rs;
  not compiled in this review sandbox per the 4 GiB budget rule —
  `cargo check -p gyre-server` passed).
- `cd web && npm ci && npm test` (full vitest suite).
- `cd web && npx playwright test tests/e2e/explorer-visual.spec.js` — the
  four regenerated filter-preset baselines must be confirmed against a fresh
  build of the current web source on the verifier's runner class.
- Repo mechanical checks (already green here): check-abac-route-registry,
  check-arch, check-inert-enforcement.

## Observations (non-blocking)

- `specs/coverage/system/system-explorer.md` still shows §4 as
  `task-assigned` — consistent with sibling completed tasks (task-178/181
  rows are also still task-assigned); coverage-matrix flipping is a
  controller/audit stage, not the implementing agent's.
- `web/dist/` was restored to HEAD after probes (a vitest run regenerates
  it); generated assets are the deterministic verifier's ownership.
- The api-reference.md rewrite of `GET /repos/{id}/graph/concept/{name}`
  (substring → manifest projection) documents the endpoint's actual
  pre-existing behavior (`resolve_concept_view` was already manifest-based
  at base); no behavior change was smuggled in — only the docs were
  corrected.
