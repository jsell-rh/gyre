# Review — task-182 (Concept Views — cross-cutting architecture views by concept)

Spec: `specs/system/system-explorer.md` §4 "Concept Views".
Candidate: `953e04c5c9dce74c3199b36f2f943ebba76951bf` (base `603cc2f7`, rebase-repair
round for durable finding `ef11cf4f` / PR 807 merge_state DIRTY).
Verdict: **approved** — all acceptance criteria hold with independent probe
evidence; no blocking findings. Evidence: `/tmp/stage/review-evidence/`
(`review-summary.md` and the per-probe transcripts).

## Round (this review)

### Rebase repair verified

Diff `ce09033..953e04c` across every task-182 source file shows zero
implementation drift: only `ExplorerCanvas.svelte` +2 lines, both main's
task-183 timeline prop pass-throughs. `git diff base..candidate -- web/dist`
is empty — the conflicted generated assets were kept at main's state for the
verifier to republish (correct ownership per process). The combined tree was
reviewed as a whole (diff `603cc2f..953e04c`, 27 files).

### Acceptance criteria — evidence

- **Concept browser lists predefined concepts from manifest** — `GET
  /repos/:id/graph/concepts` + workspace twin added (`graph.rs`, `mod.rs`)
  with ABAC resolver mappings (registry script passes); panel renders a
  "Predefined" section distinct from "Discovered". Frontend test passes with
  mocked API; server unit tests drive the real router against temp-dir git
  manifest repos (deferred to verifier per the 4 GiB rule, listed below).
- **Selecting a concept filters canvas / non-related nodes fade, not removed**
  — manifest projection (`getGraphConcept`) resolves the concept node set;
  the canvas fades non-concept nodes to `CONCEPT_FADE_ALPHA = 0.15` (drawn,
  never removed) and boosts concept nodes to full opacity. Verified by
  draw-path alpha assertions (label `globalAlpha` at `fillText`), and by a
  mutation probe: setting the fade constant to 1.0 fails both the manifest
  and discovered-concept fade tests.
- **Edges between concept nodes highlighted** — canvas draw-path test asserts
  the concept accent `#a78bfa` at alpha ≥ 0.8 for concept edges/borders while
  non-concept edges fade to ≤ 2% of their base alpha; a no-concept control
  test pins that no emphasis leaks without an active concept.
- **Active concept shown as filter chip in control bar (click to clear)** —
  `concept-filter-chip` + `concept-filter-chip-clear` testids asserted, with
  restoration of baseline opacity after clearing.
- **Multi-select + right-click → "Create concept"** — Shift+click
  hit-tested on the real canvas, contextmenu, `ctx-create-concept` menu item
  (visible only at ≥2 selected), naming dialog with name-required and
  server-error branches all tested through real component interaction.
- **Ad-hoc concepts saved as explorer views** — `saveExplorerView` payload
  asserted (ViewQuery concept scope, exact seeds = selected nodes'
  qualified names, `expand_depth: 0`); the server's `parse_and_validate`
  accepts ViewQuery-grammar specs (grammar classification pre-exists), and
  the reload round trip (`viewDocumentToQuery`) is unit-tested plus a
  full-component reload test asserting the grouping re-applies (fade ratios
  + annotation title).
- **Ask input handles concept questions via LLM** — prompt template
  (`specs/prompts/explorer-generate.md`) and `llm_defaults.rs` fallback now
  instruct concept questions → `data.concept`; `viewSpecToViewQuery` maps
  `data.concept` to `seed_match: 'substring'` (and pins `depth: 0`), the
  substring mechanism is implemented consistently in ExplorerCanvas,
  view-query-scope.js, and the Rust resolver (`gyre-domain` tests: 507/507
  passed, including 4 new seed_match tests). Mutation probe: removing
  `seed_match: 'substring'` fails 4 tests including the end-to-end
  Ask-input test whose fixture has no node *named* "auth" — exact matching
  resolves nothing there.
- **Tests pass** — full web suite 112 files / 2293 passed (41 skipped),
  exit 0 after `npm ci`; `cargo check -p gyre-server` clean; mechanical
  invariant scripts (ABAC registry, arch, MCP write tools, migration
  versions, task-commit attribution against the candidate frontmatter) all
  exit 0.

### Data source distinction (spec requirement)

Both mechanisms are implemented and kept distinct, with the grammar
extension documented in `view-query-grammar.md`: manifest-based projections
(`/graph/concept/:name`, exact case-insensitive concept-name match, 404 on
undefined) vs the ViewSpec `data.concept` substring mechanism
(`seed_match: "substring"`, case-insensitive substring over name and
qualified_name, seeding all matches). Ad-hoc groupings keep exact seeds.

### Verifier must run (server test compilation exceeds the review budget)

- `cargo test -p gyre-server --lib api::graph::tests` — includes the three
  new endpoint tests `list_graph_concepts_returns_manifest_concepts`,
  `list_graph_concepts_repo_without_manifest_returns_empty_list`,
  `list_workspace_graph_concepts_unions_and_dedupes`.
- `cargo test -p gyre-server --test graph_integration`.
- `cd web && npx playwright test tests/e2e/explorer-visual.spec.js` —
  filter-preset baselines were regenerated (commit `376a3f6f`) for the new
  matched-container emphasis; needs the release server per playwright
  `webServer` config.

### Non-blocking observations

- `api.workspaceGraphConcepts` has no component consumer yet (manifest
  concept browsing renders at repo scope, where the canvas filter panel
  lives); the workspace endpoint itself is implemented and tested
  server-side.
- An empty-string seed under `seed_match: "substring"` in a hand-crafted
  ViewQuery would match every node; unreachable from the ViewSpec path
  (trimmed + falsy-guarded).
- Coverage row `system-explorer.md` §4 remains `task-assigned` — coverage
  matrices are updated by the dedicated audit stage per repo convention.

### Generated-output note (verifier-owned, not a finding)

The candidate's committed `web/dist/` is byte-identical to base/main's — the
intentional merge resolution for the conflicted generated paths. The
authored frontend source and tests were reviewed; the verifier rebuilds and
rebinds publication to the tested tree.
