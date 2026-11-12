# Review: TASK-182 — Concept Views (cross-cutting architecture views by concept)

**Reviewer:** Review agent (independent)
**Date:** 2026-11-12
**Comparison base:** e402e1ed620c0407906528548621734d776d43a6
**Candidate:** 299d622d8e9ebae0d86d5813b72b4f642d5243c1
**Verdict:** approved (no blocking findings)

---

## Scope

system-explorer.md §4 Concept Views: concept browser (manifest), canvas
fade emphasis, ad-hoc concept creation, LLM concept exploration, both
data-source mechanisms (manifest projection vs view-spec `concept`
substring).

## Findings

None blocking. Non-blocking observations at the end.

## Behavior verification

- **Concept browser (manifest mechanism).** New endpoints
  `GET /api/v1/repos/:id/graph/concepts` and
  `GET /api/v1/workspaces/:id/graph/concepts` (graph.rs
  `list_graph_concepts` / `list_workspace_graph_concepts`) list the
  repo's (or workspace union of, case-insensitively deduped) manifest
  concepts via the pre-existing `spec_registry::read_git_file` /
  `parse_manifest` path — real manifest reads at the repo's default
  branch, not hardcoded lists. Both routes are registered in
  `api/mod.rs` AND in the ABAC resolver (`abac_middleware.rs`,
  `"graph"` resource) — `check-abac-route-registry.sh` passes; the
  handlers mirror the trust model of the pre-existing single-concept
  endpoints exactly (repo lookup + resolver ABAC for repo scope;
  `require_workspace` + resolver ABAC for workspace scope). The
  ExplorerFilterPanel's Concepts section shows "Predefined" (manifest)
  alongside "Discovered" (substring) subsections.
- **Canvas fade, not removal.** `filter-categories.js`
  `hiddenNodeIds` no longer hides non-concept nodes (concepts are
  deliberately decoupled from visibility — pinned by an explicit
  regression test). `ExplorerCanvas` applies `CONCEPT_FADE_ALPHA
  0.15` to every non-concept node/edge/bundle, full-opacity +
  accent border (`#a78bfa`) to concept nodes and containers holding
  them, and accent/thickness/alpha-floor emphasis to edges between
  concept nodes. Concept nodes bypass semantic-zoom hiding. All
  asserted against actual canvas draw output (label `globalAlpha`
  ratios ≈ 0.15 / 1.0, stroke color + alpha at `stroke()` calls) —
  not against mirrored logic.
- **Active concept chip.** Manifest and discovered concepts render as
  clearable `concept-filter-chip`s in the control bar; clearing
  restores full-opacity draw (alpha-ratio 1.0 asserted). The panel's
  "Clear concepts" also clears the manifest concept via
  `onToggleManifestConcept`.
- **Ad-hoc concepts.** Shift+Click multi-select (pre-existing) +
  right-click → "Create concept" (context menu item, gated on ≥2
  selected) opens a naming dialog; saving persists a ViewQuery
  concept-scope document (`seed_nodes` = selected nodes' qualified
  names, `expand_depth: 0`) via `POST /workspaces/:id/explorer-views`
  (frontend `saveExplorerView` assert pins the exact spec shape;
  name-required and server-error paths tested). The server's
  `parse_and_validate` accepts both grammars by disjoint top-level
  key classification (pre-existing, with its own hybrid-payload
  rejection test); every field of the persisted document is
  Option/defaulted in `gyre-common::view_query::ViewQuery` and passes
  `validate()` (concept scope: non-empty seeds, depth 0 ≤ MAX, empty
  edge list, "both" direction, default `seed_match` "exact") — the
  ad-hoc save is not rejected server-side.
- **Saved-concept round trip.** `viewDocumentToQuery` classifies the
  stored document by grammar; a ViewQuery-shaped spec (the exact
  Create-concept document) is deep-copied with its concept scope
  intact instead of being flattened to `{scope:{type:'all'}}` by the
  ViewSpec translator. Reload round trip tested end-to-end through
  the view selector with fade + annotation assertions.
