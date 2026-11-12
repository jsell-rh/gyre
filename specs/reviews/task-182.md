# Review: TASK-182 — Concept Views (system-explorer.md §4)

**Reviewer:** Review agent
**Date:** R2 (post-rebase candidate)
**Comparison base:** 7e9b3cfeea8598e05c8b0ae971e06f214dd21806
**Candidate:** 09e7527640cd51fbdd20d8dcfc7f46123bd6dda5
**Verdict:** approved

---

## Scope

Diff base→candidate (23 files, +2304/−202): manifest concept-browser API
(`GET /repos/:id/graph/concepts`, `GET /workspaces/:id/graph/concepts`),
ViewQuery `seed_match` grammar field (exact/substring) across
gyre-common/domain/server and all three JS concept-scope resolvers, canvas
concept fade emphasis (draw path, never removal), control-bar concept chips,
ad-hoc concept creation dialog persisted via explorer-views CRUD, LLM prompt
concept examples, and the rebase onto the task-176 base (single
ExplorerView.svelte import conflict resolved keeping both sides).

## Findings

No blocking findings. All §4 acceptance criteria are backed by production
code and mutation-verified tests.

## Behavior verification

- [x] **Concept browser lists predefined concepts from manifest.** New
  endpoints read the repo's `specs/manifest.yaml` `concepts:` block at its
  default branch via the existing `spec_registry::read_git_file` /
  `parse_manifest` (shared with the single-concept projection endpoint — no
  drift). Workspace endpoint unions across repos with case-insensitive
  dedupe. Both are in the ABAC resolver with the sibling `graph` resource
  mapping (`check-abac-route-registry.sh` passes); the workspace handler
  additionally calls `require_workspace`, matching siblings. A repo without
  a manifest yields an empty list (valid) — distinct from the projection
  endpoint's 404. Server tests exist for all three cases (verifier must run
  them — see /tmp/stage/review-evidence/README.md).
- [x] **Selecting a concept filters canvas.** Filter panel "Predefined"
  section fetches the listing; clicking fetches the manifest projection
  (`GET /repos/:id/graph/concept/:name`) and unions its node ids with
  discovered-concept substring matches into `explorerConceptNodeIds`.
- [x] **Non-related nodes fade, not removed.** Canvas draw path multiplies
  non-concept draw opacity by 0.15 (spec's 0.1–0.2 band); concept nodes stay
  full opacity with an accent border; concept nodes bypass semantic-zoom
  hiding. Mutation probe A (fade disabled) fails the draw-output-pinned
  test — the test is real.
- [x] **Edges between concept nodes highlighted.** Both the leaf-edge and
  the bundled-group draw paths recolor concept-to-concept edges to the
  concept accent with an alpha floor; other edges dim to 0.15×. Covered by
  draw-path stroke-capture tests.
- [x] **Active concept as filter chip in control bar.** Manifest and
  discovered concepts each render a clearable `concept-filter-chip`; test
  asserts chip presence, clear, and full-opacity restoration.
- [x] **Multi-select + right-click → "Create concept".** Shift+Click
  multi-select (pre-existing) + context-menu item (visible at ≥2 selected)
  opens the naming dialog. Test drives real mousedown/mouseup/click/
  contextmenu events against hit-tested canvas positions.
- [x] **Ad-hoc concepts saved as explorer views.** `saveConceptDialog`
  persists a ViewQuery-grammar spec (concept scope, exact seeds =
  qualified names, `expand_depth: 0`) via `POST /workspaces/:id/explorer-
  views`; server-side `parse_and_validate` classifies by disjoint top-level
  keys and accepts ViewQuery. Reload path uses the new
  `viewDocumentToQuery` so a stored ViewQuery document is re-applied with
  its concept scope intact instead of being flattened to `scope:all` —
  round-trip test covers view-selector → fade + annotation.
- [x] **Ask input handles concept questions via LLM.** Prompt template
  (llm_defaults.rs + specs/prompts/explorer-generate.md) now routes "show
  me everything related to X" questions to `data.concept` with a worked
  example. `viewSpecToViewQuery` translates `data.concept` to a concept
  scope with `seed_match: 'substring'` (ui-layout.md §4 data-source
  distinction — substring, not exact seed) and `depth: 0` pins
  `expand_depth: 0`. Mutation probe B (substring flag removed) fails both
  the end-to-end Ask-flow test and the unit tests — real coverage.
- [x] **Substring mechanism consistent across all surfaces.** The
  `seed_match` field is validated in gyre-common, resolved in
  gyre-domain's resolver, and implemented identically in the canvas and
  `view-query-scope.js` resolvers (seed EVERY name/qualified_name substring
  match, then BFS expansion); backward compatible via serde default
  "exact".

## Rebase check

The recorded conflict (ExplorerView.svelte import block) was resolved by
keeping both task-182's `viewDocumentToQuery` and task-176's flow-template
imports; every symbol is used in the merged file. The full frontend suite
(110 files / 2201 tests, including the base-side flow-template suites)
passes on the merged tree, and `cargo check -p gyre-server` is clean.

## Probes

See `/tmp/stage/review-evidence/` (README.md, frontend-focused-tests.log,
frontend-full-suite.log, rust-and-invariants.log, attribution-check-head.log)
plus mutation probes A/B described there. Server graph tests for the three
new endpoints must be executed by the verifier (gyre-server test compilation
exceeds the sandbox budget).

## Non-blocking observations

- The workspace-scope concept listing endpoint is not consumed by the UI at
  workspace scope (the panel fetches repo-scope only); the repo-scope
  Explorer is the panel's context, and the workspace endpoint is available
  to API consumers with tests. No spec text requires workspace-scope UI
  browsing.
- Workspace concept-union dedupe keeps the first-seen description when two
  repos define the same name with different descriptions (repo iteration
  order is adapter-dependent); descriptions are advisory listing text only.
- Coverage matrix row for system-explorer.md §4 still reads `task-assigned`
  — the auditor updates it after review; not implementer scope.
