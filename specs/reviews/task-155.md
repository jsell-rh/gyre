# Review — task-155 (Implement gyre search CLI command)

Spec: `specs/system/search.md` §CLI (the six `gyre search` invocations with `--type`, `--status`, `--workspace`, `--since`, `--suggest`).
Candidate: `751aaf81` on base `770785f7` (merge-base verified = base). Product commits scoped by `commits:` frontmatter: `2b6f3372` (search command, +417 main.rs / +70 client.rs / docs), `4c0df440` (baseline-gate repairs: rustfmt + starter-kit relative-path fix), plus the four preserved sandbox-attempt checkpoints (`1292303a`, `951037f8`, `0196a149`, `4e5b20d2`, `6bc9a54d`) — all listed and attribution-checked.
Verdict: **complete**.

## Round 1 — independent review (this review)

All probes run in an isolated worktree at the candidate commit (verified byte-identical to `751aaf81` for both changed CLI files via md5; worktree clean after every mutation probe — `git status --short` empty; both checkouts left clean at `751aaf81`).

### Focused tests
- `cargo test -p gyre-cli --bin gyre` → **110 passed, 0 failed** (re-run green after each mutation restore).
- Search subset: 3 CLI parse tests (`cli_search_parses`, `cli_search_faceted_parses` covering all flags, `cli_search_suggest_parses`), `parse_since_*` units (relative units, junk rejection, multibyte no-panic regression, before-epoch, ISO dates, invalid calendar dates), `result_matches_*` filter predicate tests (status, since boundary, AND composition, fail-closed on unresolvable state), `suggest_*` prefix tests.

### Mutation probes (each reverted, suite re-run green after)
1. Status comparison disabled (`if let Some(_want) = status_filter {}`) → `result_matches_*` **FAIL (2 tests)**. Tests are anchored to status filtering.
2. Fail-closed `unavailable` check disabled (`if false && ...`) → **FAIL (3 tests)**. Anchored to fail-closed semantics.
3. Since cutoff comparison disabled → **FAIL (2 tests)**. Anchored to recency filtering.

The tests kill real bugs; they are not self-confirming.

### Wire-contract verification (source + serde probe + binary smoke)
- Client `search()` params `{q, entity_type, workspace_id, limit}` match server `SearchParams` (`api/search.rs:17-23`) exactly; response types match `SearchResponse`/`SearchResultItem` (`:29-44`). Verified by source inspection **and** a temporary serde round-trip test deserializing the server's actual Serialize shapes (from `tasks.rs:65-88`, `merge_requests.rs:72-94`, `agents.rs:36+`) into the CLI structs — passed, then removed.
- The client-side `--status`/`--since` filter depends on `updated_at`/`spawned_at` being present in the detail responses: confirmed all three fields are non-`Option`, always serialized (no `skip_serializing_if`) — the `#[serde(default)]` annotations are defense, not a silent-zero risk.
- Binary smoke (dead-server config, HOME overridden): wire URLs prove `q`, `entity_type`, slug pre-resolution (`/workspaces?slug=...`), `limit=N`, and the `max(limit,100)` over-fetch exactly when `--status`/`--since` are active. Exit codes: conn-refused=1, `--since bogus`/`2026-13-01`/`é`=1 (clean error, char-boundary-safe — the multibyte regression test is genuine), no-query=0 (usage hint).
- `--suggest` falls back to regular search + client-side case-insensitive title-prefix filtering — explicitly permitted by the task contract ("for now, fall back to regular search until task-153") and documented in `docs/cli.md`.

### Design decision verified correct (fake-filtering avoidance)
The checkpoint's earlier approach folded `--status`/`--since` into `q` as `facet:value` tokens; under the server's AND-of-terms substring engine (`mem_search.rs:76-124`) that would annihilate all results — fake filtering. The shipped approach (server-side `entity_type`/`workspace_id`, client-side live-state resolution via `GET /tasks/:id` / `/merge-requests/:id` / `/agents/:id` with fail-closed drops and stderr accounting) is the real implementation. The live-state design is justified: search index facets freeze at create-time values (index writes run only on entity creation — `tasks.rs:213`, `merge_requests.rs:383`, `agents.rs:130`), and the detail endpoints carry real `updated_at`/status maintained by the server on updates/transitions.

### Gates (all at candidate)
rustfmt changed-lines clean; `check-task-commit-attribution.sh` OK; byte-slice-truncation, relative-path-defaults, unbounded-external-http, fabricated-scope-defaults, lossy-secret-conversion, inert-enforcement, scope-literal-defaults, in-memory-state-stores, fail-open-ref-resolution, forged-scope-fields, forwarded-header-trust — all OK; architecture lint OK. Clippy: only the pre-existing `gyre-common` loop-counter warning (`view_query.rs:798`, present at base).

### Sandbox limitation (infrastructure, not a code defect)
`ws_integration` fails in this sandbox: TCP `accept()` → errno 95 (capabilities.json). Reproduced identically at **base** `770785f7` (base tree checked out and re-run) — not candidate-caused. Live E2E HTTP against a real `gyre-server` cannot run here. Host verification should run: `gyre search "identity"`, `gyre search "ABAC" --type task --status in_progress`, `gyre search "x" --since 7d`, `gyre search --suggest iden`, `gyre search "budget" --workspace <slug>` against a live server; GitHub CI e2e covers the live path.

### Non-blocking notes (recorded for hygiene; no behavioral impact)
1. `4c0df440` removed the starter-kit `PathBuf::from(&repo_name)` relative-path default (fixed to a hard error — correct fix) but left the now-stale exemption entry `crates/gyre-cli/src/main.rs:1737` in `scripts/relative-path-defaults-exemptions.txt` (with its "task-099 revision round owns this" comment). The file's own rule says "when you fix one, DELETE its exemption line"; task-210's precedent deleted its three. Today the entry exempts the unrelated `client_api =` line; a future violation landing exactly at line 1737 would be silently exempted. Mechanical one-line cleanup for the next task touching that file; no gate currently fails.
2. The comment/stderr note "spec/commit have no detail endpoint yet" is inaccurate for specs — `GET /api/v1/specs/:path` exists (`specs.rs:324`, returns `approval_status` + `updated_at`). Behaviorally moot: specs are not indexed by the search backend at all (exactly 3 index sites, all task/mr/agent), so no spec result can reach `live_entity_state`, and the fail-closed drop is correct for a type that cannot appear. Comment-only inaccuracy.
3. `--workspace` currently drops all task/mr/agent results because all three index sites write `workspace_id: None` — pre-existing server gap documented in coverage row 6, owned by task-203 (access scoping). The CLI sends the param correctly; the client cannot fix the server's index.

### Acceptance criteria
- `gyre search "query"` calls the search API and displays results — **yes** (wire URL verified; print format `[type] title (id)` + indented snippet, `N shown of M total` header).
- `--type`, `--workspace`, `--status`, `--since` flags filter results — **yes** (type/workspace server-side params; status/since client-side against live state, mutation-proven).
- `--suggest` returns autocomplete suggestions — **yes** (documented fallback per contract).
- Results formatted as readable table with entity type, title, and snippet — **yes**.
- `--limit` controls result count (default 20) — **yes** (wire-verified; server cap 100 respected via over-fetch/truncate logic).
- CLI parse tests pass — **yes** (3 tests, all passing).

Evidence: `/tmp/stage/review-evidence/` (summary.md indexes all probe commands, sources, exit codes, and outputs).
