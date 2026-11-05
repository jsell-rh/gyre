# Review — task-203 (Enforce access scoping on search: tenant/workspace/repo, index-level filtering)

Spec: `specs/system/search.md` §Access Scoping (lines 72–86) + Design Principle 1
("Search is access-scoped. Results never include entities the user/agent can't
access.").
Candidate under review: `042e8f22c0f4c24c86d24e67cbeaa6d044ea643c` on
`pipeline/task-203/5b883fb841324767a194c61f9885baa4-1`, diffed against assigned
base `722179f03aef141e9e546285e3abef724086d25b`. Product commit: `a18e6e25`
(12 files, +1162/−231); remaining commits are task-file bookkeeping only
(`a8bcf435`, `043956de`, `042e8f22` touch only `specs/tasks/task-203.md` —
verified non-product-surface, so the single-entry `commits:` frontmatter is
correct under `check-task-commit-attribution.sh` rules).
Verdict: **approved** (independent evidence below; cargo test runs blocked by a
crates.io transport restriction in the review sandbox — exact commands recorded
for the verifier in `/tmp/stage/review-evidence/review-notes.md`; a restriction
is not a code defect).

## What was reviewed

Every hunk of the base..candidate diff (14 files): `gyre-ports/src/search.rs`,
`gyre-server/src/search_scope.rs` (new), `gyre-server/src/api/search.rs`,
`gyre-server/src/mcp.rs`, `gyre-server/src/api/{tasks,agents,merge_requests}.rs`
(indexing), `gyre-server/src/lib.rs`, `gyre-adapters/src/{sqlite,mem,postgres}/…search.rs`,
`gyre-server/tests/search_wiring.rs`, `docs/api-reference.md`, task file.

## Spec-conformance findings (verified by reading, probes where possible)

- **Scope is derived from the authenticated caller, never client params.**
  REST `search_handler` (`api/search.rs:48`) now takes `AuthenticatedAgent`
  first and calls `search_scope::derive_search_scope`; MCP `gyre_search`
  (`mcp.rs:1809`) receives `&auth` from the dispatch and applies the identical
  derivation. These are the only two production `SearchPort::search`
  callsites (grep-verified; `gyre-cli` calls the REST endpoint client-side).
  The client-supplied `workspace_id` flows only into `SearchQuery.workspace_id`,
  which every adapter ANDs with the accessible set — it can narrow, never widen.
- **Derivation covers every caller class and fails closed.**
  `search_scope.rs`: Admin (incl. global dev token) → unscoped within tenant;
  agent JWT with `orchestrator_type` claims → workspace orchestrator = whole
  workspace (`accessible_repo_ids: None`), repo orchestrator = single repo;
  worker agent JWT → its agent record's workspace/repo; human users (API key /
  Keycloak JWT, `user_id` present) → accepted workspace memberships only;
  unknown/ghost caller → `Some(vec![])` (zero results, never unscoped).
  Membership-lookup failure (`list_by_user(...).unwrap_or_default()`) also
  fails closed. Agent tokens carry `user_id: None` (`auth.rs` extractor), so
  the human/agent branches cannot be confused.
- **Filtering is inside the index query, not post-filter.** SQLite FTS5
  (`sqlite/search.rs:191-245`): `tenant_id = ?`, `workspace_id IN (...)`
  (empty set → `AND 0`), `(repo_id = '' OR repo_id IN (...))` are appended to
  the `search_index MATCH ?` statement as SQL via the boxed diesel query —
  `total`/result lists are derived from the filtered statement. Postgres
  tsvector (`postgres/search.rs`): same clauses with `$n`-numbered binds and
  `= ANY(...)`. Mem adapter: same constraints in the pre-scoring filter.
  Index-level semantics confirmed by an independent sqlite3 probe replicating
  the exact emitted SQL shapes against the verbatim `FTS5_DDL` (evidence:
  `/tmp/stage/review-evidence/sql-scoping-probe.{sql,out}`): scoped 3/leak 0,
  empty set 0, client `workspace_id` intersection 0, admin tenant-confined,
  repo scope keeps workspace-level docs and excludes other repos.
