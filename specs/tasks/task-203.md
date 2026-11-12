---
title: "Enforce access scoping on search (tenant/workspace/repo, index-level filtering)"
spec_ref: "search.md §Access Scoping"
depends_on: [task-201]
progress: not-started
coverage_sections:
  - "search.md §Access Scoping"
commits: []
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
