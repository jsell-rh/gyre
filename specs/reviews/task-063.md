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

## Round 2 (revision verdict)

Comparison base `8cde8130`, HEAD `5201eb7`. Repairs independently verified; each R1 finding
re-checked against the current diff, not the task prose.

- **F1 — fixed.** The client diff scope (`ExplorerCanvas.svelte:2035-2070`) now reads
  `created_sha`/`last_modified_sha`/`created_at`/`last_modified_at` — exactly the fields
  `GraphNodeResponse` serializes (`api/graph.rs:59-63`; no `last_commit_sha` exists in
  production data; the name survives only in explanatory test comments). Semantics ported
  faithfully from `view_query_resolver.rs:722-818`: both refs required (missing `to_commit` →
  empty), `~epoch` temporal mode with half-open `(from, to]` ranges, SHA mode with ≥7-char
  target-prefix guard, to-match on either sha, from-exclusion when both shas match from.
  Only cosmetic difference: Rust's `sha_lower.starts_with(target) || sha_lower == target`
  (:797) is redundant under `startsWith`; the JS omits it — identical truth table. The
  component-level test (`ExplorerCanvas.test.js:365-389`) feeds real field names and asserts
  `{{count}}` renders `1`; the mirror helper was rewritten to the same semantics.
- **F2 — fixed.** `queryMatchedWithDepth` has an `all` branch (`:1940-1944`) returning every
  node at depth 0, consistent with sibling scopes (all iterate raw `nodes`). Downstream
  consumers confirmed keyed on `queryMatchedIds` (`:2081`): `dim_unmatched` `:2117`,
  edge restriction `:3575-3576`, `zoom: "fit"` bbox `:4782-4798`, `{{count}}` `:4992`,
  `{{group_count}}` `:4995`, `highlight.matched`/`tiered_colors` `:2179-2183`. For `all` +
  `fit`, the bbox is the full graph — spec-correct (result set = everything).
- **F3 — fixed, and mutation-verified in this environment.** Positive label test captures
  `fillStyle` at each `fillText` call via spy and asserts `#ef4444` on every `'Untested'`
  draw; negative test uses the flat fixture (matched leaf `get_user` asserted drawn first,
  so the draw path executes) and additionally asserts every drawn text is a real string —
  deleting the `hlLabel` guard draws `'undefined'` and fails. All four mutations
  independently reproduced here, each failing exactly its targeted test:
  guard deletion → "does not draw highlight label…" fails; `all`-branch deletion →
  "all scope: {{count}}…" fails; dead-field reversion → "diff scope: {{count}} resolves
  against real commit fields…" fails; label color substitution → "draws
  highlight.matched.label… with the matched color" fails. Working tree restored clean
  after each probe.
- **Suite:** `cd web && npx vitest run src/__tests__/ExplorerCanvas.test.js` → **139 passed,
  0 failed** in this environment.
- **Commit attribution:** `bash scripts/check-task-commit-attribution.sh` → OK. The four
  frontmatter SHAs (`d22f50f`, `f27ff43`, `2d45617`, `2bb9fdf`) are the rebased branch
  commits and match their content (label rendering+dist / source fix / test fix / F3
  hardening); the prose SHAs `dce939e`/`a07b912` are pre-rebase equivalents — diagnostic
  provenance only, not the comparison base.
- **web/dist staleness — checked, defused.** HEAD's committed dist is the pre-F1/F2 bundle
  (`last_commit_sha` present, no `type === 'all'` branch; last dist commit `d22f50f` predates
  the source fix `f27ff43`). This is not a task defect: the loop's checkpoint machinery
  explicitly classifies `web/dist` as a build artifact — `dev-remote.sh:105-108` runs
  `git restore --worktree -- web/dist && git clean -fd -- web/dist` before every checkpoint
  commit, so fix commits mechanically cannot carry a dist rebuild — and the integration gate
  `dev-check.sh:83-86` rebuilds web from source (`npm ci && npm run build`) before promotion,
  then restores committed dist. Historical precedent: task-097/095/102 source fixes shipped
  the same way, with the dedicated `44a8187 build(web): regenerate dist from merged sources`
  landing at merge. No mechanical gate enforces dist freshness; none is needed given the
  integration rebuild.

Residual notes (unchanged from R1, triaged, not blockers):

- `filter` `name_pattern`: frontend regex vs Rust `contains`; `{{group_count}}`: frontend
  qualified-name split vs Rust Contains-edge/file_path parents. Both pre-existing from
  task-062's resolver, can disagree on edge-case graphs, and belong to the prospective
  frontend-resolver-unification task.
- Task-file npm caveat (23 pre-existing full-suite failures reproducing on a clean checkout)
  was A/B-verified by the implementer; the ExplorerCanvas file itself is green here.

F1, F2, F3 all genuinely repaired with mutation-verified regression coverage. The task's
checked criteria are met on the verifiable surface. Setting `progress: complete`.
