# Review: TASK-182 — Concept Views (system-explorer.md §4)

**Reviewer:** Independent review agent
**Date:** 2026-11-12
**Comparison base:** 3af80bd21a8a3452420e014e645eefdfd137f9be
**Candidate:** cea6871ef9571ab764fb88601955d6028c795219
**Verdict:** approved

---

## Scope of review

Candidate implements system-explorer.md §4 Concept Views on the task-178 canvas:
manifest concept browser (`GET /repos/:id/graph/concepts`, workspace union
variant), canvas concept fade emphasis (nodes, edges, group bundles, borders),
control-bar concept chips, Shift+Click multi-select + right-click "Create
concept" → naming dialog → explorer-view persistence, LLM concept-question
routing (prompt template + fallback default), and the ViewSpec `data.concept`
substring mechanism with `seed_match` grammar parity in Rust (gyre-common
ViewQuery, gyre-domain resolver) and the web translators/scopes.

This round's candidate includes repair commit 362807160 addressing both round-1
findings.

## Round-1 finding verification (both repaired, mutation-verified)

1. **Substring mechanism (was: exact-match seeds silently matched nothing).**
   `viewSpecToViewQuery` now emits `seed_match: 'substring'` for ViewSpec
   `data.concept`; `view-query-scope.js`, `ExplorerCanvas.svelte`'s inline
   scope evaluator, and the Rust resolver (`view_query_resolver.rs`, validated
   in `view_query.rs`) all implement case-insensitive substring seeding over
   node name/qualified_name, seeding ALL matches. ui-layout.md §4 depth-0
   semantics pinned (`data.depth: 0` → `expand_depth: 0`). Rust grammar doc
   updated (`view-query-grammar.md`).
2. **Saved ad-hoc concept round-trip (was: reload flattened the concept scope
   to the full graph).** New `viewDocumentToQuery` classifies the stored
   document by grammar (top-level `scope.type` → ViewQuery deep copy, else
   ViewSpec translation); `refreshSavedViews` uses it. The server's
   explorer-views `parse_and_validate` (pre-existing) accepts ViewQuery-shaped
   specs with disjoint-key classification, and the exact emphasis shape the
   dialog posts (`highlight.matched.color`, `dim_unmatched`) matches
   `gyre-common` `Emphasis`. Round trip is real: save → store → reload →
   concept scope applied (draw-path assertions).

**Mutation probe:** re-introducing both defects (removing `seed_match` from the
translator; reverting the reload path to `viewSpecToViewQuery`) fails 5 tests
(2 ExplorerConceptViews, 3 generated-view). The tests kill the bugs, not
mirrors. Files restored after the probe.

## Behavior verification

- [x] Concept browser lists predefined concepts from manifest —
  `list_graph_concepts` / `list_workspace_graph_concepts` (real manifest read
  via `spec_registry::read_git_file` + `parse_manifest`, refactor shared with
  `resolve_concept_view`); filter panel "Predefined" vs "Discovered" sections;
  server tests for listing, empty-manifest, and union+case-insensitive dedupe
  (gyre-server — recorded for verifier, see below).
- [x] Selecting a concept filters canvas — manifest projection fetched via
  `GET /repos/:id/graph/concept/:name`; emphasis set unioned with discovered
  substring matches.
- [x] Non-related nodes fade, not removed — draw-path tests assert
  label draw-opacity ratio ≈ 0.15 for non-concept nodes and ≈ 1.0 for concept
  nodes; `hiddenNodeIds` deliberately decoupled from concepts (test pins the
  decoupling).
- [x] Edges between concept nodes highlighted — accent color, alpha floor,
  thickness (direct edges and group bundles); stroke-color/alpha draw-path
  tests, plus a no-concept control test (no emphasis leak).
- [x] Active concept chip in control bar (manifest + discovered), clearable;
  tested including restore-to-full-opacity.
- [x] Multi-select + right-click "Create concept" — canvas context-menu item
  (visible only with ≥2 selected), `selectionNodeIds` resolves tree-group
  selections to real node ids; tested via real canvas hit-testing.
- [x] Ad-hoc concepts saved as explorer views — exact ViewQuery shape
  asserted at the `saveExplorerView` call; error and empty-name paths tested.
- [x] Ask input handles concept questions — prompt template
  (`specs/prompts/explorer-generate.md` Concept Questions section with worked
  example) + `llm_defaults.rs` fallback routing to `data.concept`; end-to-end
  test drives the Ask flow with a mocked SSE ViewSpec
  (`data.concept: 'auth'`, no node named 'auth') and asserts substring fade
  on real draw output.
- [x] Tests pass — full web suite 107 files / 2159 passed (41 skipped);
  gyre-domain 145+3 passed; gyre-common 5 passed.

## Structural / invariants

- New routes `GET /repos/:id/graph/concepts` and `GET /workspaces/:id/graph/concepts`
  registered in `mod.rs` **with** ABAC resolver entries (check-abac-route-registry
  OK); handlers mirror sibling graph handlers (repo lookup / `require_workspace`).
- check-arch, and all 16 mechanical constraint scripts pass.
- `cargo check -p gyre-server` clean.
- Commit attribution recorded in task-182.md frontmatter (both product commits
  39ba4fb and 362807160 listed; process/docs commits excluded appropriately).

## Observations (non-blocking)

- `api.workspaceGraphConcepts` has no UI consumer: the concept browser panel
  fetches manifest concepts only at repo scope. The workspace endpoint itself
  is real and server-tested, and the pre-existing concept search UI is
  repo-scoped too, so this matches the established surface. The acceptance
  criteria do not require a workspace-scope browser panel.
- Coverage matrix row 12 (§4 Concept Views) remains `task-assigned` — consistent
  with the pipeline convention (task-178's row 9 likewise unchanged; matrix
  updates are a separate audit stage), so not counted against this candidate.

## Verifier must run (outside review budget)

- `cargo test -p gyre-server --lib graph::tests::list_graph_concepts_returns_manifest_concepts`
- `cargo test -p gyre-server --lib graph::tests::list_graph_concepts_repo_without_manifest_returns_empty_list`
- `cargo test -p gyre-server --lib graph::tests::list_workspace_graph_concepts_unions_and_dedupes`
- full `cargo test -p gyre-server --lib graph::tests` (pre-existing concept
  projection tests unaffected by the `resolve_concept_view` refactor)
- CI parity: `cargo test --all`, clippy, web build + tests, all check scripts

Probe evidence: `/tmp/stage/review-evidence/probe-commands.log`.
