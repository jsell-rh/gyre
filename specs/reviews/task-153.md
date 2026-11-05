# Review — task-153 (Search §API/§Response Format — suggest, facets, facet_counts)

Candidate: `f5ad8e57c7529b7a7d4d7cc5aac4f0e5669f014b` (base `d81b0fe4`, task commits
`774499be`, `27503257`, `60493e27`).
Verdict: **changes requested** — one CI-breaking formatting defect; the functional
implementation itself is sound on static review.

## Environment restriction (not a code defect)

This sandbox cannot reach crates.io (`curl https://index.crates.io/config.json` →
connection refused, exit 7; no registry cache, no prebuilt target dir;
`cargo metadata --offline` fails on `anyhow`). Rust compilation and test execution
are therefore impossible here. Deferred to remote verification:

- `cargo test -p gyre-server --test search_wiring`
- `cargo test -p gyre-adapters search`
- `cargo clippy -p gyre-ports -p gyre-adapters -p gyre-server`

Raw probe evidence: `/tmp/stage/review-evidence/network-probe.log`.

## Finding (blocking)

**`crates/gyre-ports/src/lib.rs:105` — changed line violates rustfmt; repo fmt CI
gate fails.** The candidate's own edit

```rust
pub use search::{FacetCounts, SearchDocument, SearchPort, SearchQuery, SearchResult, SearchSuggestion};
```

is 103 columns (max_width 100) and needs wrapping. Reproduced with the repo's own
changed-lines policy:

```
$ python3 scripts/check-rustfmt-diff.py d81b0fe4ab93abff06089432e4a8fca5be6e5d31
rustfmt: crates/gyre-ports/src/lib.rs: changed lines need formatting: 105
exit=1
```

The CI Format job (`ci.yml:44`, `check-rustfmt-diff.py HEAD^1` on every PR to
`main`) checks out the PR merge commit, so `HEAD^1` is the main tip and this changed
line is in scope — the job fails on any PR carrying this diff. The pre-commit
`cargo fmt --all -- --check` hook flags the same line. This contradicts the round-8
shipped claim that "rustfmt --check on all six task-touched files" passed — `lib.rs`
was touched by task commit `774499be`. One-line fix: wrap the `use` item as rustfmt
suggests.

## Verified sound (static review of the full diff)

- **Port**: `SearchPort::suggest(prefix, limit)` and
  `SearchPort::facet_counts(Option<SearchQuery>)` added with no default impl; the
  only three implementors (SQLite `SqliteStorage`, `PgStorage`, `MemSearchAdapter` —
  exhaustive `grep "impl SearchPort for"`) all provide real implementations.
- **SQLite**: `suggest` = `title LIKE 'prefix%' ESCAPE '\'` with `escape_like()`
  escaping `%`/`_`/`\` in user input (literal matching, no wildcard injection),
  `ORDER BY title LIMIT ?`; empty/whitespace prefix returns `[]` at the port too, not
  just the handler. `facet_counts(Some(q))` reuses `search()`'s exact MATCH +
  `(?N IS NULL OR col = ?N)` filter clauses minus the LIMIT (filtered match set, not
  the truncated page); `None` aggregates the whole index. Metadata column is the
  same JSON the `index()` path writes (`serde_json::to_string(&doc.facets)`), so
  facet aggregation reads real indexed state.
- **PostgreSQL**: mirrors `search()` verbatim (`websearch_to_tsquery`, `tsv @@ q`,
  `ILIKE ... ESCAPE`, `metadata::text`) minus the LIMIT.
- **Mem**: mirrors mem `search()` filters exactly (`score_doc` over lowercase terms
  + entity_type/workspace filters), limit ignored for aggregation — the port
  contract holds in code where there is no schema (`check-mem-port-contracts.sh`
  passes).
- **Handlers** (`api/search.rs`): `suggest` clamps limit to 50 (default 10), returns
  `{"suggestions":[{text,entity_type,entity_id}]}`; `facets` returns whole-index
  `{"facets":{...}}`; `search_handler` now issues a second `facet_counts(Some(q))`
  call with the same filtered query — the spec's "counts reflect the filtered result
  set" is implemented as a real parallel aggregation, not page-derived counts.
- **Routing/auth**: routes registered `api/mod.rs:616-617`; ABAC resolver entries
  `abac_middleware.rs:377-379` (`check-abac-route-registry.sh` passes); both sit
  behind `require_auth_middleware` in `build_router` per the task's auth
  requirement; reindex stays admin-only at `/api/v1/admin/search/reindex`.
- **Tests are kill tests, not mirrors**: `search_wiring.rs` drives the endpoints
  through the real `build_router` + auth middleware over a temp-file SQLite FTS5
  index: whole-index vs filtered-set aggregation are distinguished
  (`facets["type"]["task"]==2` vs `fc["type"]["task"]==1` — kills a whole-index
  regression in `/search`'s `facet_counts`), prefix vs mid-title ("security" must
  NOT match "Identity security"), empty prefix → empty list, no-token → 401 on both
  new endpoints. Adapter suites add LIKE-wildcard escaping kills (`100%`, `a_`
  literal), limit-ignored aggregation, entity_type/workspace filter restriction,
  and non-matching-query-aggregates-nothing.
- **Attribution/bookkeeping**: all three task-labeled commits are in the `commits:`
  frontmatter (`check-task-commit-attribution.sh` passes); coverage rows 13/14
  updated honestly (row 14 still discloses the missing `url` result field rather
  than claiming full closure).

## Offline checks run (evidence in /tmp/stage/review-evidence/)

- `scripts/check-rustfmt-diff.py d81b0fe4` → **exit 1** (the finding above)
- `rustfmt --check` per touched file → clean except `gyre-ports/src/lib.rs`
- `scripts/check-abac-route-registry.sh` → exit 0
- `scripts/check-arch.sh` → exit 0
- `scripts/check-mem-port-contracts.sh` → exit 0
- `scripts/check-task-commit-attribution.sh` → exit 0

## Non-blocking observation (pre-existing pattern, out of task scope)

`/search`, `/search/suggest`, and `/search/facets` all aggregate/query the index
without tenant scoping (the FTS table stores `tenant_id` but no filter uses it) —
the new endpoints inherit the pre-existing `/search` scope model exactly, which is
what the task plan specified ("same as the existing search endpoint"). The coverage
matrix already tracks the separate MCP-integration scoping gap (row 15). Worth a
follow-up task if §Access Scoping is meant to bind these read paths.
