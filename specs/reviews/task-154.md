# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted Phrases, §Faceted Filtering).
Candidate under review: `ccba82aa85d74e7f190c1b151fc561e39dc633c8` (branch
`pipeline/task-154/6d773e3942144aeab217c581157a4447-1`), diff against assigned base
`199c1bb78f53f27088c54a0929b5605e7302e874`.
Attributed commits: `c8f1487` (feature), `40d1c1c` (PG predicate restore), `fc0c24e`
(rustfmt), `5ace1b3` (test-initializer compile fix after the task-203 rebase merge
`dbe5102`). All four exist in history; the task-labeled product-surface commits are
exactly the listed set (`dbe5102` is a merge commit, out of the attribution script's
non-merge scope, and its content is the combination reviewed below).
Verdict: **approved** — no findings. Independent evidence below.

## Round 3 (repair review: compile fix `5ace1b3`)

Probes run (this sandbox; evidence saved under `/tmp/stage/review-evidence/`, see its
`README.txt`):

- `cargo test -p gyre-domain search_query` (via `/tmp/stage/bin/cargo`) → **25 passed**
  (all parser patterns: simple terms, quoted phrase extraction, facets, mixed, multiple
  values OR-composition, malformed facets ignored, unknown keys fall back to terms,
  relative/absolute time bounds incl. leap day and invalid-value handling)
  (`domain.txt`).
- `cargo test -p gyre-adapters --lib search` → **35 passed** (SQLite FTS5 behavioral
  suite incl. `quoted_phrases_match_exactly_not_individual_terms`,
  `facet_filters_and_or_within_compose`, `time_facets_since_and_until_relative_and_absolute`,
  `facet_only_query_browses_without_text`, `facet_counts_honor_query_language_filters`;
  mem adapter mirror; PG SQL-shape guards) (`adapters.txt`).
- `cargo check -p gyre-server --lib --tests` → **exit 0** (cold run 5m52s in this
  sandbox; cached rerun captured in `check.txt`) — confirms the repair round's
  claim: all server test targets compile, including `mcp_search_query_language`
  and the task-203 helper that previously hit E0063.
- **Mutation probe (test-strength proof):** temporarily degraded
  `build_match_expr` in `sqlite/search.rs` to split quoted phrases into
  individual terms (simulating the "phrases not searched as exact phrases"
  defect) → `quoted_phrases_match_exactly_not_individual_terms` **FAILED**
  (`left: ["adjacent", "scattered"], right: ["adjacent"]`), exit 101
  (`mutation-phrase.txt`; baseline pass `baseline-phrase-test.txt`, post-restore
  pass `restored-phrase-test.txt`). File restored via `git checkout --`;
  working tree verified clean afterwards. The headline acceptance test kills a
  real bug, not a mirrored assertion.
- Mechanical invariants: `check-arch.sh` (ports→domain re-export of the parser is
  pre-existing layering, domain stays infrastructure-free), `check-byte-slice-truncation.sh`,
  `check-inert-enforcement.sh`, `check-mcp-write-tools.sh` (gyre_search is read-only,
  correctly outside the `needs_write` gate), `check-mem-port-contracts.sh`,
  `check-task-commit-attribution.sh` → all OK.

Verified against the task contract:

