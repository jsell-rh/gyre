# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping — every search query filtered by
the caller's access (tenant from auth context, workspace memberships, repo
access), filtering at the index-query level (not post-filter) to avoid leaking
result counts; agent scope from OIDC claims (repo-scoped agent → its repo,
workspace orchestrator → all repos in its workspace, never outside).
Candidate under review: `49dec847` on base `7cf2f81c` (branch
`pipeline/task-203/b90b2487df764e28a92792c8ffaca931-1`).
Verdict: **changes requested** (one blocking code finding; the REST/SQLite/mem
scoping itself is real, index-level, and well-tested).

## Round 1 — independent review

Probe log with commands, exit codes, and outputs:
`/tmp/stage/review-evidence/review-summary.md` (plus per-probe `.txt` files).

- `bash scripts/check-arch.sh` → OK (scope derivation is server-side; the port
  receives resolved id lists — hexagonal boundary intact).
- `/tmp/stage/bin/cargo test -p gyre-adapters search` → **21 passed, 0 failed**
  (mem + SQLite FTS5, including the new
  `access_scoping_filters_inside_the_index_query`,
  `repo_scope_restricts_repos_but_keeps_workspace_level_docs`, and mem-parity
  `access_scoping_filters_inside_the_query_filter`).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → clean, including
  `tests/search_wiring.rs` and the new MCP test
  `mcp_search_is_access_scoped_to_the_agent_token` (execution deferred to the
  verifier per the sandbox build budget).
- **Mutation probe** (source restored byte-identical afterwards, suite re-run
  green): disabling the `accessible_workspace_ids` check in
  `mem_search.rs::scope_visible` makes
  `access_scoping_filters_inside_the_query_filter` **FAIL** — the new tests
  are genuine kill-tests, not self-confirming.
- Mechanical invariants: `check-abac-route-registry`, `check-abac-exempt-handlers`
  (75 handlers), `check-api-auth`, `check-fabricated-scope-defaults`,
  `check-scope-literal-defaults`, `check-forged-scope-fields`,
  `check-inert-enforcement`, `check-mem-port-contracts`,
  `check-conditional-test-guards` → all OK. (`check-assertionless-tests` exits
  2 in this sandbox, but it reproduces identically at the base commit — an
  mawk-vs-gawk environment quirk in the script's 3-arg `match()`, not a
  candidate defect.)

### Verified working (no findings)

- **Scope derivation** (`crates/gyre-server/src/search_scope.rs`) is
  server-side and fail-closed: Admin (incl. global dev token) → unscoped
  within the tenant; agent-JWT orchestrator claims (`orchestrator_type` /
  `workspace_id` / `repo_id`, minted snake_case per `AgentJwtClaims` and
  matching the claim lookups) → workspace/repo scope; worker agents → their
  agent record's workspace/repo; human users → accepted workspace
  memberships; membership-lookup failure or an unknown agent → empty
  accessible set (zero results, never unscoped). No path derives scope from
  client params.
- **Client `workspace_id`/`entity_type` only narrow**: in all three adapters
  they are additional WHERE clauses ANDed with the accessible-set clauses; an
  inaccessible workspace yields zero rows (`AND 0` / `AND FALSE` / predicate
  false), not an error — matching the plan's no-existence-leak requirement.
- **Index-level filtering, not post-filter**: SQLite FTS5 `search()`,
  `suggest()`, and `facet_counts()` append the shared `scope_filter_sql`
  fragments to the boxed SQL before ORDER BY/LIMIT; the mem adapter applies
  the same `scope_visible` predicate inside the query filter before
  scoring/counting. Totals, suggestion lists, and facet counts therefore
  cannot leak inaccessible documents, in both facet shapes (text-filtered and
  blank-text whole-accessible-set; `/search/facets` no longer aggregates the
  whole index for scoped callers).
- **MCP `gyre_search`** now takes `&AuthenticatedAgent` and calls the identical
  `derive_search_scope`; the client `workspace_id` arg survives only as a
  narrowing filter.
- **Indexing paths** (`api/tasks.rs`, `api/agents.rs`, `api/merge_requests.rs`)
  route through `search_scope::index_doc`, stamping workspace/repo from the
  entity and resolving tenant from the workspace's real scope row; an
  unresolvable workspace leaves the doc visible to no tenant-scoped query —
  no fabricated "default" scope.
- **Tests are hard and spec-shaped**: two-workspace seeding with total/facet/
  suggest exclusion assertions, cross-tenant exclusion, repo-vs-workspace-level
  doc split, client-widening attempts, and a fail-closed ghost agent; the
  mutation probe confirms the enforcement is what the tests anchor to.

### Findings

- **F1 (blocking, code)** — `crates/gyre-adapters/src/postgres/search.rs`:
  `PgStorage::suggest()` builds its base SQL as
  `SELECT entity_type, entity_id, title FROM search_index WHERE title ILIKE $1`
  **without a table alias**, then appends the §Access Scoping fragments from
  `scope_filter_sql`, which are all `s.`-qualified and include an
  **unconditional** `AND s.tenant_id = $n` (line 219). The assembled SQL
  references alias `s` that is never defined, so PostgreSQL rejects every
  suggest call: `ERROR: missing FROM-clause entry for table "s"` (SQLSTATE
  42P01). Reachable in production — `build_state`'s `store!` macro wires
  `state.search` to `PgStorage` for `postgres://` URLs and `suggest_handler`
  calls it for `GET /api/v1/search/suggest`, so the endpoint returns 500 for
  every caller (including Admin) on Postgres deployments. Regression
  introduced by this candidate (the base commit's suggest had no
  `s.`-qualified fragments); `search()` and `facet_counts()` alias the table
  correctly, only `suggest()` was missed, and `postgres/search.rs` has no test
  module. `cargo check` cannot see it (diesel `.sql()` appends verbatim).
  Reproduce: run against a Postgres `GYRE_DATABASE_URL` and
  `curl -H "Authorization: Bearer gyre-dev-token"
  "http://localhost:3000/api/v1/search/suggest?q=a"` → 500 with the 42P01
  error in the log (the base commit returns 200). Fix is one line:
  `FROM search_index s`. Full write-up with source excerpts:
  `/tmp/stage/review-evidence/pg-suggest-defect.md`.

### Deferred to the verifier

- `cargo test -p gyre-server --test search_wiring`
- `cargo test -p gyre-server --lib mcp_search`
- `cargo test --all` + repo-wide gates (sandbox build budget)
- After F1 is fixed: exercise `PgStorage::suggest`/`search`/`facet_counts`
  against a real PostgreSQL — this defect class is invisible to `cargo check`.

### Non-blocking observations

- Agent tokens are pinned to tenant `default` (`system_principal_tenant`,
  `auth.rs`) — a pre-existing identity convention, not introduced here. On
  multi-tenant deployments where an agent's workspace belongs to another
  tenant, agent search fails closed (zero results, no leak).
- Generated `web/dist/` is untouched by this diff; nothing to observe.
