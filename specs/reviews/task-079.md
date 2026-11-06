# Review — task-079 (Explorer Unit Tests — View Query Resolver, Graph Summary & Saved Views)

Spec: `explorer-implementation.md` §30 (Unit Tests).
Comparison base: `88bd81bfcda47da2366c8080c13e69d062682559` → candidate `112e98af46bb6e9ca2a7883125aee42e7cd600aa`.
Commits under review: `4058b618` (the only product-surface commit, listed in the task frontmatter); `05055a0b`, `15d51ff8`, `93013f3d`, `112e98af` touch only the task record.

## Round 3 (this review)

### Scope and nature of the change

Tests-only. `git diff base..candidate` = +462 lines in `crates/gyre-domain/src/view_query_resolver.rs` (pure additions inside `mod tests`, after the pre-existing `test_graph_summary_top_functions_by_calls`), +196 lines in `crates/gyre-server/tests/graph_integration.rs` (one new test), and the task record. No production code, scripts, specs, or exemptions changed. Working tree clean at the candidate.

### New tests verified against production code (static, line-by-line)

Every new resolver test drives the production `dry_run()` (view_query_resolver.rs:1728) or `ViewQuery::validate()` (gyre-common view_query.rs:254) and asserts on real outputs:

- **Heat emphasis values**: `incoming_calls` — production `count_incoming_calls` filters `EdgeType::Calls` only, so the test's `Contains` edge correctly does not count (`hot`=3, `sink`=1, uncalled=0.0, all 5 matched nodes present). `complexity`/`test_coverage` — production uses `and_then(|n| n.complexity)`/`n.test_coverage` inside `filter_map`, so `None`-data nodes are excluded exactly as asserted. `churn` always present. `test_fragility` — `compute_all_test_fragility` BFSes from each `test_node` and counts distinct tests per node (shared=2, single=1, orphan=0). Trace metrics — production pushes "[info] Heat metric 'span_duration' requires OTLP trace data…" and returns `Some(0.0)`; the test kills both the silent-fabrication and the missing-warning classes.
- **tiered_colors / node_depths**: Focus `$clicked` incoming BFS depths 0/1/2 asserted; `node_depths` is documented as the tiered_colors input and is populated by focus-scope resolution (MAX_DEPTH=100, so depth 10 is valid).
- **Callouts**: exact → prefix (clean) → substring (precision warning) order verified against the resolution ladder; "Real" is a prefix of "RealService" (no warning), "Service" is substring-only (warning "resolved via substring match").
- **Narrative**: resolution counts, unresolved-steps warning naming each missing node and not resolved ones, and `validate()`'s "Duplicate narrative step order" rejection (HashSet on `order`).
- **$selected edge cases**: no selection → empty result + "Interactive mode: click a node…" hint and *no* spurious "Scope matched 0 nodes" (the `is_interactive` branch in `dry_run`); with selection → focus resolves (`"$clicked" | "$selected" => selected_node_id.unwrap_or("")`).
- **Highlight color validation**: `is_valid_color` accepts `#ef4444`/`tomato`/`rgb(255, 0, 0)` (all in CSS_NAMED_COLORS / hex / rgb-function paths) and rejects `not-a-color` with an error containing "highlight.matched".
- **Groups are overlays**: production group resolution requires `result_set.contains(id)` in both exact and substring arms; the test proves a pattern matching an out-of-scope node does not pull it in.

**Saved views ownership** (`test_saved_views_ownership_enforcement`, graph_integration.rs): two distinct Agent-role callers registered via real `POST /api/v1/agents` (response read through `RegisterAgentResponse`'s `#[serde(flatten)]` top-level `id`/`auth_token`); non-creator update AND delete return 403 with "Only the view creator or an Admin can …" (exact handler text, saved_views.rs:378-385/466-473); positive controls — the same caller succeeds (200/204) on its own view via the same endpoints, proving the 403 is ownership-specific; `created_by` records the creating agent (saved_views.rs:291); system default view deletion returns 403 "System default views cannot be deleted" (guard fires before ownership). Preconditions verified: agent tokens authenticate as roles=[Agent], tenant "default", `user_id=None`, so `check_workspace_membership` passes (agent/service tokens rely on tenant-level auth) — the 403s cannot come from membership or tenant checks. The routes are in the frozen ABAC exemption baseline with per-handler authz, per the repo invariant.

### Pre-existing coverage audit (claims verified)

Inventory confirms the acceptance-criteria rows that pre-existed this task: scope types `all`/`filter`/`focus` (BFS + incoming)/`test_gaps` (incl. endpoints/types, modules excluded)/`concept`/`diff`; dry-run warnings for empty scope, >200 cluttered, broad group >20 (25-warn/5-no-warn pair), unresolved callouts/narrative by name; graph summary counts, top types by fields, top functions by calls, test-coverage counts; BFS reachability from test nodes (`test_computed_test_unreachable/_reachable`, `test_reachability_is_calls_only…`); saved-views CRUD + system-default seeding (≥4) in `test_saved_views_crud`. System-view delete protection — previously untested — is now covered by the new test's final assertion.

### Probes (this sandbox)

- `cargo test -p gyre-domain --lib view_query_resolver` and `cargo test -p gyre-server --test graph_integration test_saved_views`: **not executable** — crates.io egress connection-refused (curl exit 7; github.com and registry.npmjs.org reachable), no registry cache or vendored sources, cargo exit 101. Verified infrastructure restriction, not a code defect. Logs and deferred commands for remote verification in `/tmp/stage/review-evidence/`.
- Prior execution evidence: attempt-2 round ran the resolver suite → 138 passed, 0 failed at a product surface byte-identical to this candidate (`git diff 93013f3d..112e98af -- crates/ web/` empty). File contains exactly 138 `#[test]` (124 at the task-068-verified state + 14 new — arithmetic consistent).
- Web spot check: `npm ci` then `vitest run ExplorerCanvas.test.js ExplorerViewScope.test.js view-query-validator.test.js` → **208 passed, 0 failed** (task adds no web code).
- Mechanical gates pass: check-arch, check-abac-route-registry, check-mem-port-contracts, check-inert-enforcement, check-task-commit-attribution (all exit 0). Test-quality awk scripts exit 2 identically at the base commit (gawk-only `\s` regexes vs sandbox mawk) — pre-existing environmental; equivalent manual verification performed instead: every new test has real assertions on production outputs, none is assertionless/self-confirming/mirrored-logic, and the ownership test's positive controls mean it fails if the ownership check is removed, weakened, or shadowed by an earlier denial.

### Verdict (round 3)

**approved.** The task contract (fill the §30 unit-test gaps with real tests, no duplicates, no production changes) is met: the 14 new resolver tests and the ownership integration test all exercise production code with assertions that fail when the specced behavior breaks; every assertion was traced to the matching production logic; the only unexecuted checks are blocked by a verified transport restriction and are recorded for remote verification.
