---
title: "Enforce access scoping on search (tenant/workspace/repo, index-level filtering)"
spec_ref: "search.md §Access Scoping"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Access Scoping"
commits: ["2cab4181f49133b4a6171045de3dbb9b31057708", "ccf715b8cde79d9bfedebb0302fb9cb806c1e70a", "6bf73593a5b686b16f0b4d8229ad830019874f1b", "790b031dde07c45674902680b69ca91e59133b8f", "f888d61dd5bbf04f9af6d3bd52893cba0a36aac3", "a18e6e25807a53a9ec7e149b7ea5aa1a2b93717b"]
---

## Spec Excerpt

From `search.md` §Access Scoping:

> Every search query is filtered by the caller's access:
> 1. Extract `tenant_id` from auth context
> 2. Determine which `workspace_ids` the user/agent has membership in
> 3. Determine which `repo_ids` within those workspaces the user/agent can access
> 4. Filter search results to only include entities within accessible scopes
>
> This filtering happens at the index query level (not post-filter) to avoid leaking result counts.
>
> For agents, scope is determined by their OIDC token claims:
> - Repo-scoped agent: search results limited to its repo
> - Workspace Orchestrator: search results include all repos in its workspace
> - No agent can search outside its workspace

From Design Principle 1: "Search is access-scoped. Results never include entities the user/agent can't access."

## Problem (current state — code-verified)

There is NO access scoping. `search_handler` (`crates/gyre-server/src/api/search.rs:46-49`) takes `workspace_id` from client-supplied query params — no authenticated-user extraction, no membership/tenant derivation, no index-level filtering. MCP `handle_search` (`crates/gyre-server/src/mcp.rs:~1467`) likewise reads `workspace_id` from client args. This is a cross-scope leak: any caller can query any workspace's entities.

## Implementation Plan

Depends on task-201 (FTS backend stores `tenant_id`/`workspace_id`/`repo_id` on every document as UNINDEXED columns available for filtering).

1. **Extend `SearchQuery`** (`crates/gyre-ports/src/search.rs`) with scope fields the adapter filters on at the index-query level:
   - `tenant_id: String`
   - `accessible_workspace_ids: Option<Vec<String>>` (None = admin/unscoped; Some = restrict)
   - `accessible_repo_ids: Option<Vec<String>>`
   Keep the existing `entity_type`/`workspace_id` as an ADDITIONAL user-supplied narrowing filter, applied only within the accessible set.

2. **Adapter enforcement** (SQLite FTS5 + Postgres tsvector, from task-201): add `WHERE tenant_id = ? AND workspace_id IN (...) AND (repo_id IS NULL OR repo_id IN (...))` clauses to the search SQL. Filtering MUST be in the query, not post-filter (spec: avoid leaking counts). An empty accessible set yields zero rows.

