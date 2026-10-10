# Review — task-170 (View Specification Grammar — TypeScript types and server-side validation)

Spec: `specs/system/ui-layout.md` §4 "View Specification Grammar" (Structure, Data Layer, Layout Layer, Encoding Layer, Highlight Layer, Extensibility, LLM Constraints; composability/nesting rules at lines 469-488; LLM-output validation contract at line 158).

Candidate: `f9913ef63e36cb5ed96c65ada80c542b785040c7` from base `e96d25abcdbb51f8890ea36d11541bfa9f80a2b8`. Cumulative implementation diff: `crates/gyre-common/src/view_spec.rs` (+269/−60: snake_case→kebab-case `LayoutType`, `deny_unknown_fields` on `SubViewSpec`, orphan `left`/`right` rejection, 13 tests), `crates/gyre-server/src/api/explorer_views.rs` (+451/−70: dual-grammar classification, `/generate` LLM-output validation with fallback, sub-view-aware `validate_repo_ownership`, 13 new tests), `web/src/lib/types/view-spec.ts` (new, 298 lines), `web/src/lib/layoutRegistry.js` (new), `web/src/lib/MoldableView.svelte` (registry-driven tabs/dispatch), `web/src/__tests__/view-spec.test.js` (new, 17 tests), `web/src/__tests__/MoldableViewListView.test.js` (new regression test), rebuilt `web/dist`. All 10 attributed commits verified ancestors of the candidate; task file delta vs base is lifecycle-only (progress, commits, Shipped section; checkboxes unchanged as `[ ]`).

Verdict: **complete**.

## Round 1

Test runs (candidate tree verified byte-identical via `git diff f9913ef6` after all probes; `SKIP_WEB_BUILD=1`, shared `/tmp/gyre-target`):

