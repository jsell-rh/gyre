# Review — task-153 (Implement search autocomplete and facets endpoints)

Spec: `specs/system/search.md` §API (endpoint table, lines 135–142) and
§Response Format (facet_counts, lines 144–168). Task:
`specs/tasks/task-153.md` (commits `774499be`, `27503257`, `60493e27`,
`6da69d14`; review base `d81b0fe4`, candidate `817e843a`).
Verdict: **complete**.

## Round 10 (this review)

Sandbox restriction (recorded, not a code defect): this review sandbox has no
route to crates.io (`cargo metadata` fails: "Failed to connect to
index.crates.io:443"; `/tmp/cargo` holds only a 4.9 MB partial index, no
cached `.crate` files, no prebuilt `target/`), so `cargo test`/`cargo clippy`
cannot execute here. Remote verification must run:

    cargo test -p gyre-server --test search_wiring
    cargo test -p gyre-adapters search
    python3 scripts/check-clippy-diff.py d81b0fe4ab93abff06089432e4a8fca5be6e5d31

Evidence: `/tmp/stage/review-evidence/` (network refusal log, SQL probe script
+ output, offline check logs).

### What ran here (all passing)

- **Real-SQLite behavioral probe** of the adapter's exact SQL semantics
  (`fts5_setup.sql` + `probe_sqlite.out`): built the specced FTS5 virtual
  table (verbatim `FTS5_DDL`, `tokenize='porter unicode61'`) with the test
  fixture documents and executed the adapter's actual query shapes:
  - `title LIKE 'iden%' ESCAPE '\'` → exactly the two identity-prefixed
    titles, ordered `Identity security` < `identity provider audit` (binary
    collation), and `LIKE 'IDEN%'` matches both — ASCII case-insensitive
    prefix match confirmed.
  - Mid-title term (`security%`) → 0 rows: suggest is a prefix match, not
    substring.
  - Escaped wildcards: `LIKE '100\%%' ESCAPE '\'` matches only
    `100% coverage task` (not `100 percent plan`); `LIKE 'a\_%'` matches only
    `a_b literal` (not `axb wildcard`) — `escape_like` genuinely neutralizes
    user-supplied `%`/`_`/`\`.
  - Whole-index aggregation (`facet_counts(None)` shape): task=7, mr=1,
    spec=1; workspace ws-a=2.
  - Filtered aggregation (`facet_counts(Some(q))` shape, same `MATCH` +
    filter WHERE clauses as `search()` minus LIMIT): query "jwt" → task=2,
    mr=1; status in_progress=2/done=1; `entity_type='mr'` filter → mr only;
    `workspace_id='ws-a'` filter → task=1; non-matching query → 0 rows.
  These are exactly the values the new adapter and wiring tests assert, so
  the tests' expected semantics are independently confirmed against a real
  FTS5 engine.
- `python3 scripts/check-rustfmt-diff.py d81b0fe4` → exit 0, 9 changed Rust
  files clean (also proves every changed Rust file parses; round-9 finding
  fbb29ab8 — the 103-char re-export — is genuinely fixed in `6da69d14`).
- `bash scripts/check-abac-route-registry.sh` → OK (both new routes have
  `RouteResourceMapping` entries; resolver `resolve()` is exact-pattern
  match, so no `/api/v1/search` prefix shadowing).
- `bash scripts/check-api-auth.sh` → OK.
- `bash scripts/check-arch.sh` → OK (ports layer unchanged in boundary kind;
  serde already a gyre-ports dependency).
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-task-commit-attribution.sh` → OK (all four product
  commits in the `commits:` frontmatter).
- `bash scripts/check-cross-surface-parity.sh` → exit 2, **pre-existing and
  unrelated**: the script's `grep -v '^\s*$'` on an exemptions file
  containing only comments/blanks exits 1 under `pipefail`; the exemptions
  file last changed in `cd6920d9`, an ancestor of the review base, and this
  candidate touches no `web/` files.

### Verified against the task contract (no findings)

- **§API endpoint set complete.** `GET /api/v1/search/suggest` and
  `GET /api/v1/search/facets` registered (`api/mod.rs:616-617`) behind the
  standard auth+ABAC middleware with resolver entries
  (`abac_middleware.rs:377-379`); `/api/v1/search` gains `facet_counts`;
  reindex remains admin-only at `/api/v1/admin/search/reindex`. Suggest
  defaults limit 10 (max 50), trims, and returns `[]` for an empty/whitespace
  prefix at both handler and port level (the empty LIKE pattern would
  otherwise match every title).
- **SearchPort::suggest / facet_counts are real port methods with no default
  impl** — all three implementors (SQLite FTS5, PostgreSQL tsvector, mem)
  provide them; `SearchQuery` derives `Clone` so the handler's
  search-then-aggregate pair is sound. SQLite facet aggregation reuses
  `build_match_expr` + the identical filter WHERE clauses of `search()`
  (porter-stemmed AND-of-quoted-terms, so the match set is the same one the
  results come from), minus LIMIT — page truncation cannot shrink the counts.
  PG mirrors its own `search()` via `websearch_to_tsquery` + the same
  filters. Mem mirrors `score_doc` all-terms semantics. The `ESCAPE '\\'`
  Rust literals unescape to a single-backslash SQL escape char, valid on both
  SQLite and PG (`standard_conforming_strings` default).
- **facet_counts semantics match the spec example shape**: built-in `type`
  (from entity_type) and `workspace` (from workspace_id) facets plus document
  facets (status/priority/...) — `{type: {...}, workspace: {...},
  status: {...}}` exactly as `search.md` lines 162–166.
- **Tests are hard, not self-confirming.** `search_wiring.rs` endpoint legs
  ride the full middleware router oneshot against the real SQLite-file FTS5
  backend: prefix match + text/entity fields, empty-prefix and
  non-matching-prefix empty lists, 401 without a token on both new
  endpoints, `/search` facet_counts over the filtered match set (whole-index
  regression would yield task=2, not the asserted 1), `/facets` whole-index
  counts (task=2 there — the two assertions cross-pin the filtered vs
  whole-index distinction). Adapter suites kill the remaining modes:
  mid-title non-match, LIKE-wildcard escaping (`100%`/`a_` literal),
  limit-ignored aggregation (limit=1 still counts 3 matches),
  entity_type/workspace filter restriction, non-matching query aggregates
  nothing. Expected values independently confirmed by the SQL probe above.
- **No fakery indicators.** No `bail!` stubs, no hardcoded counts, no
  audit-only paths; both new reads execute real SQL against the persistent
  index; docs (`docs/api-reference.md`) and coverage rows 13/14 updated with
  honest notes (row 14 explicitly keeps "Still missing vs spec example: url
  field on results" — outside this task's acceptance criteria).

### Non-blocking observations (recorded for downstream tasks, not defects)

- Suggest/facets inherit the unscoped cross-tenant index behavior of the
  pre-existing `GET /api/v1/search` (client-side optional filters only);
  `search.md` §Access Scoping is owned by task-203, which was not in scope
  here. Fixing it here would have duplicated task-203's work.
- The PostgreSQL `suggest`/`facet_counts` legs are compile-checked only — the
  repo has no PostgreSQL test infrastructure at all (no `cfg(test)` module in
  `postgres/search.rs`, no PG service in CI); this is the pre-existing
  condition of the entire PG adapter, not a regression introduced here.
- SQLite `LIKE` case-insensitivity is ASCII-only (commented in the adapter);
  PG `ILIKE` and the mem adapter fold full Unicode. Divergence only for
  non-ASCII prefixes, cosmetic for autocomplete.
