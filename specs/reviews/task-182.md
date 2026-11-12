# Review: TASK-182 — Concept Views (cross-cutting architecture views by concept)

**Reviewer:** Independent review agent
**Date:** 2026-11-12
**Comparison base:** 7cf2f81c04721e5e97ee390ecaca6dd23ac9e502
**Candidate:** 9ae6481786f074ce3974690e9d583e47935dcebe
**Verdict:** needs-revision (not approved)

---

## What was verified as real and working

- **Concept browser (mode 1, manifest mechanism).** New `GET /repos/:id/graph/concepts`
  and `GET /workspaces/:id/graph/concepts` (graph.rs) read the repo's
  `specs/manifest.yaml` via the same `read_git_file`/`parse_manifest` path as the
  existing single-concept projection endpoint (refactored into `manifest_concepts`);
  the workspace variant unions across repos with case-insensitive dedupe. Both
  routes are registered in `mod.rs` AND in the ABAC resolver
  (`check-abac-route-registry.sh` passes; no exemption-file growth). The filter
  panel renders Predefined (manifest) and Discovered (substring) sections; the
  shell fetches the listing when the panel opens (once per repo, retry on error)
  and selects fetch the `graph/concept/:name` projection.
- **Canvas concept emphasis.** `conceptNodeIds` (manifest projection ∪ discovered
  substring matches) drives a 0.15 fade multiplier on non-concept nodes (drawn,
  never removed — the §4 "fades to background" semantics; task-178's hide-by-
  concept behavior was correctly superseded), a highlighted accent border on
  concept leaves and containers (with a genuine en-route fix: `treeGroupHasAny`
  compared Svelte `$state` proxies by reference and never matched), concept nodes
  bypass semantic-zoom type hiding, and both the individual and bundled edge
  paths highlight concept-internal edges. Control-bar filter chips (manifest and
  discovered) render and clear.
- **Ad-hoc concept creation (mode 2).** Shift+Click multi-select + right-click
  "Create concept" (context menu and selection bar) → naming dialog →
  `POST /workspaces/:id/explorer-views` with a validated ViewQuery concept scope
  (`expand_depth: 0`, empty `expand_edges`, seed = selected qualified names).
  Synthetic tree-group selections resolve to real graph node ids first.
  Validation/error paths surface inline.
- **Tests.** `ExplorerConceptViews.test.js` (6 tests) pins the fade against actual
  canvas draw output (label alpha ratios vs baseline — fails on both removal and
  no-op regressions), the save payload, the dialog error paths, and edge emphasis
  at the draw-path level. Task-178's discovered-concept test was correctly
  rewritten to fade semantics.
- **Probes run (evidence in /tmp/stage/review-evidence/):**
  - `npm ci` + full frontend suite: **107 files, 2148 passed / 41 skipped**
    (vitest 4.1.0 from the lockfile).
  - `cargo check -p gyre-server`: **clean** (7m53s cold; note: build.rs rebuilt
    web assets during the check — `web/dist` restored afterward; generated-output
    observation only, verifier owns that tree).
  - `scripts/check-abac-route-registry.sh`, `scripts/check-arch.sh`: pass.
  - Node probe of the view-spec concept pipeline (see Finding 1).

## Findings (blocking)

### 1. Mode 3 ("Ask 'show me everything related to X'") is prompt text only — the applied concept view filters nothing

Acceptance criterion: *"Ask input handles concept questions via LLM."* The
candidate's delivery for this mode is a prompt-template edit
(`llm_defaults.rs` + `specs/prompts/explorer-generate.md`) instructing the LLM to
set `data.concept`, plus the claim in the shipped notes that this covers "both
data-source mechanisms". It does not work end-to-end:

- `viewSpecToViewQuery` (web/src/lib/generated-view.js:62-75) translates
  `data.concept: "auth"` into `scope: {type:'concept', seed_nodes:['auth']}`.
- The concept scope resolves seeds by **exact** match on `name`/`qualified_name`/
  `id` (web/src/lib/view-query-scope.js:116-119; ExplorerCanvas.svelte:2428-2430
  has the identical exact-match logic) — **not** the case-insensitive substring
  matching the canonical spec requires (ui-layout.md §4 Data Layer: *"concept …
  matches node `name` or `qualified_name` (case-insensitive substring)"*, repeated
  verbatim in this task's spec excerpt under "Data source distinction") and not
  the `GET /repos/:id/graph?concept=` server substring search the spec maps the
  field to.
- Consequence: a concept question produces a seed term that matches no node →
  the matched set is empty → the scope resolves to `null` → the canvas renders
  the **full graph, undimmed, unfiltered**. Probe
  (`/tmp/stage/review-evidence/probe-concept-fields.out`, run against the
  candidate's source): ViewSpec `{data:{concept:'auth'}}` over nodes
  `JwtAuthProvider`/`TokenStore`/`ShippingCalculator` →
  `resolveScopeNodes → null (full graph, no concept filtering)`.
- Implementation-plan item 5 ("Substring matching: use view spec `concept` field
  for ad-hoc queries") is unimplemented — the candidate touched neither
  `generated-view.js`, `view-query-scope.js`, nor the canvas concept-scope
  evaluation. No test covers the Ask concept path.

A prompt instruction that produces a field nothing consumes is not the specced
behavior; under the goal's rules this acceptance criterion stays open.

### 2. Saved ad-hoc concepts cannot be reloaded as concept views (round-trip broken)

`saveConceptDialog` persists a ViewQuery-shaped spec (`scope.type: 'concept'`,
`seed_nodes`, …) — the server stores and re-serves it correctly
(`ExplorerViewRecord.spec`). But the reload path,
`refreshSavedViews` → `query: r.query ?? viewSpecToViewQuery(r.spec)`
(ExplorerView.svelte:1355), runs ViewQuery documents through a translator that
only reads ViewSpec `data`-layer fields → `{scope:{type:'all'}, zoom:'fit'}`.
Selecting the saved ad-hoc concept from the view selector applies the full
graph; the grouping is silently discarded. The dialog's own copy promises
"Saving stores it as an explorer view **you can reload from the view
selector**". Probe evidence: `/tmp/stage/review-evidence/probe-concept-fields.out`
(second half — the exact spec `saveConceptDialog` posts translates to
`scope:{type:'all'}`). The loader predates this task (system default views are
ViewQuery-shaped and ride the same path), but this feature is built on it and
its user-visible promise is false as shipped.

## Server tests the verifier must execute

- `cargo test -p gyre-server --lib graph::tests::list_graph_concepts_returns_manifest_concepts`
- `cargo test -p gyre-server --lib graph::tests::list_graph_concepts_repo_without_manifest_returns_empty_list`
- `cargo test -p gyre-server --lib graph::tests::list_workspace_graph_concepts_unions_and_dedupes`
- Full `cargo test --all` and the full web suite (both green in this review's
  frontend run; the Rust test bodies were read and use the real router via
  `oneshot` against a manifest-backed temp git repo, but gyre-server test
  compilation exceeds this sandbox's budget — deferred to the verifier per
  standing instruction).

## Minor observations (non-blocking)

- `manifest_concepts` returns an empty list for an unparseable manifest (same
  swallow as the projection endpoint's `None`); documented in-code, consistent
  with the sibling endpoint.
- Implementation-plan wording "low opacity, desaturated": only opacity fades are
  implemented (no desaturation filter). The §4 spec sentence itself says only
  "fades to background" — not treated as a gap.
- `web/dist` was regenerated by build.rs during `cargo check` and restored;
  generated output is verifier-owned and not part of this verdict.
