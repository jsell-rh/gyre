# Review — task-170 (View Specification Grammar — TypeScript types and server-side validation)

Spec: `specs/system/ui-layout.md` §4 (View Specification Grammar, Structure / Data / Layout / Highlight / Encoding layers, Extensibility, LLM Constraints).
Candidate: `5bab1b275a348ba603530fdd6469237b5a5a1527` (base `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`). HEAD matches the candidate; working tree clean before and after review probes.
Verdict: **approved**.

## Evidence (independent runs at HEAD = candidate; saved under /tmp/stage/review-evidence/)

- `cargo test -p gyre-common --lib view_spec` → **13/13 passed** (spec §4 example parses/validates; kebab-case layout roundtrip for all 8 names; unknown layout rejected; flow⇒trace_source; spec_path⇒repo_id; nested side-by-side; sub-view top-level-only fields rejected via `deny_unknown_fields`; no parent repo_id inheritance; orphan left/right; flow sub-view; valid composition).
- `cargo test -p gyre-server --lib api::explorer_views` → **16/16 passed** (cold build ~11 min; in-process `oneshot` HTTP: 400 on flow-without-trace_source, nested side-by-side, spec_path-without-repo_id, foreign repo_id at top level AND smuggled into a side-by-side sub-view, hybrid ViewQuery/ViewSpec payload; SSE `/generate`: invalid LLM spec ⇒ `view_spec: null` + fallback list view (not 500, not forwarded), valid spec forwarded, hallucinated repo_id ⇒ fallback; 503 LLM-unavailable; rate limit; builtin seeding/idempotency).
- `cd web && npm ci && npx vitest run --pool=threads` (view-spec, viewEvents, MoldableViewListView, MoldableViewNodeTypeFilter) → **4 files, 32/32 passed**, exit 0.
- `bash scripts/check-task-commit-attribution.sh` → exit 0. `check-abac-route-registry.sh` → exit 0. `check-in-memory-state-stores.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`, `check-forwarded-header-trust.sh` → all exit 0.
- Adversarial probe: standalone Rust bin replicating `parse_and_validate` (constants copied verbatim) linked against the candidate's `gyre-common`, run against 14 crafted payloads (`/tmp/stage/review-evidence/task-170-probe-parse-validate.txt`). All smuggling paths rejected: ViewQuery+ViewSpec hybrid (incl. left/right-only riders on a ViewQuery doc), nested side-by-side, orphan left/right on non-sbs layouts, snake_case `side_by_side` (kebab-case now enforced per spec §4), sub-view `highlight` smuggle (`deny_unknown_fields`), flow with `trace_source: null`.
- `web/dist` rebuilt byte-identical to candidate after probes (`git diff 5bab1b27 -- web/dist` empty).

## Verified against the task contract (8/8 acceptance criteria)

