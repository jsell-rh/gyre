# Review — task-170 (View Specification Grammar — TypeScript types and server-side validation)

Spec: `specs/system/ui-layout.md` §4 "View Specification Grammar" (Structure, Data Layer, Layout Layer, Highlight Layer, Encoding Layer, Extensibility, LLM Constraints).
Assignment: base `27bd585c`, candidate `20e98012`.
Verdict: **approved**.

## Scope inspected

Diff `27bd585c..20e98012` touches: `web/src/lib/types/view-spec.ts` (new, 298 ln), `web/src/lib/layoutRegistry.js` (new), `web/src/lib/MoldableView.svelte` (registry-driven tabs/dispatch), `web/src/__tests__/{view-spec,MoldableViewListView}.test.js` (new) + timeout bumps in ExplorerCanvas tests, `crates/gyre-common/src/view_spec.rs` (+269), `crates/gyre-server/src/api/explorer_views.rs` (+451), `.gitattributes` (dist whitespace gate exemption), `specs/tasks/task-155.md` (attribution repair for base commit `27bd585c`, script-prescribed append form), `web/dist` (regenerated bundle hashes).

## Findings: none blocking

Notes (not defects, recorded for the verifier):

1. **Kebab-case rename of `LayoutType` serde is a one-way format change.** `#[serde(rename_all = "kebab-case")]` means a stored spec with `"side_by_side"` (pre-rename wire name) now fails to parse and is rejected with 400 on PUT. No persisted-view migration exists. Checked the ecosystem for consumers of the snake_case wire name: `grep` across `crates/` finds no consumer outside `view_spec.rs`/`explorer_views.rs`; the frontend had no ViewSpec producer before this task (`viewEvents.js`, `SpecDiffView` uses of "side-by-side" are unrelated UI strings). The spec mandates kebab-case JSON (`"side-by-side"`, `"hierarchical"` in §4 examples), and stored specs are LLM/human generated through these endpoints, so the rename brings the wire format into spec compliance; the check-migration-notes in the task file confirm the only previously-persisted producers were the same endpoints. Risk is a stale saved view created pre-task being edited post-task — acceptable for a grammar that had no other emission source.
2. **`PROMPT_EXPLORER_GENERATE`** (`llm_defaults.rs:49-54`) still tells the LLM `layout (one of: "graph", "hierarchical", "list")` — a subset of the eight specced layouts. Pre-existing text, unchanged by this task; not a regression. The belt-and-suspenders server validation covers whatever the LLM emits.
3. **Encoding layer is `serde_json::Value` fields** (color/size/border/... as arbitrary JSON) rather than a typed `FieldScale` struct. The spec's encoding table gives "accepted fields" as guidance to the LLM, not as a closed server-enforced set; the TS side mirrors with a loose `FieldScale` typedef. Consistent with the spec's "open for new layouts" spirit; no enforcement requirement is stated for encoding field values. Not a gap against the task's acceptance criteria, which enumerate only layout/nesting/trace_source/spec_path/repo_id validation.

## Verification (all at candidate `20e98012`, clean tree; evidence: `/tmp/stage/review-evidence/task-170-*-20e98012.txt`)

- `cargo test -p gyre-common --lib view_spec` → **13/13 passed**. Covers: §4 Structure example parses+validates; kebab-case roundtrip for all 8 layouts; unknown layout rejected; flow⇒trace_source (both directions); spec_path⇒repo_id; side-by-side requires both sub-views; nested side-by-side rejected; top-level-only fields in sub-views rejected (`deny_unknown_fields`); no parent repo_id inheritance; orphan left/right on non-side-by-side rejected; flow sub-view; valid composition from the §4 composability example.
- `cargo test -p gyre-server --lib api::explorer_views` → **16/16 passed**. In-process oneshot HTTP: 400 on flow-without-trace_source, nested side-by-side, spec_path-without-repo_id, foreign repo_id (top-level and smuggled into a side-by-side sub-view), hybrid ViewQuery/ViewSpec payload; SSE generate: invalid LLM spec ⇒ `view_spec: null` + fallback list view (not a 500, not forwarded), valid spec forwarded, hallucinated foreign repo_id ⇒ fallback; 503 LLM-unavailable; rate limit.
- `cd web && npm ci` then vitest (`--pool=threads --maxWorkers=1`, sandbox CPU contention): view-spec 17 + viewEvents 9 = **26 passed**; MoldableViewListView **1 passed** (regression test for the registry-dispatch edit — one table row per node). The combined 3-file invocation hits a vitest worker-respond timeout under this sandbox's load (infrastructure, not code; documented in evidence file).
- **Negative control**: mutating `view_spec.rs:10` `kebab-case` → `snake_case` (disabling the specced layout-name enforcement) fails the suite — **8 failed / 5 passed** (layout roundtrip, composition parse, all side-by-side rules). The tests bind to the production enforcement, not mirrored logic. Tree restored to exact candidate bytes afterward (`git status` clean, `git rev-parse HEAD` = `20e98012`, suite back to 13/13; `web/dist` restored after the cargo build.rs regen minted new hashes).
- Static checks: `check-abac-route-registry.sh` exit 0 (ABAC `RouteResourceMapping` for all three explorer-views routes, `abac_middleware.rs:423-434`); `check-task-commit-attribution.sh` exit 0 (all 11 `commits:` entries are ancestors of the candidate); `check-forged-scope-fields.sh`, `check-in-memory-state-stores.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh` all exit 0.

## Acceptance criteria

- [x] `ViewSpec` TypeScript type with all four layers — `view-spec.ts` typedefs match §4 JSON examples exactly (verified against spec lines 352-392, 471-484).
- [x] Server-side Rust struct with serde + validation — `gyre-common/src/view_spec.rs`, 400 via `parse_and_validate` on POST/PUT.
- [x] Nesting depth limit enforced — Rust `validate_view_spec` + TS mirror + server test `create_view_rejects_side_by_side_sub_view_nesting`.
- [x] `flow` requires `trace_source` (400) — enforced top-level and in sub-views, both grammars' paths tested.
- [x] `spec_path` filter requires `repo_id` (400) — enforced with no parent-to-sub-view inheritance.
- [x] `repo_id` validated against workspace membership — `validate_repo_ownership` checks top-level AND sub-view data layers against the repos port; foreign-repo tests for both create and generate.
- [x] Layout registry pattern — `layoutRegistry.js` with `registerLayout`/`getLayout`/`listLayouts`; duplicate registration no-ops (closed for modification); `registerLayoutName` keeps client validation in sync; `MoldableView.svelte` tabs and renderer dispatch driven by the registry.
- [x] Tests pass for validation edge cases — see probes above; negative control proves they fail when enforcement is disabled.

Sandbox restriction: TCP-listener probes unsupported (`capabilities.json`, errno 95) — no live-server smoke here; enforcement covered by the in-process oneshot tests. Full suites, all-target Clippy, and CI are owned by verification.