- **Parser is real and in the right layer.** `gyre-domain/src/search_query.rs` (551
  lines, pure logic): terms split on non-alphanumeric boundaries mirroring the FTS
  `unicode61` tokenizer, quoted-span phrase extraction (unterminated quote runs to
  end, empty spans dropped), facet whitelist matching the *full* spec table exactly
  (type/workspace/repo/status/priority/assigned/author/label/approval/scope/since/until
  — the excerpt's 8-key subset understates the spec; the implementation matches
  `search.md`'s 12-key table). Forgiving per the agent instructions: `type:` with no
  value is dropped, unknown `key:value` falls back to terms, invalid time values
  resolve to no bound. `since:`/`until:` support `7d`/`24h`/`2w` relative and
  `YYYY-MM-DD` absolute (start-of-day for since, end-of-day inclusive for until) with
  real civil-date math (`days_from_civil`, leap-year validated).
- **All three adapters execute the grammar, not a stand-in.** SQLite FTS5: phrases
  become quoted FTS5 phrases (adjacency enforced by the engine — proven by the
  mutation probe), facets become bound-parameter WHERE clauses (columns for
  type/workspace/repo, `json_extract` on metadata for the rest, `IN (...)` lists for
  OR-within), time bounds on a reserved metadata timestamp key (FTS5 virtual tables
  cannot be ALTERed — documented; pre-migration rows COALESCE to 0, documented).
  PostgreSQL: terms + phrases reconstructed into `websearch_to_tsquery` with the
  `s.tsv @@ q` row filter present in `build_filters` (the round-1 defect, restored by
  `40d1c1c` and guarded by SQL-shape regression tests; no PG runtime harness exists
  in this repo — honestly disclosed in the coverage matrix). Mem adapter: same
  contract via `scope_visible` + `score_doc` with exact-phrase substring semantics.
- **AND-across / OR-within composition is genuine** — separate WHERE clauses per
  facet key ANDed; `IN` lists within a key; behavioral tests assert exact result ID
  sets for both compositions.
- **Integration is real on both surfaces.** REST `GET /api/v1/search?q=` parses `q`
  first; `entity_type`/`workspace_id` params remain fallback shortcuts with
  query-string facets taking precedence (exercised over HTTP in
  `tests/search_wiring.rs`: `q=quokkas%20type:task&entity_type=mr` must return the
  task-typed results). MCP `gyre_search` parses `q` with the same precedence rules;
  tool description and `docs/api-reference.md` document the grammar.
- **Access scoping (task-203) is preserved, not bypassed.** Scope is derived from the
  authenticated caller in both handlers, never from the query string;
  `SearchQuery::parse` starts fail-closed (empty tenant, empty accessible sets) and
  scope is attached via `with_scope`; all adapters AND scope clauses into the same
  index query (never post-filtered). Facet-only queries browse only within scope.
- **No test inflation found.** Parser tests assert exact structures; adapter tests
  assert exact result sets including negative cases (non-matching facet value →
  empty, not everything); the facet_counts test proves filtered-out documents are
  not counted and the reserved timestamp key never leaks into user-facing facets.
  The mutation probe confirms the suite detects real behavioral regressions.
- **Coverage matrix is honest.** Rows 11/12 → `implemented` with landed evidence and
  explicit disclosure of the PG-runtime-test gap and the SQLite timestamp-in-metadata
  design. Counts updated 6/14 → 8/14 consistently. `specs/tasks/task-154.md` progress
  is `ready-for-review` with an accurate Shipped section (including the E0063 repair
  history).

Non-blocking observations (recorded for completeness, no action required):

- `crates/gyre-ports/src/search.rs` trait doc for `facet_counts` has one duplicated
  line ("Aggregate facet counts over the index (spec §API `GET /search/facets`"
  appears twice) — cosmetic merge artifact from the rebase, no semantic effect.
- Mem-adapter phrase matching is case-insensitive substring rather than token
  adjacency — consistent with that adapter's documented role as the dev/test
  stand-in ("case-insensitive substring matching in place of tokenized FTS"); the
  persistent adapters enforce true adjacency.

Verifier must run (server test compilation deferred per the 4 GiB budget; this
review's cargo check confirms all targets compile):

- `cargo test -p gyre-server --lib mcp_search` (includes `mcp_search_query_language`
  and the task-203 access-scoping test whose initializer `5ace1b3` repaired)
- `cargo test -p gyre-server --test search_wiring` (HTTP-leg query-language tests)
- `cargo test -p gyre-domain search_query` and `cargo test -p gyre-adapters --lib search`
  (re-run; both green in this review's sandbox)

No frontend sources were touched by this task (UI row 17 remains task-assigned to a
separate task), so there is no `web/dist` observation to record.
