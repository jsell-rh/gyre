# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (Simple Search, Quoted Phrases, Faceted Filtering).
Candidate: `5b1000261c71857c712d81dc73c27c439cebfc8b` (base `199c1bb78f53f27088c54a0929b5605e7302e874`).
Product commits: `c8f14874c` (parser + port + adapters + REST + MCP), `40d1c1cde` (PG tsv-predicate restore), `5ace1b3ac` (test initializers after access-scoping merge), `fc0c24e8b`/`2c3eff661` (rustfmt repairs). All recorded in `specs/tasks/task-154.md` `commits:` frontmatter at the candidate commit.
Verdict: **approved** — no findings. Evidence below; the two server-package test binaries the sandbox wrapper blocked are recorded as verifier probes (§Verifier probes).

## Probes run (checkout on the assigned branch, candidate checked out)

- `python3 scripts/check-rustfmt-diff.py 199c1bb…` → `rustfmt: changed lines clean (13 Rust files checked)`, exit 0. (The prior verification finding at `ccba82aa` — changed-line rustfmt violations — was repaired by `2c3eff661`; confirmed fixed at the candidate.)
- `bash scripts/check-arch.sh` → passed (gyre-domain has no forbidden deps/I/O; the new `search_query` module is pure).
- `/tmp/stage/bin/cargo test -p gyre-domain search_query` → **25/25 passed** (terms, phrases, facets, multi-value OR, malformed-facet drop, unknown-key fallback, case-insensitive keys, relative/absolute/leap-day/invalid time bounds, first-value-wins).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib search` → **35/35 passed** (sqlite FTS5: phrase adjacency, facet AND/OR composition, time bounds, facet-only browse, facet_counts honoring filters, access scoping, porter-stemming proof; mem: same query-language contract in code; postgres: SQL-shape regression tests).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → Finished, exit 0 (type-checks the handler, MCP tool, wiring and MCP test binaries).
- Mutation probes (temporary edits, restored, re-ran the full 35 to green):
  - Mutation A — `build_match_expr` degrades a phrase to individual quoted terms → `sqlite::search::tests::quoted_phrases_match_exactly_not_individual_terms` **FAILED** (`phrase must require adjacency: ["adjacent", "scattered"]`). The phrase test kills the "phrases as terms" bug.
  - Mutation B — `build_filters` drops all facet filters (`.take(0)`) → `sqlite::search::tests::facet_filters_and_or_within_compose` **FAILED**. The facet test kills filter non-enforcement.
- Mechanical gates: `check-byte-slice-truncation`, `check-mem-port-contracts`, `check-mcp-write-tools`, `check-abac-route-registry`, `check-fail-open-ref-resolution`, `check-inert-enforcement`, `check-forged-scope-fields`, `check-in-memory-state-stores` → all OK.
- FTS5 injection-safety probe: Python `sqlite3` (SQLite 3.50.4) confirmed quoted phrases containing arbitrary punctuation (`(`, `*`, `:`, `^`, `-`, `…`) are parsed as literals (no syntax error, no match); only a literal `"` inside a phrase breaks the MATCH string — and the parser structurally cannot produce one (a phrase terminates at the first closing quote). Terms are alphanumeric-only by construction (`take_token` splits on non-alphanumeric). No user text reaches either MATCH/`websearch_to_tsquery` input as syntax.

## Verified working

- **Parser (`crates/gyre-domain/src/search_query.rs`)**: real, pure, forgiving per contract. Quoted-span extraction, first-colon facet split (`assigned:agent:worker-42` → value `agent:worker-42`), whitelisted 12 facet keys matching the spec table (type/workspace/repo/status/priority/assigned/author/label/approval/scope/since/until), malformed `type:` dropped, unknown keys fall back to terms, `since:`/`until:` resolve relative (`7d`/`24h`/`2w`, overflow-checked) and absolute `YYYY-MM-DD` (validated month/day incl. leap years, Hinnant `days_from_civil`) to inclusive epoch bounds.
- **SQLite adapter**: FTS5 `MATCH` built exclusively from quoted literals and bound as a parameter; facets are real WHERE clauses (`entity_type`/`workspace_id`/`repo_id` IN-lists; metadata via `json_extract(metadata, ?)` with the key bound and the key set parser-whitelisted); `since:`/`until:` filter on the persisted timestamp (reserved `updated_at` metadata key — FTS5 tables cannot be ALTERed, documented); AND-across/OR-within composition is in the SQL itself, so filters apply before LIMIT; facet-only queries browse without a MATCH clause.
- **Mem adapter**: the same contract enforced in code inside the query filter (phrases as exact case-insensitive substrings, facets, time bounds) — not a stub.
- **PostgreSQL adapter**: parameterized (`$N::text`/`$N::bigint`) filter builder shared by `search()`/`facet_counts()`; `websearch_to_tsquery` reconstruction is inert as syntax (same parser guarantees); the full-text row filter `s.tsv @@ q` is present (restored by `40d1c1cde` after a review round found it dropped) and guarded by mutation-verified SQL-shape tests; placeholder numbering ($1 reserved for the FROM-clause tsquery) is tested.
- **REST handler (`GET /api/v1/search?q=`)**: parses `q` first; `type:`/`workspace:` facets take precedence over the `entity_type`/`workspace_id` params (params become fallbacks — exactly the task contract); access scope still derived from the authenticated caller (task-203 composition preserved). `facet_counts` in the response aggregates the same filtered match set.
- **MCP `gyre_search`**: same parse + precedence + authenticated scope; tool description documents the grammar. Integration tests exist and type-check (`cargo check -p gyre-server --tests`).
- **Indexers** pass real entity update times (`tasks.rs`/`agents.rs`/`merge_requests.rs`: `updated_at: Some(...)`), not placeholders.
- **Coverage matrix** rows 11/12 → `implemented` with accurate notes, including the honest caveat that the PG leg has no in-repo runtime harness (SQL-shape tests only) — pre-existing repo condition (no PG test harness at all), not fakery; the SQL is real and parameterized.
- No stubs (`bail!`/`todo!`/`unimplemented!`) in the diff; no new routes (ABAC registry unaffected); `gyre_search` remains read-only (write-gate check passes).

## Test-inflation check

None found. The phrase test distinguishes adjacency from both-words-present (scattered-doc control); the facet test distinguishes filtered from unfiltered result sets; the precedence tests distinguish override from AND-composition (would return 0 results if ANDed); both mutations above were killed. The PG shape tests assert the generated statement contains the row-filter predicate in WHERE position — the exact regression that occurred — and were previously shown to fail without it.

## Verifier probes (blocked in this sandbox by the 4 GiB build wrapper — not code defects)

The executor wrapper rejects gyre-server test compilation (`cargo test -p gyre-server` → exit 64, "Server builds/tests belong to the independent verifier"). These compiled clean under `cargo check -p gyre-server --tests` and must be executed remotely:

1. `cargo test -p gyre-server --test search_wiring` — HTTP leg: `q=%22count%20quokkas%22` phrase adjacency through the middleware stack, facet narrowing, facet-over-param precedence, since:/until: relative + absolute.
2. `cargo test -p gyre-server --lib mcp_search_query_language` — MCP leg: quoted phrase + facet filter + `type:` overriding the `entity_type` arg.
3. If a live PostgreSQL is available: run the search integration against a PG-backed deployment (the PG adapter has no runtime harness in-repo; SQL-shape tests only) — at minimum, one text query, one facet query, and one facet-only browse through `SearchPort` on PG.
4. Standard remote suite (`cargo test --all`, web `npm ci && npm test`).

## Notes

- The working tree's `specs/tasks/task-154.md` (progress reverted to `not-started`, empty `commits:`) is the review-stage harness reset, not candidate content; the candidate commit `5b100026` carries `ready-for-review` + the commit list. Untouched by this review.
- Relative `until:7d` resolves to `now − 7d` as the inclusive upper bound ("until 7 days ago"). The spec only exemplifies absolute `until:`; the interpretation is documented in the parser and is defensible — not a defect.
- No `web/` source changes in this candidate (docs only), so no frontend probes were needed.
