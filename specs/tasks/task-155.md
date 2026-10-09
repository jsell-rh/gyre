---
title: "Implement gyre search CLI command"
spec_ref: "search.md §CLI"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "search.md §CLI"
commits: ["2b6f337257ce435c3b6064cd1a707e0ae7727ef9", "1292303aa70971d0b93b8ff04564d51b9365c36b", "951037f8ba299c77075fc224e6cf78593b977d05", "0196a149bb5cfd42e6dfadc1e024a0149a45ddf4", "4e5b20d211eb17c7d00be369227dbab2e82b0f34", "6bc9a54dfd5bff0a78d0410ff997196612e02802"]
---

## Spec Excerpt

From `search.md` §CLI:

> ```bash
> gyre search "identity security"                          # Simple search
> gyre search "merge queue" --type spec                    # Search specs only
> gyre search "ABAC" --type spec --status approved         # Faceted search
> gyre search "auth.rs" --type commit --since 7d           # Recent commits touching auth
> gyre search "budget" --workspace platform-team           # Workspace-scoped
> gyre search --suggest "iden"                              # Autocomplete
> ```

## Implementation Plan

1. **Add `Search` variant to CLI Commands enum** (`crates/gyre-cli/src/main.rs`):
   ```rust
   /// Full-text search across all entities
   Search {
       /// Search query (supports quoted phrases and faceted syntax)
       query: Option<String>,
       /// Filter by entity type (spec, task, mr, commit, agent)
       #[arg(long, short = 't')]
       r#type: Option<String>,
       /// Filter by status
       #[arg(long)]
       status: Option<String>,
       /// Filter by workspace slug
       #[arg(long, short = 'w')]
       workspace: Option<String>,
       /// Show results since (e.g., 7d, 2026-03-01)
       #[arg(long)]
       since: Option<String>,
       /// Autocomplete mode — return suggestions for the given prefix
       #[arg(long)]
       suggest: Option<String>,
       /// Maximum results to return
       #[arg(long, default_value = "20")]
       limit: usize,
   },
   ```

2. **Implement search handler** in the CLI match block:
   - If `--suggest` is provided, call `GET /api/v1/search/suggest?q={prefix}` (once task-153 is implemented; for now, fall back to regular search with prefix)
   - Otherwise, call `GET /api/v1/search?q={query}&entity_type={type}&workspace_id={workspace}&limit={limit}`
   - Append `--since`, `--status` as facets in the query string if the query language parser (task-154) is available
   - Format results in a readable table: `[type] title (id) — snippet`

3. **Add API client method** to the CLI's API client struct:
   - `async fn search(&self, query: &str, entity_type: Option<&str>, workspace: Option<&str>, limit: usize) -> Vec<SearchResult>`

4. **Tests**:
   - CLI parse test: `gyre search "identity"` parses correctly
   - CLI parse test: `gyre search --type spec --workspace platform-team "ABAC"`
   - CLI parse test: `gyre search --suggest "iden"`

## Acceptance Criteria

- [ ] `gyre search "query"` calls the search API and displays results
- [ ] `--type`, `--workspace`, `--status`, `--since` flags filter results
- [ ] `--suggest` flag returns autocomplete suggestions
- [ ] Results formatted as readable table with entity type, title, and snippet
- [ ] `--limit` controls result count (default 20)
- [ ] CLI parse tests pass

## Agent Instructions

- Read `crates/gyre-cli/src/main.rs` for the existing CLI command structure — follow the pattern of other commands like `Explore`, `Tasks`, `Status`
- The API client is defined in `crates/gyre-cli/src/client.rs` — add a search method there
- The search endpoint is `GET /api/v1/search` with query params `q`, `entity_type`, `workspace_id`, `limit`
- For the initial implementation, `--suggest` can fall back to regular search until task-153 adds the `/search/suggest` endpoint
- For `--since` and `--status`, include them as query params or as part of the query string facet syntax

## Shipped

Commit `2b6f3372` (branch `pipeline/task-155/a4ff485562464770a5a49734c603bfb8-1`, on checkpoint `5b41fd3e`).

**Behavior:**

- `gyre search <query>` calls `GET /api/v1/search` and prints results as
  `[type] title (id) — snippet` (one per line, `N of M` header).
- `--type/-t` and `--workspace/-w` are enforced **server-side** — the
  client sends `entity_type` and (slug→id-resolved) `workspace_id` params,
  matching `SearchParams` in `crates/gyre-server/src/api/search.rs`.
- `--status` and `--since` filter **client-side against live entity
  state**: search index facets freeze at create time (index writes run
  only on entity creation) and results carry no timestamps, so per-result
  state is fetched from `GET /tasks/:id`, `/merge-requests/:id`,
  `/agents/:id` (over-fetch `max(limit, 100)` before filtering, truncate
  after). Results whose live state is unresolvable (spec/commit types,
  deleted entities) fail closed and are counted on stderr.
- `--since` accepts `<N><s|m|h|d|w>` relative durations and strict
  zero-padded `YYYY-MM-DD` (UTC midnight). Invalid values — unknown
  units, non-padded dates, impossible calendar dates (Feb 30), dates
  before 1970, multibyte junk — are hard errors (exit 1). Multibyte
  input is handled char-boundary-safe (regression test added for a
  byte-index split panic found and fixed during implementation).
- `--suggest` falls back to regular search plus client-side
  case-insensitive title-prefix filtering (documented as pending
  task-153's `/search/suggest` endpoint).
- `--limit` defaults to 20; the server caps at 100.
- Rejected the checkpoint's approach of folding `--status`/`--since`
  into `q` as `facet:value` tokens: the server's engine ANDs every
  whitespace term as a substring match with no facet parser (task-154
  not started), so any flag use returned zero results — fake filtering.
- `docs/cli.md` gained a Search section documenting all flags and the
  client-side/server-side split.

**Test evidence** (saved under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-cli --bin gyre` → **110 passed, 0 failed** —
  includes 3 CLI parse tests, since-parser units (relative, ISO,
  invalid, multibyte, overflow), filter predicate AND-composition/
  boundary/fail-closed, suggest prefix tests.
- `cargo clippy -p gyre-cli --all-targets` → clean except the
  pre-existing `gyre-common` loop-counter warning (present at base).
- Binary smoke: `search --help` shows all flags; exit codes verified —
  conn-fail=1, `--since bogus`=1, `--since 2026-13-01`=1,
  `--since é`=1 (clean error, no panic), no-query=0 (usage hint).
- Mechanical checks: byte-slice-truncation OK, unbounded-external-http
  OK, fabricated-scope-defaults OK, lossy-secret-conversion OK,
  inert-enforcement OK, scope-literal-defaults OK,
  in-memory-state-stores OK. `check-relative-path-defaults.sh` fails
  at `main.rs:1839` — pre-existing at the checkpoint (verified via
  `git stash`), in the unrelated Clone starter-kit path.
- **Sandbox limitation (recorded, not a code defect):** this sandbox
  cannot run TCP `accept()` (errno 95, Operation not supported) —
  bind/connect work, listeners don't — so a live end-to-end run
  against a real `gyre-server` is impossible here. The wire contract
  was verified by source inspection: client params `{q, entity_type,
  workspace_id, limit}` match `SearchParams` exactly; response-type
  fields match the server's Serialize structs. E2E against a real
  server + GitHub CI remain for the verification/publication stage.
