# Review — task-203 (Enforce access scoping on search — tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping. Candidate
`0b70f265d5b582066407c680232149c0c534b490` vs assigned base
`fefde2946f3a0af1f212588f54591516fee0f7b2` (base verified an ancestor).
Scoping commits `a18e6e25`, `f888d61d`, `790b031d`, `6bf73593` all verified
ancestors of the candidate. Diff surface: 15 files, all reviewed hunk-by-hunk.

Verdict: **complete** — real, production enforcement on both search entry
points; no findings.

## What was independently verified in this sandbox

- **Scope derivation is server-side and fail-closed**
  (`crates/gyre-server/src/search_scope.rs`, new). `derive_search_scope`
  resolves `tenant_id` from auth, then: Admin (incl. global dev token, which
  `auth.rs` mints with `UserRole::Admin`) → unscoped within the tenant; agent
  JWTs (`scope == "agent"` in claims) with `orchestrator_type` → workspace
  orchestrator sees its whole workspace, repo orchestrator its single repo;
  worker agents fall to their agent record's workspace/repo; human users
  (API key / Keycloak — both carry `user_id`) → **accepted** workspace
  memberships only; unknown caller → empty accessible set (zero rows, never
  unscoped). Cross-checked against every `AuthenticatedAgent` construction
  path in `auth.rs` (agent JWTs always have `user_id: None`, so agents cannot
  reach the human-membership branch; a failed membership lookup
  (`unwrap_or_default`) also fails closed). A cross-tenant membership cannot
  leak: docs carry their workspace's real tenant, and the tenant equality is
  enforced in every adapter.
- **Filtering is inside the index query, not post-filter** — verified by
  reading the adapter SQL. SQLite FTS5 (`sqlite/search.rs`): `tenant_id = ?`,
  `workspace_id IN (...)`, `(repo_id = '' OR repo_id IN (...))` appended to
  the `MATCH` query as boxed binds in placeholder order; empty accessible set
  → `AND 0`. Postgres (`postgres/search.rs`): identical shape with `$n`
  numbering, `= ANY(...)`, `AND FALSE`; bind counter verified step-by-step.
  Mem adapter: same constraints in the query filter **before** scoring. The
  handler's `total` is derived from the already-scoped result set, so counts
  cannot leak inaccessible documents.
- **Both entry points derive scope from the token context, never client
  args.** REST `search_handler` (`api/search.rs`) takes `AuthenticatedAgent`
  first and calls `derive_search_scope`; MCP `gyre_search` (`mcp.rs`)
  receives the handler's `auth` and does the same. The client-supplied
  `workspace_id` stays an intersecting narrowing filter (`AND workspace_id =
  ?` in addition to the accessible-set IN-clause) — it can only narrow.
  Grep-verified these are the **only** two production `SearchQuery`
  consumers; no unscoped path remains.
- **Indexing stamps real scope fields.** task/agent/MR creation routes
  through `search_scope::index_doc`, which stamps `workspace_id`/`repo_id`
  from the entity (fixing the prior `workspace_id: None` gap noted in
  coverage row 6) and resolves `tenant_id` from the workspace's real scope
  row; unresolvable → doc visible to no tenant-scoped query, never
  mis-attributed. `index_doc` is the only production index callsite.
- **Tests are hard, not self-confirming.** `search_wiring.rs`
  (`search_is_access_scoped_to_the_authenticated_caller`): exact-count
  assertions (`total == 2` for a ws-a member where 3 docs match — kills
  count-leaking post-filter), entity-id absence (`b-doc` never in results),
  `workspace_id=ws-b` param → `total == 0` (kills param widening), repo/
  workspace orchestrator and worker-agent paths, ghost agent fails closed.
  Adapter tests pin the scoped/empty-set/tenant-boundary/repo-scope
  semantics directly against the port, including SQLite tenant derivation
  against real workspace rows. MCP test drives the full router with minted
  orchestrator JWTs and asserts the "Found N result(s)" counts. ENV_LOCK
  serializes the env-poking integration tests correctly (both take the lock
  across `build_state`).
- **Compile-surface of the new tests statically verified** (this sandbox
  cannot compile — see below): `Workspace::new`, `WorkspaceMembership::new`
  (+ `accepted` field), `User::new`, `ApiKeyRepository::create`,
  `Agent::new` + `workspace_id`, `mint`/`mint_orchestrator`,
  `kv_set("agent_tokens", …)`, `WorkspaceMembershipRepository::list_by_user`,
  `AgentRepository`/`WorkspaceRepository::find_by_id` — all signatures match
  test usage.
- **Mechanical gates (all exit 0, logs under
  `/tmp/stage/review-evidence/`)**: check-arch, check-mcp-write-tools,
  check-abac-route-registry, check-mem-port-contracts,
  check-migration-sql-portability, check-inert-enforcement,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-relative-path-defaults, check-byte-slice-truncation,
  check-dead-message-kinds, check-lossy-secret-conversion,
  check-unbounded-external-http, check-forwarded-header-trust,
  check-fail-open-ref-resolution, check-in-memory-state-stores,
  check-task-commit-attribution; `check-rustfmt-diff.py` clean vs base (12
  Rust files); `git diff --check` clean; working tree clean throughout (no
  source edits made by this review).
- **CI runner pin (`df800fe2`) matches the retained failure log**
  (`/tmp/stage/findings.log`, PR #741 run 42573267153): the e2e job failed
  only at `Install Playwright browsers` — `ubuntu-latest` resolved to
  ubuntu 26.04, unsupported by the locked Playwright 1.58.2. Every earlier
  step (release build, npm ci, web build, vitest) passed on PR head
  `ae939f628`; `git diff ae939f628..0b70f265` touches only the e2e.yml pin
  and task-file docs, so that run attests the candidate's production code
  compiles. The pin changes no locked dependency, test, or assertion.

## Deferred to verification (infrastructure-blocked here, not code defects)

This sandbox has no crates.io network access (TCP refused; no vendored
registry or build cache), so cargo test/clippy probes were attempted and
blocked at dependency resolution — failure log retained. The verifier must
run: `cargo test -p gyre-adapters search`;
`cargo test -p gyre-server --lib mcp_search`;
`cargo test -p gyre-server --test search_wiring`; `cargo test --all`;
`check-clippy-diff.py` (its workspace-wide cargo build is rejected by the
review-stage wrapper); and exact-head GitHub checks on the pinned
`ubuntu-24.04` e2e job. Full list in
`/tmp/stage/review-evidence/task-203-deferred-verification-checks.md`.

## Notes (non-blocking)

- Task-file prose says `access_scoping_filters_inside_the_index_query`
  "asserts the SQL text carries the WHERE clauses"; the test is behavioral.
  The SQL-text fact is nonetheless true by direct code reading (above), so
  the enforcement claim does not rest on the prose.
- Repo-scoped callers keep repo-less (workspace-level) docs visible — this
  is the task plan's explicit design ("repo-less workspace docs stay
  visible") and does not violate "no agent can search outside its
  workspace".
- Coverage-matrix row for §Access Scoping remains `task-assigned`
  (same convention as task-201's row); matrix updates are the auditor's
  stage, not the implementing task's.

Findings: none.

— Reviewer, 2026-11-09