- **Indexing stamps real scope.** `search_scope::index_doc` resolves
  `tenant_id` from the workspace's scope row (`state.workspaces.find_by_id`)
  and stamps `workspace_id`/`repo_id` from the entity; task/agent/MR create
  paths route through it. A workspace that doesn't resolve leaves tenant unset
  → the doc is visible to no tenant-scoped query (never mis-attributed) —
  consistent with the fabricated-scope invariants. The SQLite/PG adapters
  independently re-derive tenant from the workspace row when the doc carries
  none, so both wiring paths agree.
- **Tests are hard and anchored to the acceptance criteria.**
  `search_wiring.rs::search_is_access_scoped_to_the_authenticated_caller`
  drives the full router (auth → ABAC → handler) with a real API-key user
  (accepted membership in ws-a only), real minted orchestrator/worker JWTs,
  and a ghost agent: asserts admin total 3, member total 2 with ws-b's
  matching doc absent from results AND total, `workspace_id=ws-b` param →
  total 0, repo-orchestrator single-repo, workspace-orchestrator both-repos-
  never-ws-b, worker = its workspace, ghost = 0. Removing the scoping (or
  letting the param widen) flips these assertions. The MCP test
  (`mcp_search_is_access_scoped_to_the_agent_token`) does the same over the
  MCP dispatch with token-bearing requests, including the "client
  `workspace_id` cannot widen" leg. Facet counts: no `facet_counts` exists in
  any response yet (task-153 pending), so the count-leak criterion reduces to
  `total`, which the tests pin.
- **No weakening of existing auth, no boundary violations.** Admin/global-token
  path unscoped within tenant (specced); ABAC route registry unchanged
  (`/api/v1/search` was already mapped, `check-abac-route-registry.sh` PASS);
  scope derivation is server-side in `gyre-server`, the port only receives
  resolved id lists (`check-arch.sh` PASS). All 19 repo invariant scripts pass
  (logs in review-evidence).

## Notes (non-blocking)

- **N1 — overstated test-evidence parenthetical in the Shipped section.** It
  says the SQLite test "asserts the SQL text carries the WHERE clauses"; the
  test (`access_scoping_filters_inside_the_index_query`) is behavioral (result
  counts/ids), and no test asserts the generated query text (only the DDL test
  asserts `sqlite_master` SQL). The acceptance criterion itself specifies
  "verified by reading the adapter query", which this review did, and the
  independent sqlite3 probe confirms the emitted SQL shapes — so the criterion
  is met, but the parenthetical misdescribes the test's mechanism.
- **N2 — pre-change index rows become invisible (fail-closed, intended).**
  Documents indexed before this change with `workspace_id` empty (old task/agent
  indexing) carry no resolvable tenant and are now visible to no tenant-scoped
  query, including Admin's. That is the never-mis-attribute behavior the
  invariants require; the rebuild half of reindex is task-202's coordinator,
  and all new writes stamp scope.
- **N3 — claims-trust boundary.** `is_agent_jwt` keys on a `scope == "agent"`
  claim, so a Keycloak-issued token carrying agent-scope claims would be
  classified as an agent. Claims come from the trusted IdP (the same trust the
  spec's "scope is determined by their OIDC token claims" relies on), tenant is
  still claim-validated, and membership-derived human scope is unaffected.
  Noted for the record; no action required for this section.

## Verification gap recorded for the verifier

This review sandbox cannot reach crates.io (connection refused; registry cache
holds 14 of ~1000 locked crates), so the Rust suites could not be run here.
Evidence supporting the approval besides the code read and probes above: the
pipeline-captured iteration-1 log shows `cargo test -p gyre-adapters search` →
16 passed / 0 failed on this code. The verifier must run (see
`/tmp/stage/review-evidence/review-notes.md` for the full list and expected
anchors):

```
SKIP_WEB_BUILD=1 cargo test -p gyre-adapters search
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib mcp_search
SKIP_WEB_BUILD=1 cargo test -p gyre-server --test search_wiring
cargo test --all && bash scripts/check-arch.sh
```

Findings: none (N1–N3 are notes, not defects).

— Review, 2026-11-05, candidate `042e8f22`
