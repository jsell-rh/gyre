# Review: task-063 — View Query Grammar: Scope Resolution, Emphasis & Rendering Primitives

Spec: `view-query-grammar.md` §2–7 (task rows 4–9 of `specs/coverage/system/view-query-grammar.md`).
Commits: `8917f0d5` (implementation: `highlight.matched.label` rendering + 2 tests + web/dist rebuild),
`3354a199` (task-file flips). No Rust code changed by this task's commits; the Rust resolver was
delivered in task-062's territory (`d7940e85`) and audited here as the criterion "All 6 scope types
resolve correctly with unit tests" claims it.

## Round 1

Scope verified:

- **Rust scope resolver** (`crates/gyre-domain/src/view_query_resolver.rs`): all 6 scope variants
  resolve. `all`, `filter` (types/name/computed), `focus` (BFS, direction, depth), `test_gaps`,
  `concept` (multi-seed BFS) verified earlier in R1 of task-062's audit; `Diff` (:722–818) verified
  here in detail — temporal mode (`~epoch` on `created_at`/`last_modified_at`, half-open ranges),
  SHA mode (≥7-hex prefix match against `to_commit` on `created_sha`/`last_modified_sha` with
  from-commit exclusion), same-commit warning. Rust unit tests for all 6 variants exist, including
  three `Scope::Diff` tests at :4385–4451 (temporal, same-commit warning, prefix minimum length);
  ran `cargo test -p gyre-domain --lib view_query_resolver::tests::test_scope_diff` → **3 passed,
  0 failed** (gcc linker override; workspace default linker absent — environmental, same caveat as
  task-062 R1).
- **Emphasis rendering** (`web/src/lib/ExplorerCanvas.svelte`): `dim_unmatched` (:2079–2081, with
  tree-group match inheritance :2058–2062), `tiered_colors` by BFS depth (:2143–2147, depth from
  `queryMatchedWithDepth`), `heat` (:2090–2138 + `heatColor` :2152, prefers server `node_metrics`
  from dry-run), `badges` (:3252–3267, `{{count}}` substitution, server-metrics preference),
  `highlight.matched.color` (:2148, default `#fbbf24`), and the new
  `highlight.matched.label` (:3236–3249) drawn below the node box in the matched color, LOD-gated
  `sw > 30 && sh > 14 && cam.zoom >= 0.5` — matches spec §3 "color + label for nodes in the
  result set".
- **Edge filtering** (§4): type filter + result-set restriction both applied — `renderEdges`
  (:964–987) and the edge-draw loop (:3537–3543) skip edges unless both endpoints are in
  `queryMatchedIds`.
- **Zoom** (§5): `$effect` at :4731–4763 — `{"level": N}` clamped to MIN/MAX_ZOOM, `"current"`
  no-op, `"fit"` bounding box of matched layoutNodes × 0.8 padding via animated `targetCam`
  (`lerpCam` :3780), guarded by `lastZoomedQuery` identity so it fires once per query instance.
- **Annotation** (§6): `resolveVars` (:4969–4971) substitutes `$name`, `{{count}}`
  (`queryMatchedIds.size`), `{{group_count}}` (distinct parent prefixes :4955–4968).
- **Interactive bindings** (§7): template capture (:55–68), `$clicked` re-run on canvas click
  (:4122–4132, substitutes clicked node's qualified_name and re-substitutes `$name` in the
  annotation), `$selected` re-run on selection change (:71–87). Click-mode badge in the annotation
  UI (:4979–4981).
- **Tests**: `cd web && npx vitest run src/__tests__/ExplorerCanvas.test.js` → **134 passed,
  0 failed** (verified in this environment). Both new label tests pass.

Findings:

- [ ] **F1 — Frontend `diff` scope is dead code against real API data: reads a nonexistent field
  and ignores `to_commit`.** `web/src/lib/ExplorerCanvas.svelte:2027–2038` filters on
  `n.last_commit_sha && n.last_commit_sha !== scope.from_commit`. The server's
  `GraphNodeResponse` (`crates/gyre-server/src/api/graph.rs:59–63`) serializes
  `last_modified_sha`, `last_modified_by`, `last_modified_at`, `created_sha`, `created_at` —
  there is no `last_commit_sha` field anywhere in production data (grep across `web/src` finds it
  only at ExplorerCanvas.svelte:2033 and its test mirror). On a real graph every node fails the
  `n.last_commit_sha` truthiness check → `matched.size === 0` → returns `null` → no diff
  rendering at all: no highlight, no dim, no `{{count}}`, no `zoom: "fit"`. The code path is
  unreachable-in-effect dead behavior. Even if the field existed, the logic ignores `to_commit`
  entirely and diverges from the Rust resolver's semantics
  (`view_query_resolver.rs:722–818`: SHA-prefix matching with ≥7-hex minimum, from-commit
  exclusion, `~epoch` temporal mode) — the server sends the raw query JSON to the client
  (`explorer_ws.rs:3017–3021`, `ExplorerServerMessage::ViewQuery { query }`), so this client-side
  resolution is authoritative for rendering. The only frontend "coverage" is a test helper that
  re-implements the same broken field name (`ExplorerCanvas.test.js:441`) fed by a fixture that
  fabricates `last_commit_sha` props (:550) — the wrong field name is enshrined, not caught.
  The task checks "All 6 scope types resolve correctly with unit tests" — this fails for `diff`
  on the rendering surface. Fix: port the Rust semantics (match `to_commit` prefix against
  `last_modified_sha`/`created_sha`, exclude from-matches, support `~epoch`) using the fields the
  API actually returns, and fix the test helper/fixture to use real field names.
