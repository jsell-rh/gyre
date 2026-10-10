# Review — task-170 (View Specification Grammar — TypeScript types and server-side validation)

Spec: `specs/system/ui-layout.md` §4 (lines 346-546): four grammar layers (Data/Layout/Encoding/Highlight), side-by-side composability (depth ≤ 1, sub-views = data/layout/encoding only, no field inheritance), flow⇒trace_source, filter.spec_path⇒repo_id, repo_id validated against workspace membership (400 on violation, CRUD + generate), layout registry extensibility (open for new layouts, closed for modification), LLM grammar-bound output.

Assigned base `19d65446` → candidate `a83aef11`. Changed files: `crates/gyre-common/src/view_spec.rs`, `crates/gyre-server/src/api/explorer_views.rs`, `web/src/lib/types/view-spec.ts`, `web/src/lib/layoutRegistry.js`, `web/src/lib/MoldableView.svelte`, two new frontend test files, `web/dist/*` (rebuilt bundle), `specs/tasks/task-170.md` (lifecycle + Shipped only). Working tree at candidate verified clean before and after all probes.

Verdict: **approve**. Prior contract finding 97c0fbed is repaired: the task file keeps its original `[ ]` checkboxes (only `progress: not-started → ready-for-review` and `commits: [] → [9 SHAs]` changed); coverage matrix untouched; implementation tree byte-identical to published candidate `06e0665b` (`git diff 06e0665b a83aef11 -- crates/ web/src/` is empty — the Shipped claim is accurate).

## Round 1 (independent, at candidate a83aef11)

Probe runs (evidence: `/tmp/stage/review-evidence/task-170-a83aef11-*.txt`):

- `cargo test -p gyre-common --lib view_spec` — **13/13 pass** (cold build).
- `cargo test -p gyre-server --lib api::explorer_views` — **16/16 pass** (cold build 16m40s; build.rs web rebuild left tree clean, dist byte-identical).
- `cd web && npx vitest run --pool=threads src/__tests__/view-spec.test.js src/__tests__/MoldableViewListView.test.js` — **18/18 pass** (locked `node_modules`; the `npm ci` invocation itself was cut off by the 600 s sandbox command window after completing installation — a host run of `npm ci` should confirm, restriction recorded, not a code defect).
- Mechanical gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-task-commit-attribution.sh` (all 9 attributed commits reachable), `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh` — all exit 0.

**Mutation probes (proof the tests are not self-confirming):**

- *Mutation 1* — neutered server `parse_and_validate` (`if true { return Ok(()) }` injected): `cargo test -p gyre-server --lib api::explorer_views` → **5 FAILED** (`create_view_validates_spec`, `create_view_rejects_spec_path_filter_without_repo_id`, `create_view_rejects_hybrid_viewquery_viewspec_payload`, `create_view_rejects_side_by_side_sub_view_nesting`, `generate_rejects_invalid_llm_view_spec_with_fallback`). The repo-ownership tests still passed — correct, since they exercise the separate `validate_repo_ownership`, which mutation 1 did not touch.
- *Mutation 2* — neutered `gyre-common::view_spec::validate_view_spec`: `cargo test -p gyre-common --lib view_spec` → **7 of 13 FAILED** (all grammar-rejection tests; parse/roundtrip/positive tests correctly unaffected).
- Both mutations reverted; tree clean at `a83aef11` after probes.

Verified working (no findings):

- **Server enforcement is real, belt and suspenders holds.** POST (`explorer_views.rs:329`) and PUT (`:435`) call `parse_and_validate` (grammar) + `validate_repo_ownership` (top-level **and** each side-by-side sub-view `repo_id` against the repos port, `:654-685`); `/generate` validates LLM output before the SSE `complete` event — invalid or foreign-repo spec ⇒ `{view_spec: null, fallback: {layout: "list"}}`, never a 500 and never an unvalidated forward (`:576-598`), exactly matching the ui-layout.md §2 Ask-input contract verbatim. No `bail!`/stub/audit-only paths anywhere in the diff.
- **The hybrid-grammar bypass is genuinely closed.** Base code tried ViewQuery first and fell through on validation failure, letting a hybrid payload ride the ViewQuery parse and smuggle unvalidated ViewSpec fields (serde ignores unknown fields). The candidate classifies by top-level keys and rejects mixed payloads — proven by `create_view_rejects_hybrid_viewquery_viewspec_payload` failing under mutation 1. Sub-view `deny_unknown_fields` enforces the data/layout/encoding whitelist at parse time, so smuggled top-level-only fields inside sub-views 400 at deserialization.
- **Types match the spec examples exactly.** `LAYOUT_TYPES` (TS) and `#[serde(rename_all = "kebab-case")]` (Rust) both carry the eight §4 layout names in kebab-case — this fixed a real base defect where Rust serialized `side_by_side` (snake_case) contrary to every §4 JSON example. No stale snake_case consumers repo-wide; seeded system default views use the ViewQuery grammar and are unaffected. Encoding/highlight/data typedef field lists match the §4 tables one-for-one (all 11 encoding properties, all 8 data fields, all 3 highlight fields, `trace_source{mr_id?, gate_run_id?}`).
- **No field inheritance / nesting rules enforced on both sides.** Rust `validate_view_spec` + `validate_sub_view` and TS `validateViewSpec`/`validateSideBySide` implement the identical rule set (flow⇒trace_source incl. sub-views, spec_path⇒repo_id per-sub-view with no parent inheritance, both-subviews-required, depth-1, sub-view whitelist, orphan left/right on non-side-by-side rejected). Cross-verified rule-by-rule against the spec text.
- **Registry pattern is real.** `layoutRegistry.js` (`registerLayout`/`getLayout`/`listLayouts`, duplicate no-op = closed for modification) drives `MoldableView.svelte` tabs and renderer dispatch; `registerLayoutName` keeps client grammar validation in sync so a registered layout validates client-side. The registry-dispatch refactor's list-view regression is covered by `MoldableViewListView.test.js`.
- **Contract hygiene.** ABAC `RouteResourceMapping` entries for all three routes pre-exist and remain; coverage matrix and all other specs untouched vs base; `check-task-commit-attribution.sh` passes with all 9 attributed commits.

Notes (non-blocking, recorded for completeness):

- PUT update-with-spec has no dedicated test, but it calls the same `parse_and_validate` + `validate_repo_ownership` pair proven by mutation probe on the POST path.
- `validateViewSpec` (client) and Rust validation exist per the "belt and suspenders" instruction; no Svelte component currently invokes `validateViewSpec` before rendering saved specs (the Explorer's ViewQuery editor has its own inline validation). The renderer-integration half of §4's "client-side check before rendering" sentence belongs to the rendering tasks (task-172+/Canvas+Controls), not to this types-and-validation task.
- TCP-listener probes are unsupported in this sandbox (errno 95, `/tmp/stage/capabilities.json`); live-server HTTP smoke, full workspace suites, and all-target Clippy remain with verification/CI as scoped.