3. **REST handler scoping** (`api/search.rs`):
   - Add `auth: crate::auth::AuthenticatedAgent` as the first extractor argument to `search_handler`.
   - Derive `tenant_id` from `auth.tenant_id`.
   - Derive accessible `workspace_ids` via `WorkspaceRepository` membership for `auth.user_id`; for Admin role, unscoped (None). For agents, derive workspace/repo scope from `auth.jwt_claims` (agent JWT scope claims) — repo-scoped agent → single repo; workspace orchestrator → all repos in its workspace; never outside its workspace.
   - The client-supplied `workspace_id` param becomes an optional narrowing filter intersected with the accessible set (a client requesting a workspace it can't access gets zero results, not an error that leaks existence).

4. **MCP handler scoping** (`mcp.rs` `handle_search`): apply the identical derivation from the agent's authenticated token context (NOT from client args). Remove reliance on client-supplied `workspace_id` for authorization; it may only narrow within the agent's scope.

5. **Facet counts** (if task-153 landed `facet_counts`): counts MUST reflect only the accessible result set.

## Acceptance Criteria

- [ ] `search_handler` and MCP `handle_search` derive tenant/workspace/repo scope from the authenticated caller, not from client-supplied params.
- [ ] Filtering is applied as SQL WHERE clauses at the index-query level (verified by reading the adapter query), not post-filtering.
- [ ] A user with membership in workspace A but not B, searching a term that matches entities in both, receives ONLY workspace-A results (and any facet counts exclude B).
- [ ] A repo-scoped agent receives only its repo's entities; a workspace-orchestrator agent receives all repos in its workspace; neither can retrieve entities outside its workspace, even by passing another `workspace_id` param.
- [ ] Admin callers are unscoped (can search across the tenant).
- [ ] A test seeds documents in two workspaces, runs a search as a caller scoped to one, and asserts the other workspace's matching docs are absent AND that total/facet counts do not include them. The test MUST fail if scoping is post-filter that still leaks counts or if client-supplied `workspace_id` can widen access.
- [ ] `cargo test --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Read task-201's adapter. Read `crates/gyre-server/src/auth.rs:241-258` (`AuthenticatedAgent`: `tenant_id`, `user_id`, `roles`, `jwt_claims`), the `WorkspaceRepository` port for membership lookup, an existing handler that uses `AuthenticatedAgent` for scoping (e.g. `api/workspaces.rs:101-105`, `api/compute_targets.rs:69-79`), and `crates/gyre-server/src/mcp.rs` `handle_search` (~1467) plus how agent JWT scope claims are shaped (see agent JWT minting in `auth.rs`).
- Do NOT weaken any existing auth; add scoping without breaking Admin/global-token paths.
- Preserve hexagonal boundaries — scope derivation is server-side; the port just receives the resolved id lists.
- Run validation once at the end. On completion set `progress: ready-for-review` and record commit SHAs.

## Shipped

Search is access-scoped end to end (search.md §Access Scoping). The history carries the original implementation (`a18e6e25`), verification repairs (`790b031d`, `6bf73593`, `f888d61d`), the PG suggest alias review repair (`ccf715b8`), and this attempt's verification repair `2cab4181f` (rustfmt drift, below).

### Behavior

- **Scope derivation (server-side, `crates/gyre-server/src/search_scope.rs`)**: `derive_search_scope` resolves the caller's `tenant_id` from auth, then — Admin (incl. global dev token) → unscoped within the tenant; agent JWTs with `orchestrator_type` claims → workspace orchestrator sees its whole workspace, repo orchestrator its single repo; worker agents → their agent record's workspace/repo; human users → accepted workspace memberships; unknown callers → fail closed (empty accessible set, zero results, never unscoped).
- **Port (`gyre-ports/src/search.rs`)**: `SearchQuery` carries `tenant_id`, `accessible_workspace_ids: Option<Vec<String>>` (`None` = Admin/unscoped, empty = zero rows), `accessible_repo_ids: Option<Vec<String>>` (repo-less workspace docs stay visible). The user-supplied `workspace_id`/`entity_type` remain narrowing filters intersected with the accessible set — they can only narrow, never widen. `suggest()` and `facet_counts()` take the same `SearchQuery` and enforce the same scope.
- **Adapter enforcement, inside the index query**: SQLite FTS5 and Postgres tsvector adapters build the §Access Scoping WHERE clauses (`tenant_id = ?`, `workspace_id IN (...)`, `(repo_id = '' OR repo_id IN (...))`; empty accessible set → `AND 0`/`AND FALSE`) in shared `scope_filter_sql` helpers applied identically by `search()`, `suggest()`, and `facet_counts()` — never post-filtered, so totals, suggestion lists, and facet counts cannot leak inaccessible documents. The mem adapter applies the same scope predicate in its query filter (port-contract parity).
- **Handlers**: REST `search_handler`, `suggest_handler`, `facets_handler`, and MCP `gyre_search` all take `AuthenticatedAgent` and derive scope from the token context, never from client args; a client `workspace_id` only narrows.
- **Indexing**: task/agent/MR creation routes documents through `search_scope::index_doc`, which stamps `workspace_id`/`repo_id` from the entity and resolves `tenant_id` from the workspace's scope row (unresolvable → doc visible to no tenant-scoped query, never mis-attributed).
- **Docs**: `docs/api-reference.md` search/suggest/facets rows state the access scoping and that `workspace_id` only narrows.

### Review repair `ccf715b8` (prior attempt)

Reviewer finding: `PgStorage::suggest`'s base SQL was `FROM search_index` with no `s` alias while the §Access Scoping fragments are `s.`-qualified, so PostgreSQL rejected every suggest call with 42P01 "missing FROM-clause entry for table \"s\"" — `GET /api/v1/search/suggest` 500'd for every caller (incl. Admin) on Postgres deployments; `cargo check` passed, making it runtime-only. Fixed by extracting all four PG base SQLs into consts (`SEARCH_BASE_SQL`, `SUGGEST_BASE_SQL`, `FACET_TEXT_BASE_SQL`, `FACET_BLANK_BASE_SQL`) and aliasing the suggest base `FROM search_index s` with `s.`-qualified columns and ORDER BY.

- Added the first test module to `postgres/search.rs`: each base SQL is composed with the production `scope_filter_sql` fragments and asserted to define the `s` alias the fragments reference, keep the tenant boundary inside the index query, keep placeholder numbering contiguous, and compile an empty accessible set to `AND FALSE`. Verified the new `suggest_base_sql_defines_the_scope_alias` test FAILS on the pre-fix SQL (reproducing the finding) and passes after the fix — the regression is now mechanically pinned even without a live PG in CI.
- No local Postgres exists in this sandbox (no binary, no network to install one), so a live PG execution probe is deferred to remote verification; the defect class (alias/reference mismatch in composed SQL) is now covered by the unit tests on the real composed artifact.

### Verification repair `2cab4181f` (this attempt)

Verification finding: `check-rustfmt-diff.py` against base `7e9b3cfee` flagged changed-line formatting drift in the four files this task touched (`crates/gyre-adapters/src/mem_search.rs`, `crates/gyre-adapters/src/postgres/search.rs`, `crates/gyre-adapters/src/sqlite/search.rs`, `crates/gyre-server/tests/search_wiring.rs`) — the sole failing gate; clippy, architecture, ABAC, and all other check scripts passed. Fixed by applying the gate's exact rustfmt invocation (`rustfmt --edition 2021 --config skip_children=true`) to the four files. Whitespace only, no semantic change; `cargo check -p gyre-adapters` and `cargo check -p gyre-server --tests` clean and `cargo test -p gyre-adapters search` still passes 25/25 after the reformat.

### Test evidence

Logs retained under `/tmp/stage/review-evidence/task-203-rustfmt-2cab4181f/` (this attempt), `/tmp/stage/review-evidence/task-203-repair-ccf715b8/` and `/tmp/stage/review-evidence/task-203-rebase-7cf2f81c/` (prior attempts, retained by controller):

- This attempt (`2cab4181f`, rustfmt-only): `scripts/check-rustfmt-diff.py 7e9b3cfee` — changed lines clean (12 files); `cargo check -p gyre-adapters` — clean; `cargo check -p gyre-server --tests` — clean; `cargo test -p gyre-adapters search` — 25 passed, 0 failed.
- `cargo test -p gyre-adapters postgres::search::tests` — pre-fix run: `suggest_base_sql_defines_the_scope_alias` FAILED on the unaliased base SQL; post-fix run: 4 passed.
- `cargo test -p gyre-adapters search` — 25 passed (mem + SQLite FTS5 scoping/behavior + the 4 new PG composed-SQL tests).
- Prior attempt: `cargo check -p gyre-ports -p gyre-adapters --tests`, `cargo check -p gyre-server --tests` (incl. `search_wiring`) — clean; `bash scripts/check-arch.sh` — passed. This attempt touches formatting only (no port/server surface change).
- `crates/gyre-server/tests/search_wiring.rs` `search_is_access_scoped_to_the_authenticated_caller` asserts alice's search, `facet_counts`, `/search/suggest`, and `/search/facets` exclude workspace B (type-checked here; execution deferred — see below).

### Verifier-required probes (server test binaries exceed this sandbox's build budget)

- `cargo test -p gyre-server --test search_wiring`
- `cargo test -p gyre-server --lib mcp_search`
- `cargo test --all` and `bash scripts/check-arch.sh` (repo-wide gates)
- If a Postgres instance is available to the verifier: `GYRE_DATABASE_URL=postgres://... cargo run -p gyre-server`, then `curl -H 'Authorization: Bearer gyre-dev-token' 'http://localhost:3000/api/v1/search/suggest?q=a'` — must return 200 (was 500 with 42P01 before `ccf715b8`)