- `cargo test -p gyre-common --lib view_spec` → **13 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::explorer_views` (cold build) → **16 passed, 0 failed**.
- `npm ci` (169 packages) then `npx vitest run --pool=threads` on view-spec / viewEvents / MoldableViewListView / MoldableViewNodeTypeFilter → **32 passed, 0 failed** across 4 files.
- Mechanical gates: `check-abac-route-registry.sh`, `check-task-commit-attribution.sh`, `check-in-memory-state-stores.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh` — all exit 0.

Mutation probes (each source restored afterward; restoration verified by empty `git diff f9913ef6`):

1. `validate_view_spec` body → unconditional `Ok(())`: gyre-common **7/13 failed**, gyre-server **4/16 failed** (flow-no-trace_source, nested side-by-side, spec_path-no-repo_id, invalid-LLM-fallback). Grammar tests are load-bearing.
2. `validate_repo_ownership` body → unconditional `Ok(())`: gyre-server **3/16 failed** (`create_view_rejects_unknown_repo_id`, `create_view_rejects_foreign_repo_id_in_sub_view`, `generate_rejects_llm_spec_with_foreign_repo_id`). The workspace-membership criterion is genuinely enforced, including the sub-view smuggle and hallucinated-repo_id paths.
3. `validateViewSpec` → `{valid:true,errors:[]}`: vitest **12/17 failed**. The 5 that pass are the accept-cases + name-list assertion, which cannot fail under this mutation by construction.

Evidence: `/tmp/stage/review-evidence/task-170-f9913ef6-*.txt`.

Verified working (no findings):

- **Four-layer grammar types, both sides, matching the spec JSON.** Rust `ViewSpec`/`DataLayer`/`EncodingLayer`/`HighlightLayer`/`Annotation`/`SubViewSpec` mirror `ui-layout.md` §4 Structure exactly; `LayoutType` uses `#[serde(rename_all = "kebab-case")]` (the spec's `"side-by-side"` string; the base's `snake_case` produced `"side_by_side"` — a real serialization mismatch this task fixed, covered by `layout_names_serialize_as_kebab_case_per_spec`). TS typedefs in `web/src/lib/types/view-spec.ts` mirror the same shapes, following the existing JSDoc-typedef convention of `view-query.ts`.
- **Nesting depth ≤ 1 enforced.** `validate_view_spec` rejects a `side-by-side` sub-view whose own layout is `side-by-side`, requires both `left` and `right`, and `SubViewSpec` carries `#[serde(deny_unknown_fields)]` so top-level-only fields (`name`, `annotations`, `explanation`) fail at parse time. Orphan `left`/`right` on non-side-by-side layouts are rejected (smuggled-content closure). No field inheritance: sub-view `filter.spec_path` without the sub-view's own `repo_id` fails even when the parent declares one — test-proven both in gyre-common and the TS mirror.
- **flow ⇒ trace_source (400), spec_path ⇒ repo_id (400)** on POST/PUT `/workspaces/:id/explorer-views` via `parse_and_validate` + `validate_view_spec` — confirmed by oneshot HTTP tests asserting `StatusCode::BAD_REQUEST`, and by the mutation probes above.
- **repo_id workspace-membership validated at every layer.** `validate_repo_ownership` walks the top-level `data` plus each `left`/`right` sub-view's own `data` (no inheritance), checking each `repo_id` against `state.repos` workspace membership; applied on create, update, and the `/generate` path.
- **`/generate` validates LLM output before `event: complete`.** Base forwarded `predict_json` output verbatim. Candidate runs `parse_and_validate` + `validate_repo_ownership` on the LLM response: invalid grammar or foreign repo_id ⇒ `view_spec: null` + explanation + `fallback: {layout: "list"}` — exactly the §2/§4 contract ("Generated view was invalid — try rephrasing"); valid specs forwarded intact. Never a 500, never an unvalidated forward. LLM-unavailable stays 503.
- **Dual-grammar smuggling closed.** Base's `parse_and_validate` fell through ViewQuery→ViewSpec; serde's unknown-field tolerance let a hybrid payload (ViewQuery `scope` + ViewSpec `left`/`right`) ride the ViewQuery branch and store an unvalidated nested side-by-side. Candidate classifies by top-level keys and 400s on any mix — regression test `create_view_rejects_hybrid_viewquery_viewspec_payload` passes (and remains green under the ownership mutation, correctly: it guards the classifier, not ownership).
- **Layout registry.** `layoutRegistry.js` (`registerLayout`/`getLayout`/`listLayouts`) with duplicate-registration no-op (closed for modification) and `registerLayoutName` keeping `validateViewSpec`'s accepted set in sync; `MoldableView.svelte` renders tabs and dispatches renderers from the registry. Extensibility test proves a newly registered layout passes validation. The list-view table regression (lost in the refactor, restored per commit `9aa19ef9`) is pinned by `MoldableViewListView.test.js`, which fails on the header-only-table state.
- **ABAC.** All three explorer-views routes have `RouteResourceMapping` entries (`explorer_view`, `generate` action override on `/generate`) — `check-abac-route-registry.sh` exit 0.
- **LLM constraints (read-only, grammar-bound, no arbitrary code execution)** are enforced structurally: the generate handler passes the user question solely as the user prompt (template `{{question}}` stripped from the system prompt), validates output against the closed grammar before forwarding, and the generated view is ephemeral (not auto-saved). The grammar's closed set rejects anything outside it with 400/null-view_spec.
- **`web/dist` rebuilt** from the new source (bundle contains `moldable_view`/`tab_flow` strings; `index.html` points at the new hashed assets), so Rust-only builds serve current UI.

Notes (non-blocking):

- The production Explorer UI drives views through the WebSocket/ViewQuery canonical grammar (`validateViewQuery` wired in ExplorerChat); `api.explorerViews`/`generateExplorerView` exist in `api.js` but no Svelte component currently calls them. The task's contract is the grammar types + server-side validation + registry, all of which are real and enforced at the API boundary; wiring a ViewSpec-rendering consumer is task-174/178 territory (§5 scope rendering).
- Client-side validation mirror is exercised by its unit tests and kept in sync with the registry via `registerLayoutName`; the WS render path validates ViewQuery (canonical) rather than ViewSpec — consistent with `parse_and_validate` accepting both grammars.
- Sandbox restriction: TCP-listener probes unsupported (`accept` errno 95, `/tmp/stage/capabilities.json`); no live HTTP smoke run. Enforcement is covered by the in-process oneshot tests and mutation probes above. Host/CI should run `cargo test --all`, `cd web && npm test`, clippy all-targets, `check-arch.sh`, and a live-server smoke of the four 400 bodies from `api::explorer_views::tests`.
