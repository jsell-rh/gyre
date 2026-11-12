---
title: "Enforce access scoping on search (tenant/workspace/repo, index-level filtering)"
spec_ref: "search.md §Access Scoping"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Access Scoping"
commits: ["dedfeb4fff806a40c611b09af7214ddd1bd5574f", "2cab4181f49133b4a6171045de3dbb9b31057708", "ccf715b8cde79d9bfedebb0302fb9cb806c1e70a", "6bf73593a5b686b16f0b4d8229ad830019874f1b", "790b031dde07c45674902680b69ca91e59133b8f", "f888d61dd5bbf04f9af6d3bd52893cba0a36aac3", "a18e6e25807a53a9ec7e149b7ea5aa1a2b93717b"]
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

Search is access-scoped end to end (search.md §Access Scoping). The history carries the original implementation (`a18e6e25`, `f888d61d`), review/verification repairs (`790b031d`, `6bf73593`, `ccf715b8`, `2cab4181f`), and this attempt's review repair `dedfeb4ff` (test-only tenant binding, below).

### Behavior

- **Scope derivation (server-side, `crates/gyre-server/src/search_scope.rs`)**: `derive_search_scope` resolves the caller's `tenant_id` from auth, then — Admin (incl. global dev token) → unscoped within the tenant; agent JWTs with `orchestrator_type` claims → workspace orchestrator sees its whole workspace, repo orchestrator its single repo; worker agents → their agent record's workspace/repo; human users → accepted workspace memberships; unknown callers → fail closed (empty accessible set, zero results, never unscoped).
- **Port (`gyre-ports/src/search.rs`)**: `SearchQuery` carries `tenant_id`, `accessible_workspace_ids: Option<Vec<String>>` (`None` = Admin/unscoped, empty = zero rows), `accessible_repo_ids: Option<Vec<String>>` (repo-less workspace docs stay visible). The user-supplied `workspace_id`/`entity_type` remain narrowing filters intersected with the accessible set — they can only narrow, never widen. `suggest()` and `facet_counts()` take the same `SearchQuery` and enforce the same scope.
- **Adapter enforcement, inside the index query**: SQLite FTS5 and Postgres tsvector adapters build the §Access Scoping WHERE clauses (`tenant_id = ?`, `workspace_id IN (...)`, `(repo_id = '' OR repo_id IN (...))`; empty accessible set → `AND 0`/`AND FALSE`) in shared `scope_filter_sql` helpers applied identically by `search()`, `suggest()`, and `facet_counts()` — never post-filtered, so totals, suggestion lists, and facet counts cannot leak inaccessible documents. The mem adapter applies the same scope predicate in its query filter (port-contract parity).
- **Handlers**: REST `search_handler`, `suggest_handler`, `facets_handler`, and MCP `gyre_search` all take `AuthenticatedAgent` and derive scope from the token context, never from client args; a client `workspace_id` only narrows.
- **Indexing**: task/agent/MR creation routes documents through `search_scope::index_doc`, which stamps `workspace_id`/`repo_id` from the entity and resolves `tenant_id` from the workspace's scope row (unresolvable → doc visible to no tenant-scoped query, never mis-attributed).
- **Docs**: `docs/api-reference.md` search/suggest/facets rows state the access scoping and that `workspace_id` only narrows.

### Review repair `dedfeb4ff` (this attempt)

Reviewer finding: the acceptance-criterion test `search_is_access_scoped_to_the_authenticated_caller` failed at runtime — the seeded API-key user `alice` was created without a tenant binding, and API-key auth fail-closes those users with 403 (task-099 F1), so alice's first scoped search panicked the test's status assertion. Fixed by binding alice to tenant `"default"` (the tenant the seeded workspaces live in), mirroring `auth_integration.rs`. Test-only change; no production code touched.

Because the stage build wrapper still blocks gyre-server test execution in this sandbox (infrastructure restriction, recorded below), every other leg of that test was statically traced against the implementation to preclude further runtime-only failures: the global dev token's tenant is `system_principal_tenant() == "default"` (matches the seeded workspaces' tenant); agent JWTs authenticate via the `agent_tokens` KV lookup without requiring an agent record (so the record-less orchestrators and the ghost agent reach the handler, and the ghost fails closed at scope derivation with zero results, not a 403); `mint_orchestrator` claims (`scope: "agent"`, `orchestrator_type`, `workspace_id`, `repo_id`) match what `derive_search_scope` reads; ABAC builtin policies allow Admin (900) / Developer (800) / Agent (700) read on the `search` resource; the SQLite adapter's `count_facet_row` synthesizes the `workspace` facet from the row's `workspace_id`, so the asserted `facet_counts["workspace"]` values materialize from docs seeded with empty facet maps; rate limit (100 req/s per state) cannot trip at the test's request count.

### Test evidence

Evidence for this attempt under `/tmp/stage/review-evidence/task-203-repair-891273691d/` (controller artifact path):

- `cargo check -p gyre-server --tests` — clean (includes `search_wiring` with the tenant-binding fix; log `cargo-check-gyre-server-tests.txt`).
- `python3 scripts/check-rustfmt-diff.py d31906c5a16ab8045ab435dfad2bd7dc1ce3f616` — changed lines clean, 12 Rust files (log `rustfmt-diff-gate.txt`).
- `cargo test -p gyre-adapters search` — 25 passed (mem + SQLite FTS5 scoping/behavior + PG composed-SQL tests; prior attempts, unchanged by this repair — this attempt touches only a gyre-server test file).
- `crates/gyre-server/tests/search_wiring.rs` `sqlite_url_wires_fts5_search_port` — 1 passed (verified at task-201 time; proves the sqlite wiring, middleware stack, FTS5 behavior, and admin HTTP search path this attempt's fix relies on).

### Verifier-required probes (gyre-server test execution exceeds this sandbox's build budget; blocked by the stage build wrapper at implement stage, exit 64 — infrastructure restriction, not a code defect)

- `cargo test -p gyre-server --test search_wiring` — must pass both tests, including the repaired `search_is_access_scoped_to_the_authenticated_caller`.
- `cargo test -p gyre-server --lib mcp_search`
- `cargo test --all` and `bash scripts/check-arch.sh` (repo-wide gates)
- If a Postgres instance is available: `GYRE_DATABASE_URL=postgres://... cargo run -p gyre-server`, then `curl -H 'Authorization: Bearer gyre-dev-token' 'http://localhost:3000/api/v1/search/suggest?q=a'` — must return 200 (was 500 with 42P01 before `ccf715b8`)
