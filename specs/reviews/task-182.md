# Review: TASK-182 — Concept Views (system-explorer.md §4)

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-10-10
**Comparison base:** 3b734b1af744f594ec5d903be242b41ec0f287f2
**Candidate:** 4d0af41d04f634a6b0396f5dc86ed7b013886f3a
**Verdict:** approved — all §4 acceptance criteria are enforced by production code; focused tests mutation-verified.

---

## Scope

Diff base→candidate: 23 files, +2298/−203. Product surface: two new concept-browser
endpoints (`GET /repos/:id/graph/concepts`, `GET /workspaces/:id/graph/concepts`),
`seed_match` concept-scope grammar extension (gyre-common validate + gyre-domain
resolver), ExplorerView/ExplorerCanvas/ExplorerFilterPanel concept-view UI, ad-hoc
concept saving as explorer views, ViewSpec `data.concept` substring translation,
LLM prompt concept guidance, and ExplorerConceptViews.test.js (8 tests). This
round also merges main (task-168) into the branch; source merged cleanly, only
generated `web/dist` conflicted (resolved to the candidate's verified build —
generated output, verifier-owned, non-blocking).

## Acceptance criteria verification

- [x] **Concept browser lists predefined concepts from manifest.**
  `list_graph_concepts` / `list_workspace_graph_concepts` (graph.rs) read the
  repo's `specs/manifest.yaml` `concepts:` block via the same
  `read_git_file`/`parse_manifest` path the single-concept projection endpoint
  uses; workspace listing unions across repos with case-insensitive dedupe.
  Both routes registered in `api/mod.rs` AND the ABAC resolver
  (`check-abac-route-registry.sh` exit 0). The filter panel renders a
  "Predefined" section fed by `api.repoGraphConcepts`; verified end-to-end by
  `concept browser lists predefined concepts from the manifest` (passed).
- [x] **Selecting a concept filters canvas to show related nodes.**
  `toggleManifestConcept` fetches the `GET /repos/:id/graph/concept/:name`
  projection (manifest mechanism) and feeds `explorerConceptNodeIds`; the
  canvas draw loop emphasizes concept nodes (full opacity + accent border,
  visible regardless of semantic zoom) — pinned by the fade test.
- [x] **Non-related nodes fade to background (not removed).** Canvas fade
  `drawOp *= 0.15` for non-concept nodes/edges/bundles, never skipped;
  `filter-categories.hiddenNodeIds` deliberately no longer hides by concept
  (regression-pinned by an explicit test). Mutation probe: disabling the fade
  line fails the manifest-concept test (1 failed).
- [x] **Edges between concept nodes highlighted.** Accent color `#a78bfa`,
  alpha floor 0.8, thicker stroke for concept–concept edges and bundles;
  non-concept edges dim to 0.15 with their endpoints — asserted through
  captured canvas stroke output (draw-path describe block, both tests pass).
- [x] **Active concept shown as filter chip in control bar.**
  `concept-filter-chip` with clear button; test asserts appearance, fade
  behavior, and restore-on-clear.
- [x] **Multi-select + right-click → "Create concept" works.** Shift+click
  multi-select (pre-existing), new `create_concept` context-menu action gated
  on ≥2 selected, `selectionNodeIds` resolves tree-group selections to real
  node ids. Test drives real canvas mousedown/mouseup/click/contextmenu
  events at label hit positions.
- [x] **Ad-hoc concepts saved as explorer views.** Dialog persists a
  ViewQuery-grammar concept scope (`expand_depth: 0`, exact seeds) via
  `POST /workspaces/:id/explorer-views`; server `parse_and_validate`
  classifies it as ViewQuery and validates it (verified in
  explorer_views.rs). Reload round trip via `viewDocumentToQuery` (grammar
  classification by top-level `scope.type`) applies the grouping instead of
  flattening to `{scope:{type:'all'}}` — pinned by the saved-view round-trip
  test.
- [x] **Ask input handles concept questions via LLM.** `explorer-generate`
  prompt template + `llm_defaults.rs` default prompt route concept questions
  to `data.concept`; `viewSpecToViewQuery` translates `data.concept` to a
  concept scope with `seed_match: 'substring'` (ui-layout.md §4 data-source
  distinction), implemented consistently in all three evaluation surfaces
  (gyre-domain `view_query_resolver`, `view-query-scope.js`,
  ExplorerCanvas). End-to-end Ask test (SSE-mocked LLM → fade asserted via
  draw alphas) passes; mutation probe removing `seed_match: 'substring'`
  fails 4 tests across 2 suites.
- [x] **Tests pass.** See evidence below.

## Probes run (evidence in /tmp/stage/review-evidence)

- `npx vitest run src/__tests__/ExplorerConceptViews.test.js` — 8/8 pass
  (after `npm ci`).
- `npx vitest run` filter-categories, view-query-scope, generated-view,
  ExplorerViewNavigableArchitecture — 66/66 pass.
- Mutation probes (restored; `git status --porcelain` clean after each):
  fade-line disabled → 1 test fails; substring seed_match removed → 4 tests
  fail. The tests kill real regressions, not self-confirming.
- `cargo test -p gyre-common` — 124 pass (incl. `seed_match` validation).
- `cargo test -p gyre-domain view_query_resolver` — 145 pass (incl. 3 new
  substring-seed tests + invalid-seed_match validation test).
- `cargo check -p gyre-server` — clean (8m20s cold).
- `cargo fmt -p gyre-server -p gyre-domain -p gyre-common -- --check` — clean.
- `scripts/check-abac-route-registry.sh`, `scripts/check-arch.sh` — exit 0.
- `scripts/check-task-commit-attribution.sh` against HEAD — exit 0 (the
  working-tree failure is the review guard's task-file reset, not the
  candidate; at HEAD all four task-labeled commits are listed).

## Verifier-owned (recorded for the independent verifier)

- `cargo test -p gyre-server graph` — includes the three new
  `list_graph_concepts*` tests (manifest listing, no-manifest empty list,
  workspace union/dedupe).
- Full workspace suite, all-target Clippy, full web vitest + e2e, and the
  web/dist rebuild/publication.

## Observations (non-blocking)

1. *Generated output:* `web/dist` was resolved to the candidate's verified
   build through the main merge; my `cargo check` briefly regenerated it
   (build.rs) — restored to HEAD; the verifier owns the final rebuild.
2. *Workspace-scope UI:* the filter panel fetches manifest concepts only at
   repo scope (`selectedRepoId`); the workspace-union endpoint exists and is
   bound in `api.js` (`workspaceGraphConcepts`) but not yet consumed by the
   panel at workspace scope. Not required by the task's acceptance criteria
   (the plan's workspace endpoint item is API integration, which is done).
3. *Coverage matrix:* `specs/coverage/system/system-explorer.md` §4 row
   remains `task-assigned` — consistent with sibling completed tasks
   (task-180 §9, task-181 §7); matrix flipping is a controller/audit stage,
   not part of this branch's contract.

## Security / scope handling

New routes are registered in the ABAC resolver (middleware policy applies,
registry check green) and follow the exact handler pattern of their pre-existing
siblings (`get_graph_concept` repo lookup; `require_workspace` for the workspace
variant). No exempt-handler surface added, no migrations, no new durable state,
no scope-literal defaults, no relative-path defaults.
