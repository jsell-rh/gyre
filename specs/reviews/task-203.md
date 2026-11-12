# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping.
Comparison base: `7e9b3cfeea8598e05c8b0ae971e06f214dd21806` → candidate `694508dd4bce053ac7fd5578ebc063ff69a014e5`.
Diff: 14 files — ports (`gyre-ports/src/search.rs`), all three adapters (sqlite/postgres/mem search), server handlers (`api/search.rs`, `mcp.rs`, new `search_scope.rs`), indexing callsites (`api/agents.rs`, `api/merge_requests.rs`, `api/tasks.rs`), integration tests (`tests/search_wiring.rs`), docs, task file.

Verdict: **changes-requested** (one blocking finding; the production scoping implementation itself is sound).

## What is real and verified

- **Port contract** (`gyre-ports/src/search.rs`): `SearchQuery` carries `tenant_id: String`, `accessible_workspace_ids: Option<Vec<String>>`, `accessible_repo_ids: Option<Vec<String>>` (None = unscoped/Admin, empty = zero rows); `suggest()` and `facet_counts()` take the same query so all three read paths enforce the identical scope. Client-supplied `workspace_id`/`entity_type` remain narrowing filters intersected with the accessible set.
- **Adapter enforcement is in the index query, not post-filter.** SQLite FTS5 (`scope_filter_sql` → `AND tenant_id = ? AND workspace_id IN (...) AND (repo_id = '' OR repo_id IN (...))`, empty set → `AND 0`) appended to `search()`/`suggest()`/`facet_counts()` SQL before ORDER BY/LIMIT; Postgres tsvector equivalents with `= ANY($n)` and `AND FALSE`; mem adapter applies the same `scope_visible` predicate inside the query filter. Tenant is resolved at index time from the workspace's real scope row (unresolvable → empty tenant → visible to no tenant-scoped query, never a fabricated `"default"`).
- **Scope derivation is server-side** (`gyre-server/src/search_scope.rs::derive_search_scope`): Admin (incl. global dev token) → unscoped within tenant; agent JWT `orchestrator_type` claims → workspace orchestrator sees its whole workspace, repo orchestrator its single repo; worker agents → their agent record's workspace/repo; humans → accepted workspace memberships; unknown callers → fail closed (empty accessible set). REST `search_handler`/`suggest_handler`/`facets_handler` and MCP `gyre_search` all route through it; the client `workspace_id` can only narrow (verified in adapter SQL as an `AND workspace_id = ?` on top of the accessible-set `IN`). Hexagonal boundary respected — the port receives resolved id lists only.
- **Executed probe**: `cargo test -p gyre-adapters search` (via the stage build wrapper) → **25 passed, 0 failed, exit 0** — including `sqlite::search::tests::access_scoping_filters_inside_the_index_query`, `repo_scope_restricts_repos_but_keeps_workspace_level_docs`, `mem_search::tests::access_scoping_filters_inside_the_query_filter`, and the four PG composed-SQL alias/numbering tests. The adapter tests assert facet-count exclusion of the inaccessible workspace (the count-leak kill condition), not just row exclusion.
- **Type-check probe**: `cargo check -p gyre-server --tests` → clean, exit 0 (includes `tests/search_wiring.rs` and the new MCP test).

Evidence: `/tmp/stage/review-evidence/task-203-candidate-694508d/` (logs + NOTES.md with the full defect chain).

## Blocking finding — the human-user acceptance test fails at runtime

`crates/gyre-server/tests/search_wiring.rs::search_is_access_scoped_to_the_authenticated_caller` — the only test covering the acceptance criterion "a user with membership in workspace A but not B ... receives ONLY workspace-A results (and facet counts exclude B)" — **fails on its first alice assertion**:

1. `search_wiring.rs:412-414`: `User::new(...)` never gets a `tenant_id` (domain default `None`, `gyre-domain/src/user.rs:205`).
2. Test runs under `GYRE_DATABASE_URL=sqlite://…`; `users.tenant_id` is nullable with no default (migration 000058), and the sqlite adapter round-trips NULL → `None` with no backfill.
3. `auth.rs:1137-1160` (API-key path, task-099 F1): a user without a tenant binding is rejected **403 FORBIDDEN** ("user has no tenant binding; re-provision via POST /api/v1/users") — the documented fail-close that `tests/auth_integration.rs:62-76` explicitly works around by setting `user.tenant_id = Some(...)`.
4. `search_wiring.rs:447-448`: alice's `GET /api/v1/search?q=needle` returns 403; `assert_eq!(resp.status(), StatusCode::OK)` panics.

Consequences: `cargo test --all` fails (acceptance criterion violated); the human-membership scoping criterion is unproven by any executed test. The candidate's own task notes concede the test was "type-checked here; execution deferred" — it has never been run. Fix is one line in the test (`alice.tenant_id = Some(Id::new("default"));`, matching the seeded workspaces), after which the suite must actually be executed.

## Verifier-required probes (server test binaries exceed this sandbox's 4 GiB build budget; stage build wrapper blocks them — infrastructure restriction, not a code defect)

- `cargo test -p gyre-server --test search_wiring` — expected to FAIL at line 448 per the chain above (this is the blocking finding's reproduction).
- `cargo test -p gyre-server --lib mcp_search` — `mcp_search_is_access_scoped_to_the_agent_token`; static read suggests it passes (mem test state, agent JWTs resolve tenant "default", docs seeded with tenant "default", output format matches the assertions), but it was never executed by the implementer either.
- Full `cargo test --all`, `bash scripts/check-arch.sh`, and the `scripts/check-*.sh` gates (rustfmt-diff vs base was repaired in `2cab4181f` per the task notes).

## Minor observations (non-blocking, recorded for completeness)

- `derive_search_scope` for humans intersects nothing with the workspace's tenant — memberships from another tenant would be admitted by workspace id but excluded by the `tenant_id = ?` clause; fail-closed, no leak.
- Docs (`docs/api-reference.md`) updated for the three REST endpoints and `gyre_search`; MCP tool description notes `workspace_id` only narrows.
- Generated `web/dist/` untouched by this candidate — no generated-output observations.
