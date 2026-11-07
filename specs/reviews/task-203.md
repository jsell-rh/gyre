# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping — every search query filtered by the caller's access (tenant from auth, accessible workspaces, accessible repos), filtering at the **index-query level** (not post-filter) to avoid leaking result counts; repo-scoped agent → its repo, workspace orchestrator → all repos in its workspace, no agent searches outside its workspace.

Candidate: `4c1f2f62c9b703267a2da98299b791b005bb1459` (base `4d6759a0`). Scoping per `commits:` frontmatter: `a18e6e25`/`f888d61d` (implementation checkpoints on merged trees), `790b031d` (verification repair: `ENV_LOCK` static in `search_wiring.rs` rebuilt via `std::sync::LazyLock` so no non-const call sits in the static initializer — E0015 on the verification toolchain; test logic unchanged).

Verdict: **complete** — approved, no findings.

## What was verified (independent evidence, this checkout at the candidate SHA)

### Scope derivation is server-side and fail-closed

- `crates/gyre-server/src/search_scope.rs::derive_search_scope` — read line-by-line against every `AuthenticatedAgent` construction path in `auth.rs`:
  - **Admin / global dev token** (roles contains Admin, incl. `jwt_claims: None` global token) → unscoped within `auth.tenant_id` — Admin and global-token paths not broken.
  - **Agent JWT** (`scope == "agent"`, signature-validated by `AgentSigningKey::validate` before the claims are trusted): `orchestrator_type == workspace_orchestrator` → whole workspace, repos unrestricted; `repo_orchestrator` → single (workspace, repo) pair. Claims cannot be client-forged — only the server's EdDSA key mints them, and a JWT not present in `agent_tokens` is rejected as revoked.
  - **Worker agents** (minted by `mint()`, which sets `workspace_id: None` — the code comment is accurate) → their agent record's workspace/repo; **no agent record (ghost)** → `Some(vec![])`/`Some(vec![])`, zero results. Confirmed fail-closed by the wiring test.
  - **Human users** (API key / Keycloak JWT, `user_id` set) → accepted workspace memberships only; zero memberships → empty set → zero results.
- The client-supplied `workspace_id` param is only ever an additional `AND workspace_id = ?` inside the accessible-set restriction on all three adapters — it intersects, never widens (asserted behaviorally in all three test suites).

### Filtering is inside the index query, not post-filter

- **SQLite FTS5** (`crates/gyre-adapters/src/sqlite/search.rs`): the boxed raw query appends `AND tenant_id = ?`, `AND workspace_id IN (…)` (bound parameters), `AND (repo_id = '' OR repo_id IN (…))`, empty accessible set → `AND 0`, all before `ORDER BY rank LIMIT ?` — read directly in the adapter source per the acceptance criterion. No string interpolation of ids (placeholders bound via diesel).
- **Postgres tsvector** (`postgres/search.rs`): mirrors it with `$N` placeholder bookkeeping (verified the `n` increments match every appended `.sql`+`.bind` pair) and `= ANY($n)` arrays; empty set → `AND FALSE`.
- **Mem adapter** (`mem_search.rs`): same constraints in the pre-scoring query filter; repo-less docs (`None`/`""`) stay visible under repo scope, matching the task plan's exact clause `(repo_id IS NULL OR repo_id IN (…))`.
- Only two production `SearchQuery` constructions exist (REST `api/search.rs`, MCP `mcp.rs handle_search`); both derive scope from `AuthenticatedAgent` — `grep` confirms no unscoped caller anywhere in gyre-server/gyre-cli.

### Indexing stamps real scope

- `search_scope::index_doc` (used by task/agent/MR creation) resolves `tenant_id` from the workspace's real scope row; unresolvable → tenant stays unset → visible to **no** tenant-scoped query (fail-closed, no fabricated "default"). The storage-backed adapters independently derive tenant from the workspace row when the doc carries none. Verified against `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh` — both OK.

### Test runs (fresh, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`; logs in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --test search_wiring` → **2 passed** (`search_is_access_scoped_to_the_authenticated_caller`: member of ws-a gets total 2 with ws-b's doc absent from results AND total; `workspace_id=ws-b` param → total 0; repo orchestrator, workspace orchestrator, worker, ghost-agent paths; `sqlite_url_wires_fts5_search_port`: FTS5 wiring over the real router incl. POST /tasks → GET /search flow, tenant boundary).
- `cargo test -p gyre-server --lib mcp_search` → **1 passed** (MCP `gyre_search` scoped to the agent token; client `workspace_id` cannot widen).
- `cargo test -p gyre-adapters search` → **16 passed** (SQLite + mem scoping, repo scope, tenant boundary, empty-set).
- Gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-forwarded-header-trust.sh` — all exit 0.

### Mutation probes (source restored afterwards; `git status --short` clean, HEAD re-verified at the candidate SHA)

- Disabled scoping in `derive_search_scope` (early return of unscoped-within-tenant for every caller) → `search_is_access_scoped_to_the_authenticated_caller` **FAILS** (`scoped total must exclude ws-b`, total 3 ≠ 2) and `mcp_search_is_access_scoped_to_the_agent_token` **FAILS** (repo scope wrong: Found 3). Both tests are genuinely anchored to the enforcement, not to incidental behavior. Logs: `mutation-search-wiring.log`, `mutation-mcp-search.log`.

### Acceptance criteria check

- [x] REST + MCP derive tenant/workspace/repo scope from the authenticated caller, never client params.
- [x] Filtering is SQL WHERE at index-query level (verified by reading all three adapters).
- [x] Workspace-A member sees only ws-A results; total excludes ws-B (no `facet_counts` aggregation exists yet — task-153 not landed, so that plan item is n/a; the response `total` is derived from the scoped result set).
- [x] Repo-scoped agent → its repo (+ repo-less workspace docs, exactly the plan's clause); workspace orchestrator → all repos in its workspace; neither widens via `workspace_id` param (tested both surfaces).
- [x] Admin unscoped within the tenant (global dev token path intact).
- [x] Two-workspace seeded test asserting absence AND count exclusion, with mutation-proof anchoring.
- [x] Focused suites + arch gate pass here; full `cargo test --all`, all-target Clippy, and web build deferred to verification per assignment scope (no web code touched; `SKIP_WEB_BUILD=1` uses committed `web/dist`).

## Non-blocking notes (for the verifier / auditor, no action required for this task)

1. The Shipped narrative describes the SQLite test `access_scoping_filters_inside_the_index_query` as "asserts the SQL text carries the WHERE clauses"; the test is in fact behavioral (result/count assertions). The acceptance criterion prescribes verification "by reading the adapter query", which this review performed directly — the WHERE clauses are genuinely in the index query — so the criterion is met; only the test-evidence wording overstates.
2. Federated remote-Gyre agents (G11 path) now fail closed to zero search results: their host-prefixed agent id resolves to no local agent record, and remote `mint()` claims carry no workspace. Direction is safe and spec-compliant ("no agent can search outside its workspace"); pre-change they could search everything, so this is a tightening, not a regression.
3. `specs/coverage/system/search.md` row 8 remains `task-assigned` — consistent with pipeline convention (task-201/202 rows likewise await the spec-fidelity auditor; the matrix reflects reality, it does not define it).
4. Human users receive `accessible_repo_ids: None` (all repos within accessible workspaces) — matches the task plan's own clause and the platform model, which has no repo-level grant for human members.

Findings: none.

— Reviewer, task-203 independent review
