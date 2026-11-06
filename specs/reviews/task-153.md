# Review — task-153 (Search autocomplete and facets endpoints)

Spec: `specs/system/search.md` §API (endpoint table) + §Response Format
(`facet_counts`). Task plan and acceptance criteria as in
`specs/tasks/task-153.md`.
Candidate under review: `3b4da7bd` (scoped product commit `fe303595`,
recovered after worker-lease expiry and merged onto base `7f8b5d9c` as
`e572e17d`; attribution/process commits `586a322b`, `3b4da7bd` touch only the
task file).
Verdict: **approved** — every acceptance criterion is met by real production
code; all offline probes pass. Rust test execution is deferred to remote
verification (sandbox has no crates.io reachability — verified, see
Restrictions).

## Scope

`git diff 7f8b5d9c..3b4da7bd --stat` touches exactly the task-153 surface:
`gyre-ports/src/search.rs` (+26), `gyre-ports/src/lib.rs` (re-export),
`gyre-adapters/src/{sqlite,postgres}/search.rs` (+339/+153),
`gyre-adapters/src/mem_search.rs` (+217), `gyre-server/src/api/search.rs`
(+86), `api/mod.rs` (+2 routes), `abac_middleware.rs` (+2 resolver entries),
`gyre-server/tests/search_wiring.rs` (+111, purely additive),
`docs/api-reference.md`, coverage matrix, task file. No out-of-scope edits.

## Verified working (independent evidence)

- **Port contract is real and total.** `SearchPort::suggest(prefix, limit)` and
  `SearchPort::facet_counts(Option<SearchQuery>)` added with no default impl;
  grep confirms exactly three implementors (SqliteStorage, PgStorage,
  MemSearchAdapter) and all three provide both methods — no missed implementor
  that would silently fall back or fail to compile.
- **SQLite adapter (the wired default).** `suggest` = `title LIKE ? ESCAPE '\'`
  on the FTS5 table with `escape_like()` escaping user `%`/`_`/`\` so they
  match literally (adapter test `suggest_escapes_like_wildcards_in_prefix`
  kills unescaped-wildcard regressions with the `100%`/`a_` vs `100 percent`/
  `axb` pair). `facet_counts(Some(q))` reuses the exact `MATCH` +
  `entity_type`/`workspace_id` WHERE clauses of `search()` minus LIMIT, so
  `facet_counts` describes the full filtered match set, not the truncated page
  (test pins this with `limit: 1` and asserts untruncated counts). `None`
  aggregates the whole index. `FacetRow.workspace_id` binds non-nullable
  `Text` — safe because `index()` has always written
  `unwrap_or_default()` (never NULL) on both SQL adapters.
- **PostgreSQL adapter.** Same contract via `ILIKE ... ESCAPE` and
  `websearch_to_tsquery` match-set aggregation mirroring `search()`'s WHERE
  clauses; empty-query guard matches `search()`'s.
- **Mem adapter.** `facet_counts(Some(q))` mirrors `search()`'s filters
  exactly (entity_type, workspace_id, `score_doc` AND-of-terms); `suggest` is
  a lowercased title-prefix match with the same empty-prefix guard the SQL
  adapters enforce in code (mem has no schema — guards in code, per the port
  contract invariant).
- **Handlers.** `suggest_handler` (empty/whitespace prefix → empty list, limit
  clamp 50), `facets_handler` (whole-index `None`), and `search_handler` now
  carries `facet_counts` computed from a second `facet_counts(Some(q))` call
  with the result-page limit excluded — matching the task plan's "counts
  reflect the filtered result set". All calls hit the real `SearchPort`.
- **Routes + ABAC.** Both routes registered in `api/mod.rs:616-617` with
  resolver entries in `abac_middleware.rs:378-379`;
  `scripts/check-abac-route-registry.sh` passes (unregistered routes get no
  policy evaluation — these are registered). Reindex stays admin-only at
  `/api/v1/admin/search/reindex` (pre-existing M34 Slice 5 placement,
  disclosed; the spec-table path was never this task's deliverable — the
  pre-task coverage row already counted the admin path as the existing
  endpoint).
- **Tests are hard, not self-confirming** (static kill-condition analysis;
  execution deferred): adapter suites pin mid-title terms NOT matching
  (autocomplete must not degrade to substring search), wildcard escaping,
  limit-ignored aggregation, entity_type/workspace filter restriction, and
  non-matching query aggregating nothing. The wiring test
  (`tests/search_wiring.rs`) drives the full middleware router via oneshot:
  suggest prefix match, empty-prefix/non-matching empty lists, **401 without
  token on both new endpoints**, `facet_counts` over the filtered match set,
  `/facets` whole-index counts. Its count expectations were statically
  validated: only tasks/agents/MRs index on create, `build_state` +
  `seed_builtin_policies` index nothing, and the tasks-create path indexes
  `status=backlog` with `entity_type=task` — so `facets["type"]["task"] == 2`
  (wiring-1 + quokka task) and `fc["status"]["backlog"] == 1` hold. Test
  changes are purely additive; no existing leg weakened.
- **Checkpoint-recovery integrity independently re-verified.** The round-9
  head `6da69d14` is reachable in this review clone (contrary to the Shipped
  note's unreachable claim — that was true only of the worker's sandbox).
  Blob-hash comparison of all nine task-153 product files between `6da69d14`
  and the candidate: byte-identical except the disclosed one-line
  `#[allow(clippy::double_must_use)]` on the `SearchPort` trait (benign lint
  suppression for async_trait-generated must_use futures). Evidence:
  `review-evidence/cmp-round9-recovery.txt`.