1. **ViewSpec TS type, four layers** — `web/src/lib/types/view-spec.ts` typedefs (`DataLayer`/`LayoutType`/`EncodingLayer`/`HighlightLayer`/`SubViewSpec`/`ViewSpec`) match the §4 Structure and composability JSON examples field-for-field.
2. **Rust struct + validation** — `crates/gyre-common/src/view_spec.rs`; `#[serde(rename_all = "kebab-case")]` corrected from the base's `snake_case` (base could not roundtrip the spec's own `"side-by-side"` example — probe 8 confirms snake_case now rejected).
3. **Nesting depth ≤ 1** — enforced in `validate_view_spec` + `validate_sub_view`, `SubViewSpec` has no `left`/`right` fields, and `deny_unknown_fields` blocks re-introducing them; 400 via oneshot test.
4. **flow requires trace_source (400)** — enforced at top level and in sub-views; `create_view_validates_spec` passed in this review's own run.
5. **filter.spec_path requires repo_id (400)** — enforced at both levels incl. no-inheritance case (parent repo_id does not excuse a sub-view); tests passed.
6. **repo_id workspace membership** — `validate_repo_ownership` checks top-level AND each sub-view data layer against the repos port; covered by unknown-repo, foreign-sub-view-repo, and hallucinated-LLM-repo tests, all passing.
7. **Layout registry** — `web/src/lib/layoutRegistry.js` (`registerLayout`/`getLayout`/`listLayouts`; duplicate registration no-ops = closed for modification; `registerLayoutName` keeps `validateViewSpec`'s accepted set in sync); `MoldableView.svelte` tabs and renderer dispatch are registry-driven; extensibility test proves a registered layout name passes validation.
8. **Tests** — see counts above; the tests are adversarial (each asserts a 400/null-forward on a specced invalid case, verified by this review's independent re-run, not just the implementer's logs).

Beyond the ACs, the candidate hardened two real holes in the base: the ViewQuery-first parse order let a hybrid payload smuggle unvalidated ViewSpec fields into storage (the hybrid-classification fix closes this — probe-verified), and `/generate` previously forwarded raw LLM output (now grammar- and ownership-validated with a fallback contract per ui-layout.md §2).

## Non-blocking observations (recorded, no revision required)

- **`validateViewSpec` has no production caller yet** — only tests import it. The Svelte surface that would consume saved/generated ViewSpecs (`MoldableView.svelte`, the registry consumer) was unmounted upstream by d7940e85 (pre-dates the task base) when ExplorerView.svelte moved to ExplorerCanvas; the current `web/src/components/ExplorerView.svelte` does not consume `api.generateExplorerView`'s SSE either. The spec's "Svelte renderer checks before rendering and shows an error message instead of crashing" clause therefore cannot be exercised until the ViewSpec rendering surface lands — that surface is task-171 (LLM Endpoint Contract) / task-173-178 (Explorer rendering) scope per the coverage matrix. The task's contract (types, guards, validation functions, registry) is fully delivered, and the load-bearing barrier (server-side 400 on CRUD + `/generate`) is real and independently verified. No fake-completion claim found: the Shipped section describes the mirror and its callers accurately.
- **Unknown top-level fields on a ViewSpec payload are accepted** (serde default ignores them; probe14). Inert — no renderer consumes unknown fields, and the grammar-critical constraints (nesting, layout set, sub-view whitelist) are enforced. Same tolerance the ViewQuery branch exhibits. A future hardening could add `deny_unknown_fields` to `ViewSpec`, but that is not required by the task or §4 and risks rejecting forward-compatible saved views.
- **Highlight-layer `node_ids`/`spec_path` are not workspace-scoped** — per §4 the highlight layer is visual emphasis only (glowing border), not data scope, so this matches the spec's own assignment of the workspace-leak guard to `data.repo_id`.

## Repairs verified in this round

- `6ab72e55` — rustfmt on the two changed Rust files (4 reflowed lines, no semantic change; diff vs `a83aef11` confirmed formatting-only).
- `394ac741` — ExplorerCanvas/performance test timeout args (30000ms) matching the tests' own documented 15s/20s assertion budgets; assertions untouched, so a timing regression still fails. Repair for verification finding 470a534c (gate log tail was CSS-warning noise pushing the real `Test timed out in 5000ms` failures out of view).
- `9aa19ef9` — restored the list-view `<tbody>` lost in the registry refactor; covered by the new `MoldableViewListView.test.js` regression test (fails on the clobbered state, passes now).
- Task file keeps its assigned `[ ]` checkboxes with status in `progress:`/Shipped, per the contract-preservation finding 97c0fbed; `check-task-commit-attribution.sh` exit 0 confirms all attributed commits reachable and recorded.

## Sandbox restriction (infrastructure, not a code defect)

TCP-listener probes unsupported (`accept` errno 95, `/tmp/stage/capabilities.json`). No live HTTP smoke possible in this sandbox. Enforcement is covered by the in-process `oneshot` tests above, which exercise the full axum router (middleware → handler → port → adapter) minus the socket. Host/CI verification should additionally run the full workspace suites (`cargo test --all`, `cd web && npm test`), all-target Clippy, and `scripts/check-arch.sh`, per the standing division of labor with verification/publication.