- [ ] **F2 — Frontend `all` scope produces no result set, making §3/§4/§5/§6 primitives inert for
  `all`-scope queries.** `queryMatchedWithDepth` (`ExplorerCanvas.svelte:1933–2041`) has branches
  for `focus`, `test_gaps`, `filter`, `concept`, and `diff` — but no `scope.type === 'all'`
  branch, so `all` falls through to `return null` (:2040). Spec §2 defines `all` = "Show
  everything". Consequences, all verified by code path: (a) `{{count}}` renders as `'?'` and
  `{{group_count}}` as `'0'` (:4954, :4957 — `queryMatchedIds?.size ?? '?'`); (b) `dim_unmatched`
  is inert (:2079 returns 1.0 when `queryMatchedIds` is null) — an `all` + `dim_unmatched: 0.3`
  query dims nothing; (c) edge restriction to result-set connections is inert (:3537 guard);
  (d) `zoom: "fit"` early-returns (:4744) so it never zooms; (e) `highlight.matched` color/label
  and `tiered_colors` are inert (:2141–2142 guard). Only `heat` and `badges` work for `all`
  scopes because they don't depend on the result set. The client-side resolution is authoritative
  for rendering (see F1), so `{"scope": {"type": "all"}, "emphasis": {"dim_unmatched": 0.15},
  "annotation": {"title": "{{count}} nodes"}}` — a legal query under the spec — renders wrong
  counts and no emphasis. Fix: add an `all` branch returning every active node id at depth 0.
- [ ] **F3 — The two new tests from `8917f0d5` do not defend the behavior they claim.** The
  negative test "does not draw highlight label when emphasis has no label"
  (`ExplorerCanvas.test.js:268–277`) uses the deep `NODES` fixture, whose matched leaf is never
  drawn as a leaf node at test zoom — the fixture renders only tree-group summaries ("api — 4
  nodes", "domain — 2 nodes", …), so the label-drawing branch at ExplorerCanvas.svelte:3239 never
  executes whether or not a label is configured. Verified empirically in this environment:
  rendering the same fixture with `label: 'Untested'` present produces **0** `fillText('Untested')`
  calls — the negative test passes identically with the guard deleted, i.e. it is vacuous. The
  positive test's color assertion `expect(mockCtx.fillStyle).toBeDefined()` (:265) is tautological
  — `mockCtx.fillStyle` is initialized to `''` in the mock (:34) — so the commit-message claim
  "colored with the matched color" is never verified (nothing asserts `#ef4444`). Fix: give the
  negative test a flat fixture (the positive test already demonstrates the pattern) so the label
  branch executes, and assert the fillStyle captured at the `fillText` call (e.g. record
  `fillStyle` inside a `fillText` spy) equals the configured matched color.

Checked, not filed:

- `filter` scope `name_pattern`: frontend uses `new RegExp(pattern, 'i')` (:1986–1988) while Rust
  uses `contains` — divergent on regex-special inputs. Pre-existing from `d7940e85`, untouched by
  this task's commits; same defused class as task-062 R10's triaged frontend/Rust divergences.
  Note for the eventual frontend-resolver-unification task.
- `{{group_count}}` algorithm: frontend splits `qualified_name` on `[:.]` (:4962) vs Rust
  `count_distinct_parent_modules` using Contains-edge parents with `file_path` fallback
  (:1699–1724) — can disagree on the same graph; same pre-existing/diverged class as above.
- `timeline-utils.js` uses the correct `last_modified_sha` — only the diff scope and its test
  mirror use the wrong name (F1).
- Task's cargo/npm caveats (missing libpq link, blocked loopback TCP, pre-existing 23 vitest
  failures reproducing on clean checkout) are environmental and documented in the task file; the
  verifiable subset is green (134/134 ExplorerCanvas vitest, 3/3 Rust diff tests).

F1 and F2 are correctness breaks of checked acceptance criteria ("All 6 scope types resolve
correctly", "Emphasis primitives render correctly", "Annotation templates resolve `{{count}}`/
`{{group_count}}`"); F3 is test inflation on this task's own new tests. Setting
`progress: needs-revision`.
