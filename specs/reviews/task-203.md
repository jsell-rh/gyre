# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping — "Every search query is filtered by the caller's access … This filtering happens at the index query level (not post-filter) to avoid leaking result counts"; Design Principle 1: "Search is access-scoped."
Candidate under review: `0f45bd5b12c5219611fdfe7ce906da4efa88a8d5` (base `4e895f76`), reviewed as the full diff `4e895f76..0f45bd5b` (includes the rebase-repair merge `08c1c757` for durable finding `25e35a17`).
Commits per frontmatter: `6bf73593` (unwrap `seed_builtin_policies`), `790b031d` (LazyLock `ENV_LOCK`), `f888d61d`/`a18e6e25` (implementation checkpoints). Verdict: **complete**.

## Round 1

Test runs (this checkout at `0f45bd5b`, `SKIP_WEB_BUILD=1`, via `/tmp/stage/bin/cargo`; logs in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-adapters search` → **16 passed, 0 failed** (incl. `access_scoping_filters_inside_the_index_query` — asserts the SQLite SQL text carries `tenant_id = ?` / `workspace_id IN (...)` / `(repo_id = '' OR repo_id IN (...))`; `access_scoping_filters_inside_the_query_filter` — mem parity; `repo_scope_restricts_repos_but_keeps_workspace_level_docs`).
- `cargo test -p gyre-server --test search_wiring` → **2 passed, 0 failed** (`search_is_access_scoped_to_the_authenticated_caller`, `sqlite_url_wires_fts5_search_port` — proves the wired backend is FTS5, not mem, so the scoping evidence is against the real adapter).
- `cargo test -p gyre-server --lib mcp_search` → **1 passed, 0 failed** (`mcp_search_is_access_scoped_to_the_agent_token`).
- `bash scripts/check-arch.sh`, `check-mem-port-contracts.sh`, `check-abac-route-registry.sh`, `check-task-commit-attribution.sh` → all exit 0. No `scripts/` or exemption files touched by the diff.

Mutation probes (each restored with `git checkout --`, byte-identity against `git show HEAD:<file>` verified by md5; post-restore rerun of `search_wiring` green):

- **No scoping** (`search_scope.rs`: Admin branch → `if true`, everyone unscoped): `search_is_access_scoped_to_the_authenticated_caller` **FAILS** at the scoped-total assertion (`total` 3 ≠ 2, ws-b doc present) — the test genuinely kills unscoped search.
- **Client `workspace_id` widens** (`api/search.rs`: `accessible_workspace_ids` replaced by the client param when supplied): the same test **FAILS** at the dedicated assertion `client workspace_id widened access` (`workspace_id=ws-b` returned total 1 ≠ 0). The param-must-only-narrow contract is anchored, not asserted vacuously.

Verified against the acceptance criteria (no findings):

- **Scope derived from auth, never client args.** `search_handler` (`api/search.rs:48`) takes `AuthenticatedAgent` first and resolves scope via `search_scope::derive_search_scope`; MCP `gyre_search` (`mcp.rs:1830`) receives `&auth` and calls the identical derivation. `grep SearchQuery` in `gyre-server/src` shows exactly these two construction sites — no unscoped entry point remains. The client `workspace_id` param stays a narrowing filter the adapter intersects with the accessible set (union of `workspace_id = ?` and `workspace_id IN (...)` WHERE clauses in one query — intersection, not override).
- **Index-level filtering, read in the adapters.** SQLite FTS5 (`sqlite/search.rs`): scope constraints are appended as SQL fragments (`AND workspace_id IN (...)`, `AND (repo_id = '' OR repo_id IN (...))`, `AND tenant_id = ?`, `AND 0` for an empty accessible set) on the boxed `sql_query` over `search_index` — inside the query, before `ORDER BY rank LIMIT`. Postgres tsvector mirrors it with `$n`-numbered binds and `= ANY(...)`. The mem adapter applies the same constraints in its query filter before scoring. No post-filter anywhere; `total` is computed from the scoped result set.
- **Membership / agent scope semantics.** Admin (incl. global dev token) → `None`/`None` (unscoped within tenant — test: total 3). Agent JWTs: `orchestrator_type: workspace_orchestrator` → whole workspace, all repos; `repo_orchestrator` → single repo; workers fall through to their agent record's workspace/repo; ghost agent (no record) → empty accessible set, zero results (fail closed, test-asserted). Human users (API key / Keycloak JWT, `user_id` present) → accepted workspace memberships only; `workspace_id=ws-b` as a member of ws-a yields total 0. Cross-tenant membership cannot leak: even if `list_by_user` returns a workspace of another tenant, the adapter's `tenant_id = ?` clause excludes those docs (defense in depth).
- **Indexing stamps scope server-side.** Task/agent/MR creation routes documents through `search_scope::index_doc`, which sets `workspace_id`/`repo_id` from the entity and resolves `tenant_id` from the workspace's real scope row (previously all three were `None` — the old `workspace_id` filter could never match). Unresolvable tenant → doc visible to no tenant-scoped query, never a fabricated `"default"` (consistent with the repo's scope-literal-defaults invariant). Storage-backed adapters independently derive tenant from the workspace row when the doc doesn't carry one; a caller-supplied `tenant_id` on the document only short-circuits when non-empty.
- **Facet counts**: `facet_counts` does not exist yet (task-153 not landed — `grep facet_counts` empty), so AC item 5 is n/a; per-result facets come from the scoped SQL rows.
- **No auth weakened, boundaries preserved.** Hexagonal boundary holds (scope derivation is `gyre-server`; `gyre-ports` receives resolved id lists — `check-arch.sh` green). The Admin/global-token path is unchanged in behavior. Agent JWTs still require signature validation + presence in `agent_tokens` (auth.rs unchanged). No new exemptions, no scripts touched, no tests deleted. Diff touches only `search.rs` (ports), the three adapters' search modules, `search_scope.rs`, the two handlers' call sites, `mcp.rs` dispatch, docs, and the task file.
- **Commit attribution**: all four task-labeled product-surface commits (`6bf73593`, `790b031d`, `f888d61d`, `a18e6e258`) are in the `commits:` frontmatter; `check-task-commit-attribution.sh` exits 0. HEAD `0f45bd5b` drops the merge SHA `08c1c757` from the list, matching the canonical assignment (merge commits are excluded by that check anyway; the merge content is covered by reviewing `base..candidate` directly, which this review did).
- **Coverage matrix**: `specs/coverage/system/search.md` row 8 remains `task-assigned` — correct at this stage; status flips are the post-verification auditor's job (rows 4/5/7 for task-201 show the same pattern), and the worker did not self-mark.

Deferred to verification (per review scope — smallest meaningful probe): full `cargo test --all`, all-target Clippy, GitHub CI. The Postgres adapter's SQL is code-reviewed and compiles (unconditional module) but was not executed against a live PG server in this sandbox — no PG available; recommend the verifier exercise a PG-backed `build_state` search probe if PG is available in its environment.

Findings:

- (none)

## Shipped

- `SearchQuery` carries `tenant_id`, `accessible_workspace_ids` (`None` = Admin, empty = zero rows), `accessible_repo_ids` (repo-less workspace docs stay visible); user-supplied `entity_type`/`workspace_id` remain narrowing-only.
- `search_scope::derive_search_scope` (server-side): Admin → unscoped within tenant; agent JWT orchestrator claims → workspace/repo scope; workers → agent record; humans → accepted memberships; unknown → fail closed.
- All three adapters enforce the scope inside the index query (FTS5 WHERE clauses / tsquery `$n` binds / mem query filter); empty accessible set → `AND 0` / `AND FALSE` / no rows.
- REST `GET /api/v1/search` and MCP `gyre_search` derive scope from the authenticated caller; client `workspace_id` can only narrow.
- Task/agent/MR indexing stamps workspace/repo/tenant from the entity and the workspace's scope row via `search_scope::index_doc`.
- Hard tests on both surfaces (REST wiring test over the real FTS5 backend through the full router middleware stack; MCP token-scoping test), each verified by mutation to fail when scoping is removed or when the client param can widen.

— Reviewer, independent review round on `0f45bd5b`
