# Review — task-203 (Search §Access Scoping — tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping ("Every search query is filtered by the caller's access... filtering happens at the index query level (not post-filter) to avoid leaking result counts"; Design Principle 1: "Search is access-scoped").
Commits under review: `a18e6e25`, `f888d61d`, `790b031d`, `6bf73593` (candidate `546be125`, base `4d6759a0`).
Verdict: **approved**.

## Round 1

Independent verification at candidate `546be125` (clean tree; all cargo probes via `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`; full evidence and logs in `/tmp/stage/review-evidence/task-203-review.md`):

- `cargo test -p gyre-server --test search_wiring` → **2 passed** (`search_is_access_scoped_to_the_authenticated_caller`, `sqlite_url_wires_fts5_search_port`).
- `cargo test -p gyre-server --lib mcp_search` → **1 passed** (`mcp_search_is_access_scoped_to_the_agent_token`).
- `cargo test -p gyre-adapters search` → **16 passed** (SQLite FTS5 + mem, incl. the index-query and repo-scope scoping tests).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-forged-scope-fields.sh` → all exit 0.

**Mutation testing (kill-test proof, per the AC "test MUST fail if scoping is post-filter that still leaks counts or if client-supplied workspace_id can widen access"):**

- Disabling `derive_search_scope` (early-return unscoped for everyone) makes `search_is_access_scoped_to_the_authenticated_caller` **FAIL** with ws-b's document leaking into both results and `total` (3 ≠ 2) — exactly the leak the AC targets.
- No-op'ing the `accessible_workspace_ids` clause in the SQLite adapter makes **both** the wiring test and `access_scoping_filters_inside_the_index_query` **FAIL** — the client-`workspace_id`-widening failure mode is caught at two levels.

Mutations reverted; post-restore reruns green; tree identical to candidate.

**Source-level verification (beyond the diff):**

- Port: `SearchQuery` gains `tenant_id`, `accessible_workspace_ids: Option<Vec<String>>`, `accessible_repo_ids: Option<Vec<String>>` exactly per plan; user `workspace_id`/`entity_type` remain narrowing-only.
- Adapters: tenant + accessible workspace/repo sets are enforced **inside the index query** — SQL WHERE clauses on the FTS5 MATCH / PG tsvector query (boxed raw queries, bind/placeholder ordering verified correct, PG `$n` counter tracked), `AND 0`/`AND FALSE` on empty accessible sets; mem applies the same constraints in its pre-scoring filter. No post-filtering anywhere; repo-less workspace docs intentionally stay visible under repo scope (documented).
- Handlers: REST `search_handler` (`api/search.rs:47`) and MCP `gyre_search` (`mcp.rs:1831`) take `AuthenticatedAgent` first; scope is derived from the token context, never client args (`mcp_handler`'s auth comes from the extractor at `mcp.rs:3029`).
- `search_scope.rs` derivation order is sound against `auth.rs`: agent JWTs carry `user_id: None` (auth.rs agent-token path), so orchestrator-claim agents, worker agents (agent-record scope), and human users (accepted memberships) each resolve through the correct arm; Admin/global token → unscoped within tenant; unknown callers fail closed with an empty accessible set. `unwrap_or_default()` on the membership lookup fails closed (zero results), the safe direction.
- Indexing (tasks/agents/MRs) routes through `search_scope::index_doc`: workspace/repo stamped from the entity, tenant resolved from the workspace's real scope row, unresolvable → invisible to every tenant-scoped query (never mis-attributed). The `req.workspace_id` read in `create_task` is pre-existing and unchanged.
- Only two production callers of `SearchPort::search` exist (REST + MCP); both are scoped.

**Non-blocking notes (recorded for completeness):**

- Coverage row 8 in `specs/coverage/system/search.md` still reads `task-assigned`; coverage rows are auditor-maintained in this repo's convention (task-201's rows 4/5/7 were likewise left to the auditor), and understating shipped state is not fakery.
- The Postgres adapter follows the repo-wide no-live-PG-integration-test pattern; its SQL construction was reviewed manually (bind ordering correct) and shares the construction shape proven by the SQLite tests.
- REST `SearchResponse` has no aggregate `facet_counts` (task-153 scope), so the facet-count clause of AC-3 is vacuously satisfied: per-result facets come from the already-scoped result set, and `total` is `results.len()` of that set.