- **Offline mechanical checks (all pass; logs in review-evidence/):**
  `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-abac-exempt-handlers.sh` (88 handlers), `check-task-commit-attribution.sh`
  (frontmatter `commits: [fe303595]` covers the product commit),
  `python3 scripts/check-rustfmt-diff.py 7f8b5d9c` — all exit 0.
- **Compile-sanity (static, since cargo cannot resolve deps):** serde is a
  dependency of gyre-ports (SearchSuggestion derives); serde_json/diesel in
  gyre-adapters; tower "util" + tempfile + http are gyre-server dev-deps
  (oneshot legs); `build_router`/`build_state`/`abac_middleware` are pub;
  test helpers `make_doc`/`doc`/`tmp_storage` exist; deref coercions in the
  mem adapter closures are valid.

## Restrictions (not code defects)

This sandbox cannot reach crates.io (curl to index.crates.io and
static.crates.io returns HTTP 000; no `~/.cargo` registry cache, no vendored
deps, no prebuilt target/). `cargo check -p gyre-ports --offline` fails at
dependency resolution (`no matching package named anyhow found`) — evidence:
`review-evidence/cargo-offline-probe.txt`. The Rust suites therefore could
not be executed here. Remote verification must run:

    cargo test -p gyre-server --test search_wiring
    cargo test -p gyre-adapters search
    cargo test -p gyre-ports --lib
    cargo clippy -p gyre-ports -p gyre-adapters -p gyre-server

## Observations (non-blocking, for the spec-fidelity auditor)

- §Response Format's example also shows `url` (and `workspace`/`repo` echo)
  fields on result items; these remain unimplemented. Pre-existing gap
  (absent before task-153), outside this task's plan and acceptance criteria,
  and transparently disclosed in coverage row 14 ("Still missing vs spec
  example: url field").
- `suggest`/`facets` are not tenant-scoped at the data level — they exactly
  mirror the pre-existing `/api/v1/search` endpoint's scope model, as the task
  instructions require ("same as the existing search endpoint"). The
  cross-tenant search-scope concern is a pre-existing platform-level issue
  (coverage row 15 documents a worse variant on the MCP path) and is not a
  regression introduced by this candidate.
- The base CLI (task-155) still falls back to `/search` + client-side prefix
  filtering for `--suggest` rather than calling the new endpoint; switching it
  over is task-155's scope, not this task's.

Findings:

- (none)
