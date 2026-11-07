---
title: "Enforce access scoping on search (tenant/workspace/repo, index-level filtering)"
spec_ref: "search.md §Access Scoping"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Access Scoping"
commits: ["790b031dde07c45674902680b69ca91e59133b8f", "f888d61dd5bbf04f9af6d3bd52893cba0a36aac3", "a18e6e25807a53a9ec7e149b7ea5aa1a2b93717b", "6bf73593ad04e5a956c67204389a9fa2a9ad31e5"]
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

Search is access-scoped end to end (search.md §Access Scoping), continuing the interrupted checkpoint `a18e6e25` on the merged tree, plus the verification repair `790b031d`.

### Behavior

- **Scope derivation (server-side, `crates/gyre-server/src/search_scope.rs`)**: `derive_search_scope` resolves the caller's `tenant_id` from auth, then — Admin (incl. global dev token) → unscoped within the tenant; agent JWTs with `orchestrator_type` claims → workspace orchestrator sees its whole workspace, repo orchestrator its single repo; worker agents → their agent record's workspace/repo; human users → accepted workspace memberships; unknown callers → fail closed (empty accessible set, zero results, never unscoped).
- **Port (`gyre-ports/src/search.rs`)**: `SearchQuery` carries `tenant_id`, `accessible_workspace_ids: Option<Vec<String>>` (`None` = Admin/unscoped, empty = zero rows), `accessible_repo_ids: Option<Vec<String>>` (repo-less workspace docs stay visible). The user-supplied `workspace_id`/`entity_type` remain narrowing filters intersected with the accessible set — they can only narrow, never widen.
- **Adapter enforcement, inside the index query**: SQLite FTS5 and Postgres tsvector adapters bind `tenant_id = ?`, `workspace_id IN (...)`, `(repo_id = '' OR repo_id IN (...))` as SQL WHERE clauses on the FTS/tsquery itself (empty accessible set → `AND 0`); the mem adapter applies the same constraints in its query filter before scoring. No post-filtering anywhere, so totals/facet counts cannot leak inaccessible documents.
- **Handlers**: REST `search_handler` and MCP `gyre_search` take `AuthenticatedAgent` first and derive scope from the token context, never from client args.
- **Indexing**: task/agent/MR creation routes documents through `search_scope::index_doc`, which stamps `workspace_id`/`repo_id` from the entity and resolves `tenant_id` from the workspace's scope row (unresolvable → doc visible to no tenant-scoped query, never mis-attributed).
- **Docs**: `docs/api-reference.md` search rows now state the access scoping and that `workspace_id` only narrows.

### Test evidence

Fresh sandbox, `SKIP_WEB_BUILD=1` (committed `web/dist`; no web code touched), via `/tmp/stage/bin/cargo`; logs retained in `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-server --lib mcp_search` — 1 passed: `mcp_search_is_access_scoped_to_the_agent_token` (admin sees 3; repo orchestrator sees only its repo; client `workspace_id: ws-b` cannot widen; workspace orchestrator sees both ws-a repos, never ws-b).
- `cargo test -p gyre-server --test search_wiring` — 2 passed: `search_is_access_scoped_to_the_authenticated_caller` (member of ws-a gets total 2 with ws-b's matching doc absent from results AND total; `workspace_id=ws-b` param → total 0; repo/workspace orchestrator and worker-agent paths; ghost agent fails closed) and `sqlite_url_wires_fts5_search_port` (FTS5 wiring, stemming, tenant boundary, facets).
- `cargo test -p gyre-adapters search` — 16 passed, incl. `access_scoping_filters_inside_the_index_query` (asserts the SQL text carries the WHERE clauses), `access_scoping_filters_inside_the_query_filter` (mem), `repo_scope_restricts_repos_but_keeps_workspace_level_docs`.
- `bash scripts/check-arch.sh`, `check-mem-port-contracts.sh`, `check-abac-route-registry.sh` — all exit 0.

Full workspace suites, all-target Clippy, and GitHub CI are deferred to verification per assignment scope.

### Verification repair (`790b031d`)

Durable finding `3f2e4f62`: the verification sandbox failed to compile the `search_wiring` test binary — `E0015: cannot call non-const associated function tokio::sync::Mutex::<()>::new in statics` — because `static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(())` depends on `Mutex::new` being `const fn`, which does not hold on the verification toolchain's tokio build. Fixed by initializing the env-var lock through `std::sync::LazyLock` (the pattern already used by `crates/gyre-adapters/src/call_graph.rs`), so no const call sits in the static initializer; test logic unchanged. Everything else in that verification run passed (arch lints, ABAC registry, mem port contracts, web build); the svelte a11y/unused-CSS output in the log tail is warnings-only. Post-fix probes in this sandbox, `SKIP_WEB_BUILD=1` via `/tmp/stage/bin/cargo` (notes in `/tmp/stage/review-evidence/task-203-e0015-repair.md`): `cargo test -p gyre-server --test search_wiring` — 2 passed; `cargo test -p gyre-server --lib mcp_search` — 1 passed; `rustfmt --check` on the changed file — clean.

### Verification repair (`6bf73593`)

Durable finding `82706acf`: the verification checks run (`bash /tmp/stage/checks.sh`, candidate `4c1f2f62`) exited 1 with a single actionable code diagnostic — `crates/gyre-server/tests/search_wiring.rs:275: unused_must_use: unused std::result::Result that must be used`. Root cause: `abac_middleware::seed_builtin_policies(&state).await;` drops its `anyhow::Result<()>` at both call sites in the test (lines 200 and 275 of the candidate); rustc's `unused_must_use` fires on the dropped `#[must_use]` result. Everything else in that run passed (rustfmt clean, build OK, web build succeeded; the svelte a11y/unused-CSS output is warnings-only and pre-existing). Fix: both call sites now `.unwrap()`, matching every other fallible call in the file — no production code, test logic, or assertions changed. Post-fix probes in this sandbox, `SKIP_WEB_BUILD=1` via `/tmp/stage/bin/cargo` (notes and logs in `/tmp/stage/review-evidence/task-203-unused-must-use-repair.md`): `unused_must_use` reproduced pre-fix at :200 and :275, post-fix `cargo clippy -p gyre-server --all-targets` reports zero `unused_must_use`; `cargo test -p gyre-server --test search_wiring` — 2 passed; `cargo test -p gyre-server --lib mcp_search` — 1 passed; `rustfmt --edition 2024 --check` — clean.
