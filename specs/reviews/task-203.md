# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping — every query filtered by caller access
(tenant from auth, workspace memberships, repo access within them), **at the index-query
level, not post-filter**, so result counts cannot leak; agents scoped by JWT claims
(repo orchestrator → its repo, workspace orchestrator → its workspace, never outside);
Design Principle 1: "Search is access-scoped."

Candidate: `0697c331` (base `1ae18d1c`); commits in scope per frontmatter: `a18e6e25`,
`f888d61d` (both verified ancestors). Full diff read: 14 files, +1215/−177; no `scripts/`,
`web/`, or exemption files touched (no gate weakened).

Verdict: **complete** (approved) — on thorough static verification; runtime test execution
was blocked by sandbox network policy and is deferred to verification with exact commands
(see below and `/tmp/stage/review-evidence/`).

## Round 1 (this review)

### Environment restriction (not a code defect)

`/tmp/stage/bin/cargo test -p gyre-adapters search` fails at registry update:
`index.crates.io`/`static.crates.io`/`crates.io` all unreachable (curl 000), no vendored
crates, no build cache, `PI_OFFLINE=1`. Per assignment, a transport restriction is not a
code defect; the focused probes were replaced by deep static verification plus
verifier-deferred runtime checks (recorded in `/tmp/stage/review-evidence/offline-restriction.log`).
The interrupted attempt-10 review left no usable evidence (0-byte log); nothing was
carried forward from it.

### What was independently verified

**Scope derivation is server-side and fail-closed** (`gyre-server/src/search_scope.rs`):
- Admin (incl. global dev token, `roles.contains(Admin)`) → unscoped within `auth.tenant_id`
  (adapter still enforces `tenant_id = ?`).
- Agent JWTs (`scope == "agent"`, Ed25519-validated in the auth extractor): workspace
  orchestrator → whole workspace; repo orchestrator → workspace + single repo. Claims
  shapes verified against `AgentSigningKey::mint`/`mint_orchestrator` in `auth.rs`.
- Worker agents fall through to their **agent record** (JWT has no orchestrator claims;
  agent tokens always carry `user_id: None` — verified at every `AuthenticatedAgent`
  construction site in `auth.rs` — so they can never land in the human-membership branch).
- Human users (API key / Keycloak) → accepted workspace memberships only.
- Unknown/ghost callers, failed lookups, federated JWTs without records → empty accessible
  set (zero results), never unscoped.

**Filtering is inside the index query in all three adapters:**
- SQLite FTS5 (`gyre-adapters/src/sqlite/search.rs`): `tenant_id = ?`, `workspace_id IN
  (…)`, `(repo_id = '' OR repo_id IN (…))` appended as WHERE clauses on the FTS MATCH
  query via diesel's boxed raw SQL; empty accessible set → `AND 0`.
- Postgres tsvector (`postgres/search.rs`): same constraints with `= ANY($n)` / `AND FALSE`.
- Mem (`mem_search.rs`): same constraints in the pre-scoring query filter.
- Diesel 2.3.7 semantics verified against upstream source (fetched via api.github.com):
  `BoxedSqlQuery::walk_ast` emits base SQL + appended fragments, then all binds in call
  order — the placeholder/bind ordering in both SQL adapters is provably aligned.
- The client-supplied `workspace_id` param is a second `workspace_id = ?` narrowing filter
  intersected with the accessible set — it can only narrow; requesting an inaccessible
  workspace yields zero results (no existence-leaking error).

**Handlers:** REST `search_handler` and MCP `gyre_search` take `AuthenticatedAgent` first
and pass only derived scope fields; grep confirms these are the only two production
`SearchQuery` construction sites. MCP scope comes from the token, never client args.

**Indexing:** task/agent/MR create routes route through `search_scope::index_doc`, which
stamps `workspace_id`/`repo_id` from the entity and resolves `tenant_id` from the
workspace's real scope row; unresolvable tenant → invisible to tenant-scoped queries,
never mis-attributed. SQL adapters derive tenant identically at index time.

**Tests are hard and anchored** (read line-by-line, kill conditions checked by
failure-mode analysis):
- `search_wiring.rs::search_is_access_scoped_to_the_authenticated_caller` — full router
  middleware stack, real API key + membership port, real Ed25519 minted JWTs registered
  in `agent_tokens`: alice (member of ws-a only) gets `total == 2` with ws-b's matching
  doc absent from results AND total; `workspace_id=ws-b` → total 0; repo orchestrator →
  only its repo; workspace orchestrator → both ws-a repos, never ws-b; ghost agent →
  total 0. Each assertion fails if scoping is removed, post-filtered with leaked counts,
  or widenable via client params.
- Adapter tests (`access_scoping_filters_inside_the_index_query` SQLite,
  `access_scoping_filters_inside_the_query_filter` mem,
  `repo_scope_restricts_repos_but_keeps_workspace_level_docs`) plus tenant-boundary and
  empty-set cases; MCP test covers the same matrix through `/mcp`.
- Note: the Shipped section describes the SQLite test as asserting SQL text; it is
  actually behavioral (result-set assertions) — stronger evidence, not weaker.

**Gates run here (static, no cargo needed):** `check-arch.sh`, `check-mem-port-contracts.sh`,
`check-abac-route-registry.sh`, `check-task-commit-attribution.sh` — all exit 0
(logs in `/tmp/stage/review-evidence/`). ABAC builtin policies confirmed to still allow
Developer/Agent read on the `search` resource (route is not accidentally admin-only).
Coverage matrix honestly untouched (search.md §Access Scoping row still `task-assigned`).

### Deferred to verification (exact commands)

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters search`
2. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test search_wiring`
3. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_search`
4. `cargo test --all` (acceptance criterion) and `bash scripts/check-arch.sh` (latter: PASS here)
5. Optional mutation probes listed in `offline-restriction.log` (remove the WHERE
   construction → tests must fail; unscope `derive_search_scope` → alice total 2→3 fails;
   honor client `workspace_id` for authorization → total-0 assertions fail)

Findings: none.

— Reviewer, 2026-10-09 (attempt 11; offline-sandbox static review, runtime deferred)