- **Substring mechanism (data-source distinction).** New
  `seed_match` field on the ViewQuery concept scope: `"exact"`
  (default) vs `"substring"` (case-insensitive substring on
  name/qualified_name, seeding ALL matches). Validated in
  gyre-common, implemented consistently in all three evaluation
  surfaces (gyre-domain `view_query_resolver`, `view-query-scope.js`,
  ExplorerCanvas `queryMatchedWithDepth`). `viewSpecToViewQuery`
  translates ViewSpec `data.concept` to `seed_match: 'substring'`
  (and pins `depth: 0` as matched-nodes-only instead of falling
  through to the default expansion depth). The resolver warns per
  unmatched substring seed.
- **LLM concept exploration.** `PROMPT_EXPLORER_GENERATE`
  (llm_defaults.rs) routes concept questions ("show me everything
  related to X") to `data.concept` with the substring semantics
  stated; `specs/prompts/explorer-generate.md` gains a Concept
  Questions section with a worked JSON example. The end-to-end Ask
  flow is tested through the real generate SSE → ViewSpec →
  translation → canvas fade chain with a graph where NO node is
  named "auth" (exact matching provably resolves nothing there).

## Mutation evidence

- **Mutation probe (review):** commenting out `seed_match:
  'substring'` in `viewSpecToViewQuery` makes
  `ExplorerConceptViews.test.js` "Ask input handles concept questions"
  FAIL (timeout waiting for the fade) — the substring mechanism is
  genuinely pinned, not self-confirmed. Mutation reverted;
  `generated-view.test.js` re-run green (22 passed);
  working tree verified clean afterward. Evidence:
  `/tmp/stage/review-evidence/mutation-substring-seed.log`.

## Focused test runs (evidence in /tmp/stage/review-evidence/)

- `cd web && npm ci && npx vitest run src/__tests__/ExplorerConceptViews.test.js`
  — 8 passed (browser: listing, projection fade + chip + clear,
  multi-select → Create concept → save, Ask concept question,
  saved-concept reload, dialog validation/errors; canvas: edge
  emphasis + no-leak control).
- `npx vitest run filter-categories generated-view view-query-scope
  ExplorerViewNavigableArchitecture` — 66 passed (modified tests
  pin the new fade-not-remove behavior with real draw-alpha
  assertions; not weakened).
- `cargo test -p gyre-common` — 124 passed.
- `cargo test -p gyre-domain view_query_resolver` — 145 passed;
  `cargo test -p gyre-domain substring` — 5 passed (incl. the 3 new
  seed_match tests).
- `cargo check -p gyre-server` — clean (7m41s cold).
- `rustfmt --check` on all changed Rust files — clean (round-1
  verification finding repaired).
- Invariant gates: check-arch, check-abac-route-registry,
  check-inert-enforcement, check-scope-literal-defaults,
  check-unbounded-external-http, check-task-commit-attribution —
  all pass.

## Server tests for the independent verifier

- `cargo test -p gyre-server graph` — includes
  `list_graph_concepts_returns_manifest_concepts`,
  `list_graph_concepts_repo_without_manifest_returns_empty_list`,
  `list_workspace_graph_concepts_unions_and_dedupes`.
- Full `cargo test -p gyre-domain`, `cargo test -p gyre-common`,
  workspace suites, all-target Clippy, full web vitest, GitHub CI.

## Commit attribution

The three task-labeled product commits (39ba4fb69, 362807160,
188b9f948) are recorded in the candidate's task-182 frontmatter
`commits:`; `check-task-commit-attribution.sh` passes (remaining
task-182-labeled commits are docs/process rounds, out of the check's
product-surface scope).

## Non-blocking observations

1. **Workspace-mode concept browser UI.** The workspace-scoped
   listing endpoint is implemented and server-tested, but the filter
   panel only fetches manifest concepts when a repo is selected
   (repo mode). In workspace boundary mode the Concepts section shows
   only Discovered concepts. The acceptance criteria are satisfied
   in repo mode (the primary canvas use case), and the API
   integration item is satisfied server-side; noted as a follow-up
   surface, not a defect against §4.
2. **Generated web/dist.** `cargo check -p gyre-server` triggers
   build.rs, which rebuilds `web/dist` even under `check` — my probe
   regenerated it; the tree was restored to the committed dist
   afterward (`git status` clean except the executor's task-doc
   contract restore, which predates this review). dist is
   verifier-owned; no review signal taken from it.
