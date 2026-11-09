# Review — task-153 (Implement search autocomplete and facets endpoints)

Spec: `specs/system/search.md` §API (4-endpoint table) + §Response Format
(`facet_counts`). Assignment: base `fefde2946`, candidate `16a85a3de`
(merge repair round 12; product files byte-identical to round-11 head
`12a36efd` — verified by empty `git diff 12a36efd9..16a85a3de` on all six
task-153 product files). Verdict: **complete**.

## Environment

This sandbox cannot run cargo (crates.io HTTP 000, no registry cache, no
vendored deps, no prebuilt target — `/sandbox/workspace/.cargo` and
`/sandbox/workspace/.gyre-target` absent; raw log
`/tmp/stage/review-evidence/crates-io-probe.txt`). That is an infrastructure
restriction, not a code defect. Remote verification must run:

    cargo test -p gyre-server --test search_wiring
    cargo test -p gyre-adapters search

## Independent probes run here (raw logs in /tmp/stage/review-evidence/)

- **FTS5 behavioral probe** (`fts5-like-probe.txt`, sqlite3 3.45.1 CLI):
  recreated the adapter's exact FTS5 DDL and ran the adapter's exact query
  shapes. Confirmed: `title LIKE 'prefix%' ESCAPE '\'` works on the fts5
  virtual table (ASCII-case-insensitive, prefix-only — mid-title terms return
  0 rows), wildcard escaping is real (`100\%%` matches only the literal `%`
  title; `a\_%` only the literal `_`), `ORDER BY title` yields binary
  collation exactly as the adapter test's ordering assertions expect, and the
  facet filtered-set WHERE shape (`MATCH + (? IS NULL OR col = ?)`) returns
  precisely the filtered match set. Every expected value the SQLite adapter
  tests assert matches real SQLite behavior.
- **Mechanical checks** (all exit 0): check-arch.sh, check-abac-route-registry.sh
  (both new routes resolve to ("search", None)), check-task-commit-attribution.sh,
  check-rustfmt-diff.py fefde294, check-mem-port-contracts.sh, and the
  byte-slice/relative-path/scope-literal/fabricated-scope/lossy-secret/
  dead-message-kinds/fail-open-ref/forwarded-header/forged-scope/inert-
  enforcement battery (invariant-scripts.txt).
- **Merge integrity**: product files byte-identical to round-11 head;
  base..candidate diff is exactly the task's changes (candidate descends from
  base); the docs/api-reference.md conflict resolution preserves the base's
  task-098 secrets rows.

## Verified working (no findings)

- **All four §API endpoints real.** `GET /api/v1/search` (existing, now with
  `facet_counts`), `GET /api/v1/search/suggest?q=&limit=` (default 10, max 50;
  empty/whitespace prefix → empty list at both handler and port level),
  `GET /api/v1/search/facets` (whole-index aggregation), POST reindex
  (admin-only at `/api/v1/admin/search/reindex`, untouched). No stubs, no
  bail!()s, no hardcoded values.
- **Port contract enforced by every implementor.** `SearchPort::suggest` and
  `SearchPort::facet_counts(Option<SearchQuery>)` have no default impl; all
  three implementors (SQLite FTS5, PostgreSQL tsvector, Mem) provide them with
  the same contract: case-insensitive title-prefix match; `Some(q)` aggregates
  q's filtered match set (terms + entity_type/workspace_id filters, `limit`
  ignored), `None` aggregates the whole index. The SQL adapters escape
  LIKE/ILIKE wildcards in user prefixes and mirror `search()`'s proven
  parameterized WHERE shapes; PG reuses the identical
  `websearch_to_tsquery` cross-join `search()` already uses.
- **`facet_counts` semantics match the plan**: computed over the filtered
  match set (second port call with the same query, limit ignored), not the
  truncated page and not the whole index — pinned by adapter tests
  (limit=1 aggregation case, entity_type/workspace restriction cases,
  non-matching-query-aggregates-nothing case) and by the wiring test.
- **Auth/ABAC**: both routes registered in `api_router()` (api/mod.rs:617-618),
  which `build_router()` wraps with require_auth → last_seen → abac
  (lib.rs:660-672); ABAC resolver entries added (abac_middleware.rs:380-381);
  registry check passes; wiring test carries 401-without-token legs for both
  endpoints.
- **Tests are hard, not self-confirming.** Adapter suites assert specific
  counts/IDs with kill conditions (mid-title term must NOT match; unescaped
  wildcards WOULD match — escaping test; limit-ignored aggregation; filter
  restriction; empty aggregation on non-match). The wiring test drives the
  full middleware router via oneshot against the real SQLite FTS5 backend
  (GYRE_DATABASE_URL=sqlite://tmpfile) and asserts concrete JSON values
  (suggest text/type, facet_counts.type.task == 1 over the filtered set,
  facets whole-index counts, 401 legs). The wiring diff is purely additive —
  task-201's legs were not weakened. My sqlite3 CLI probe independently
  confirms the expected values match real SQLite behavior.
- **Coverage scope respected**: only search port/adapters/API/docs/task files
  touched; no gates weakened, no exemptions added, no tests deleted.

Findings:

- (none — blocking)

Non-blocking observations for the record:

1. **Stale process note**: task-153.md's Shipped section says the `commits:`
   frontmatter "now also records the round-12 merge commit 813cbea12", but the
   final process commit `16a85a3de` removed it, leaving only `fe303595`. The
   attribution check passes either way (it scans only non-merge task-labeled
   commits), and review scoping uses the base..candidate diff which covers the
   merge, so nothing is hidden — but the sentence is inaccurate at the
   candidate. Process-note nit; no product impact.
2. **Coverage row 14 labeling**: §Response Format is marked `implemented`
   while the note honestly discloses the remainder ("Still missing vs spec
   example: url field on results"). This follows the file's existing
   convention (rows 15/17 are `implemented` with Partial notes), the task's
   acceptance criteria scope §Response Format to `facet_counts` (which is
   genuinely done), and the remainder stays visible in the note for the
   spec-fidelity auditor rather than being papered over. Flagging for the
   auditor's next pass, not a defect in this task's delivery.
3. **Deferred verification**: the Rust suites have not been executed in any
   recent round's sandbox (offline). The commands above must run in remote
   verification; my FTS5 CLI probe de-risked the SQL semantics, and static
   checks found no compile-level issue (serde dep present in gyre-ports,
   SearchQuery: Clone, all three implementors updated, test-support APIs
   public), but compile+test confirmation remains outstanding by transport
   restriction.
