# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping.
Comparison base: `d31906c5a16ab8045ab435dfad2bd7dc1ce3f616` → candidate `8d0c9a266949e0c924355276898306534fb5447b`.
Diff: 14 files — port (`gyre-ports/src/search.rs`), all three adapters (sqlite/postgres/mem search), server handlers (`api/search.rs`, `mcp.rs`, new `search_scope.rs`), indexing callsites (`api/agents.rs`, `api/merge_requests.rs`, `api/tasks.rs`), integration tests (`tests/search_wiring.rs`), docs, task file.

Verdict: **approved** (round 2; round 1's blocking finding repaired in `dedfeb4ff` and re-verified).

## Round 1 → round 2

Round 1 (candidate `694508dd`, review checkpoint `a7143fa0`) rejected on one
blocking finding: `search_is_access_scoped_to_the_authenticated_caller` failed
at runtime — the seeded API-key user `alice` had no tenant binding, and API-key
auth fail-closes those users with 403 (task-099 F1), so alice's first scoped
search panicked the status assertion. This candidate carries exactly that
repair (`dedfeb4ff`: `alice.tenant_id = Some(Id::new("default"))`, matching the
seeded workspaces' tenant) plus process commits. No production code changed
between the two candidates.

## What was executed here (evidence: `/tmp/stage/review-evidence/task-203-candidate-8d0c9a2/`)

- `cargo test -p gyre-adapters search` → **25 passed, 0 failed** — SQLite FTS5
  scoping (`access_scoping_filters_inside_the_index_query`,
  `repo_scope_restricts_repos_but_keeps_workspace_level_docs`), mem scoping
  parity, and the four PG composed-SQL alias/placeholder tests. These assert
  facet-count exclusion of the inaccessible workspace (the count-leak kill
  condition) and that a client `workspace_id` outside the accessible set yields
  zero rows — not just row exclusion.
- **Mutation probes** (restore verified, `git status crates/` clean after each):
  - Removing the `workspace_id IN (...)` fragment from SQLite
    `scope_filter_sql` → `access_scoping_filters_inside_the_index_query`
    **FAILS**.
  - Forcing mem `scope_visible` to `true` → 3 mem tests **FAIL**.
  Both adapter test suites are anchored to the enforcement, not incidental
  behavior.
- `cargo check -p gyre-server --tests` → clean (covers `search_wiring.rs` with
  the tenant-binding repair and the new MCP scoping test).
- Gates: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`,
  `check-in-memory-state-stores.sh`, `check-dead-message-kinds.sh`,
  `check-rustfmt-diff.py` vs base — all OK. `check-task-commit-attribution.sh`
  OK against the candidate's committed task file (all seven task-labeled
  product-surface commits listed in `commits:`).

## Verified against the acceptance criteria

1. **Scope from the authenticated caller, not client params.** REST
   `search_handler`/`suggest_handler`/`facets_handler` and MCP `gyre_search`
   all take `AuthenticatedAgent` first and call
   `search_scope::derive_search_scope`: tenant from auth; Admin (incl. global
   dev token) unscoped within the tenant; agent JWTs via `orchestrator_type` /
   `workspace_id` / `repo_id` claims (workspace orchestrator → whole workspace;
   repo orchestrator → its repo); worker agents via their agent record; humans
   via accepted workspace memberships; unknown callers fail closed (empty
   accessible set → zero rows). The client `workspace_id` is bound as an
   `AND workspace_id = ?` *on top of* the accessible-set `IN (...)` — it can
   only narrow.
2. **Index-query-level filtering.** SQLite FTS5 and PG tsvector adapters build
   `AND tenant_id = ? AND workspace_id IN (...) AND (repo_id = '' OR repo_id IN
   (...))` (PG: `= ANY($n)`) in shared `scope_filter_sql` helpers appended to
   the `search()`/`suggest()`/`facet_counts()` SQL before ORDER BY/LIMIT; empty
   accessible set compiles to `AND 0` / `AND FALSE`. No post-filtering path
   exists; the mutation probes prove the tests enforce this.
3. **Counts cannot leak.** `facet_counts` (both the response field and the
   `/search/facets` endpoint) and `suggest` take the same `SearchQuery` and the
   same WHERE fragments — adapter tests assert the inaccessible workspace is
   absent from facet counts, not just from rows.
4. **Admin unscoped within the tenant** — `accessible_workspace_ids: None`
   while the `tenant_id = ?` boundary stays.
5. **Indexing stamps real scope.** Task/agent/MR creation routes through
   `search_scope::index_doc`, which sets `workspace_id`/`repo_id` from the
   entity and resolves `tenant_id` from the workspace's real scope row
   (unresolvable → empty tenant → visible to no tenant-scoped query; no
   fabricated `"default"`).
6. **Hexagonal boundary** — scope derivation is entirely server-side; the port
   receives resolved id lists. `check-arch.sh` OK.
7. **Round-1 finding re-verified**: the repair is correct and every other
   runtime leg of the test was independently traced at source level (auth
   paths, ABAC builtin policies 900/800/700 allowing `read` on the `search`
   resource, `hash_api_key` ≡ the test's sha256, adapter/port signatures, the
   SQLite `workspace` facet synthesis, JWT claim names) — details in the
   evidence NOTES.md.

## Verifier-required probes

gyre-server test execution is wrapper-blocked in this sandbox (exit 64, "server
builds/tests belong to the independent verifier"; log retained). The verifier
must run:

1. `cargo test -p gyre-server --test search_wiring` — both tests, including the
   repaired `search_is_access_scoped_to_the_authenticated_caller`.
2. `cargo test -p gyre-server --lib mcp_search`.
3. `cargo test --all` and `bash scripts/check-arch.sh`.

## Minor observations (non-blocking)

- `total` in the search response is the (limit-truncated) page length, not the
  full match count; `facet_counts` describes the full filtered set. The
  acceptance criterion ("total/facet counts do not include them") is met —
  neither ever counts inaccessible docs.
- Human users receive all repos within their accessible workspaces
  (`accessible_repo_ids: None`) — matches the task plan (repo restriction is
  agent-scope only) and the platform model (human repo access = workspace
  membership).
- The mem adapter applies the scope predicate as a Rust filter inside the query
  function rather than SQL — it is the fallback/test backend, the predicate
  runs before any count is produced (no count leak), and parity is pinned by
  the mem tests plus the mutation probe.
- Generated `web/dist/` untouched by this candidate — no generated-output
  observations.
