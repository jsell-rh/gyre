# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping — every search query filtered by the caller's access (tenant from auth, workspace memberships, repo access), filtering at the index-query level (not post-filter) so counts don't leak; agents scoped by OIDC claims (repo orchestrator → its repo, workspace orchestrator → its workspace, never outside); Design Principle 1: results never include inaccessible entities.

Candidate: `a06a88073` (base `ceefc14a`). Product-surface commits per frontmatter: `a18e6e25` (implementation), `790b031d`, `6bf73593`, `f888d61d` (verification repairs), `ccf715b8` (review repair for the PG suggest alias defect found in the prior round).

Verdict: **approved** — no findings.

## What was verified

- **Port contract** (`crates/gyre-ports/src/search.rs`): `SearchQuery` now carries `tenant_id: String`, `accessible_workspace_ids: Option<Vec<String>>` (`None` = Admin/unscoped, empty = zero rows), `accessible_repo_ids: Option<Vec<String>>` (repo-less workspace docs stay visible). `suggest()` and `facet_counts()` take the same scoped `SearchQuery` (facet signature changed from `Option<SearchQuery>` — all call sites updated; grep shows no unscoped construction site left in the server). Doc comments state the index-level, never-post-filter obligation.
- **Adapter enforcement, inside the index query**: SQLite FTS5 `scope_filter_sql` emits `AND workspace_id IN (...)`, `AND (repo_id = '' OR repo_id IN (...))`, unconditional `AND tenant_id = ?`, and `AND 0` for an empty accessible set — appended to the boxed raw query of `search()`, `suggest()`, and `facet_counts()` alike, so results, suggestion lists, and facet counts cannot leak. Postgres analog uses `$N` placeholders (numbering advanced through a `&mut usize`), `= ANY(...)`, and `AND FALSE`. The mem adapter applies the identical predicate (`scope_visible`) inside its query filter — port-contract parity. Index-time tenant derivation (SQLite/PG `index()`, server `search_scope::index_doc`) resolves the tenant from the workspace's real scope row; an unresolvable workspace leaves the tenant blank/None → invisible to every tenant-scoped query (fail closed, never a fabricated "default").
- **Server-side scope derivation** (`crates/gyre-server/src/search_scope.rs`): Admin (incl. global dev token) → unscoped within tenant; agent JWT with `orchestrator_type` claims → workspace orchestrator sees its whole workspace, repo orchestrator its single repo; worker agents → their agent record's workspace/repo; human users → accepted workspace memberships only; unknown caller → empty accessible set (fail closed). Client-supplied `workspace_id` remains a narrowing filter the adapters intersect with the accessible set — verified by tests asserting zero results (not an error) when requesting an inaccessible workspace.
- **Handlers**: REST `search_handler`, `suggest_handler`, `facets_handler` and MCP `gyre_search` all take `AuthenticatedAgent` first and derive scope from token context, never client args. No existing auth weakened: routes unchanged, `check-abac-route-registry` and `check-mcp-write-tools` pass (gyre_search is read-only, correctly outside the `needs_write` gate).
- **Prior-round blocking defect fixed** (`ccf715b8`): PG `suggest()`'s base SQL previously had no `s` alias while the scope fragments are `s.`-qualified → SQLSTATE 42P01 on every suggest call (runtime-only; cargo check passed). Now all four PG base SQLs are extracted consts with `FROM search_index s`, and a new PG test module composes the production `scope_filter_sql` fragments and asserts the alias, in-query tenant boundary, contiguous placeholder numbering, and `AND FALSE` on empty accessible sets. The prior round verified `suggest_base_sql_defines_the_scope_alias` FAILS on the pre-fix SQL; this round confirms it passes on the candidate (see Probe 1). No live PG exists in this sandbox — a runtime probe is recorded for the verifier.
- **Tests are anchored, not self-confirming**: mutation probe — disabling `scope_visible` in the mem adapter makes `access_scoping_filters_inside_the_query_filter` (and the facet/entity-filter tests) FAIL; restored byte-identical and re-run green. The wiring test (`crates/gyre-server/tests/search_wiring.rs`, `search_is_access_scoped_to_the_authenticated_caller`) seeds docs in two workspaces + two repos and asserts, through the full router middleware stack: alice (member of ws-a only) gets total 2 / no ws-b results / facet counts exclude ws-b; `workspace_id=ws-b` param → 0; repo orchestrator → only its repo; workspace orchestrator → both ws-a repos, never ws-b; worker agent → its record's workspace; agent with no record → fail closed. MCP lib test `mcp_search_is_access_scoped_to_the_agent_token` mirrors this over JSON-RPC with minted orchestrator JWTs, including the no-widening case. Both type-check clean (`cargo check -p gyre-server --tests`); execution deferred to the verifier per the sandbox build budget.

## Probes run (evidence: /tmp/stage/review-evidence/task-203-final/probes.md)

- `cargo test -p gyre-adapters search` → **25 passed, 0 failed** (mem + SQLite FTS5 scoping + 4 PG composed-SQL tests).
- Mutation probe (mem `scope_visible` disabled) → scoping test **FAILED**; restored → 8 passed.
- `cargo check -p gyre-server --tests` → clean.
- `check-arch.sh`, `check-mem-port-contracts.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-forged-scope-fields.sh` → all pass.
- `check-task-commit-attribution.sh` fails only against the controller's staged working-tree reset of `specs/tasks/task-203.md` to `commits: []`; the candidate's committed frontmatter lists all five product-surface commits — attribution correct at the commit.

## Verifier-required probes (gyre-server test binaries exceed this sandbox's 4 GiB build budget)

- `cargo test -p gyre-server --test search_wiring`
- `cargo test -p gyre-server --lib mcp_search`
- `cargo test --all` and `bash scripts/check-arch.sh`
- If a live PostgreSQL is available: run the server against it and GET the suggest endpoint with the global dev token — must return 200 (was 500 / 42P01 before `ccf715b8`).

Findings: none.
