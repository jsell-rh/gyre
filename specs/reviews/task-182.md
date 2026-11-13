# Review — task-182 (Concept Views — cross-cutting architecture views by concept)

Spec: `specs/system/system-explorer.md` §4 "Concept Views" (cross-cutting views by concept; three interaction modes; manifest-projection vs substring data-source distinction per `ui-layout.md` §4).
Comparison base: `a218b9565608f4bf0dc61477688013b0264c2edc` → candidate `fb227c2afb13771ff430e3d0ec860c0cbf1ba1fd`.
Diff: 23 files — Rust (`gyre-common/view_query.rs`, `gyre-domain/view_query_resolver.rs`, server `graph.rs`/`mod.rs`/`abac_middleware.rs`/`llm_defaults.rs`), web (`ExplorerView.svelte`, `ExplorerCanvas.svelte`, `ExplorerFilterPanel.svelte`, `api.js`, `filter-categories.js`, `generated-view.js`, `view-query-scope.js`), tests (new `ExplorerConceptViews.test.js`, 683 lines; 4 modified test files), prompt template `specs/prompts/explorer-generate.md`, grammar doc, api/ui docs, task file.

Verdict: **approved** — all acceptance criteria are backed by real production code and hard tests; probes below.

## What was verified (independently, in this session)

### Manifest mechanism (predefined concepts)
- New endpoints `GET /repos/:id/graph/concepts` and `GET /workspaces/:id/graph/concepts` (`graph.rs`): read the repo's real `specs/manifest.yaml` from git at its default branch via the existing `spec_registry::read_git_file`/`parse_manifest`; workspace variant unions across the workspace's repos with case-insensitive dedupe. No fabrication — a repo without a manifest lists empty (tested), contrasting the single-concept projection endpoint's 404.
- Both routes registered in `api/mod.rs` **and** given `RouteResourceMapping` entries in `abac_middleware.rs` — `scripts/check-abac-route-registry.sh` passes. The repo-scoped handler's authorization shape mirrors the pre-existing `get_graph_concept` exactly (middleware ABAC via the resolver, `find_by_id` + 404); no new exempt-handler surface.
- Server tests (3 new, oneshot against the real router with a real git manifest fixture) type-check via `cargo check -p gyre-server --tests`; the test *build/run* is deferred to the independent verifier per the 4 GiB budget rule (recorded in `/tmp/stage/review-evidence/probes.md`).

### Substring mechanism (ViewSpec `data.concept`)
- `seed_match` field added to the Concept scope in `gyre-common` (serde default `"exact"`, validated against `{exact, substring}`), resolved in `gyre-domain` `view_query_resolver` (substring seeds every node whose name/qualified_name contains the seed, case-insensitive; unmatched seeds warn), and mirrored in **both** frontend evaluation surfaces (`ExplorerCanvas.svelte` query traversal, `view-query-scope.js` `resolveScopeNodes`). `viewSpecToViewQuery` now marks translated concept scopes `seed_match: 'substring'` and pins `depth: 0` to `expand_depth: 0` (ui-layout §4: 0 = matched nodes only).
- Focused Rust tests: `cargo test -p gyre-domain --lib view_query_resolver` — 145 passed including the 4 new substring tests; invalid-`seed_match` validation test passes in `gyre-common`.

### Canvas behavior (fade, not remove; emphasis; edges; chip)
- `ExplorerCanvas.svelte`: concept nodes keep full draw opacity + accent border; everything else multiplies by `CONCEPT_FADE_ALPHA = 0.15` (spec range 0.1–0.2), still drawn; edges between concept nodes recolored/alpha-floored/thickened, others dim with endpoints; concept nodes bypass semantic-zoom hiding; tree-group containers emphasize when they contain concept nodes (the recursion was also fixed to compare by id, not Svelte-proxy reference — the old reference-equality check was dead). Active concept renders as a clearable control-bar chip (`concept-filter-chip`).
- Concept selection no longer hides nodes: `hiddenNodeIds` decoupled from concepts; `conceptNodeIds` is the new emphasis set. Behavior-change tests updated to pin "fade, not remove".

### Ad-hoc concepts
- Shift+Click multi-select + right-click "Create concept" (context menu, gated on ≥2 selected) → naming dialog → `POST /workspaces/:id/explorer-views` with a ViewQuery-grammar spec (`scope.type 'concept'`, exact seeds = selected nodes' qualified names, `expand_depth: 0`). Verified the server's `parse_and_validate` genuinely accepts **both** grammars (classifies by disjoint top-level key sets, deserializes `ViewQuery`, runs `validate()`) — the save path is real, not client-only. Reload round trip uses the new `viewDocumentToQuery` (passes ViewQuery-shaped documents through intact instead of flattening them to `{scope:{type:'all'}}` via the ViewSpec translator).

### LLM concept exploration
- `specs/prompts/explorer-generate.md` gains a Concept Questions section with a worked `data.concept` example; the hardcoded fallback prompt (`llm_defaults.rs PROMPT_EXPLORER_GENERATE`) routes concept questions to `data.concept` on the `"graph"` layout.

### Test strength (mutation-probed)
- `web` focused run: 62/62 pass across the 4 concept-related files; broader Explorer surface: 27 files, 500/500 pass.
- **Mutation kill:** in a disposable worktree at the candidate SHA, disabling the fade (`CONCEPT_FADE_ALPHA` 0.15 → 1.0) makes `ExplorerConceptViews.test.js` fail ("fades non-concept nodes (not removed)") — the tests assert actual canvas draw alphas (baseline-relative ratios), not structural presence, so they fail when the production behavior is disabled. Worktree removed; the review checkout was never modified.

### Process/bookkeeping
- All 4 attributed commits (`39ba4fb69`, `362807160`, `188b9f948`, `a44ad8886`) are ancestors of the candidate; interleaved `process(...)`/merge commits touch only `specs/tasks/task-182.md`. The task-198 base merge conflict (`docs/api-reference.md`) was resolved correctly — the resolved rows accurately describe the code (manifest-based projection for `concept/{name}`, substring search via `?concept=`; the added "history capped at 20 entries" claim is backed by `graph.rs:1696`).
- Coverage matrix (`specs/coverage/system/system-explorer.md` §4 row) remains `task-assigned` — consistent with repo precedent (task-178, `complete`, has the same stale row; coverage status is a separate pipeline stage), not an implementation gap.

## Non-blocking observations
- Substring-mode "partially resolved" warning compares matched-*node* count against seed-*string* count, so one multi-match seed can suppress the aggregate partial warning for an unmatched sibling seed; each unmatched seed still emits its own warning, so nothing fails silently.
- `api.workspaceGraphConcepts` is exported but unused by the UI (endpoints are server-complete; the acceptance criteria only require the repo-scoped browser).

## Verifier must run
- `cargo test -p gyre-server --lib api::graph::tests::list_graph_concepts_returns_manifest_concepts`, `..._repo_without_manifest_returns_empty_list`, `..._unions_and_dedupes`, then the full `cargo test --all` + `web` `npm test` suites (deferred here per the 4 GiB budget rule).

Evidence: `/tmp/stage/review-evidence/` (diffs, probe commands, results).
