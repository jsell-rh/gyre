# Review — task-155 (search.md §CLI — gyre search CLI command)

Spec: `specs/system/search.md` §CLI (six example invocations: simple, --type, --type+--status, --type+--since, --workspace, --suggest).
Candidate: `f69b3b606920e399ce609e121b41d1fbb4cff4c8` (base `8c2d177505852b3e39cd77f4f782fb355de245aa`). Product diff: `2b6f3372` — `crates/gyre-cli/src/main.rs` (+656), `crates/gyre-cli/src/client.rs` (+123), `docs/cli.md` (+40); all six task-155 product commits present in frontmatter `commits:`.
Verdict: **complete**.

## Evidence

Probes under `/tmp/stage/review-evidence/` (PROBES.md is the index). Summary:

- `cargo test -p gyre-cli --bin gyre` → **110 passed, 0 failed**.
- `cargo clippy -p gyre-cli --all-targets` → clean except the pre-existing `gyre-common` loop-counter warning (present at base).
- Binary smoke (candidate build, config pointed at always-refused 127.0.0.1:1): `--help` shows all flags; no-query → usage hint exit 0; `--since bogus`/`2026-13-01`/`é` → clean exit-1 errors (multibyte does not panic); connection failure → exit 1 with reqwest error chain.
- Wire-contract probes (error URLs reveal exact request shape): `-t spec` → `GET /api/v1/search?q=hello&entity_type=spec&limit=20`; `-w slug` → slug→id resolution via `GET /api/v1/workspaces?slug=…` first; `--status X --limit 5` → over-fetch `limit=100` before client-side filtering; `--suggest iden` → `q=iden`; default `limit=20`. All match server `SearchParams` (q, entity_type, workspace_id, limit capped at 100).
- Serde probe (`/tmp/stage/wire-probe`): server-shaped `SearchResponse` JSON deserializes into the CLI's client types; field lists byte-identical to candidate structs; `TaskResponse`/`MrResponse`/`AgentResponse` client fields all exist in the server's Serialize structs (extras ignored, optionals defaulted); status strings snake_case on both sides.
- Mutation probes (break production code → test → restore; tree verified byte-identical afterward): suggest prefix `starts_with`→`contains` KILLED; since boundary `<`→`<=` KILLED; multibyte regression (byte-slice revert) KILLED; calendar validation (Feb 30) KILLED; status case-insensitivity KILLED; client dropping `entity_type` param KILLED by wire probe. One survivor: removing the `unavailable` fail-closed early-return — analyzed below, not a shipped defect.
- Mechanical checks on candidate tree: arch, byte-slice, unbounded-http, fabricated-scope, lossy-secret, inert-enforcement, scope-literal, in-memory-state all EXIT=0. `check-relative-path-defaults.sh` fails at main.rs:1850 — identical code at base (line 1737, Clone starter-kit path), untouched by this diff. `check-task-commit-attribution.sh` fails only for a781ede2/task-210 — pre-existing (ancestor of base); no task-155 attribution failure.

## Verified working

- **All six acceptance criteria met.** Search calls the API and renders `[type] title (id)` + snippet lines with an `N shown of M total` header; `--type`/`--workspace` reach the server as real filter params; `--status`/`--since` filter client-side against live entity state; `--suggest` returns title-prefix suggestions via the documented fallback; `--limit` defaults to 20; parse tests pass.
- **Client-side filtering is honest, not fake.** The implementer correctly rejected the checkpoint's `q`-folding approach (appending `status:X` tokens to the query string — unmatchable AND-terms under the server engine, i.e. guaranteed zero results presented as filtering). Instead `--status`/`--since` resolve live state per result from `GET /tasks/:id`, `/merge-requests/:id`, `/agents/:id` — real endpoints, real state, with over-fetch `max(limit, 100)` bounded by the server cap before filtering. This is the right call given the index facets freeze at create time (server-side write sites are create-path-only: tasks.rs:213, merge_requests.rs:383, agents.rs:130 — pre-existing, owned by task-201/202/203).
- **Fail-closed semantics are genuine**: unresolvable live state (deleted entity, no detail endpoint) is excluded under any active filter and counted on stderr — an unknown status is not a match.
- **`--since` parsing is robust**: relative `<N><s|m|h|d|w>` with checked arithmetic (no wraparound past epoch), strict zero-padded ISO dates with real calendar validation (Feb 30/month 13/pre-1970 rejected), char-boundary-safe multibyte handling with a regression test for a byte-index panic found and fixed during implementation. Test constants independently verified (`date -d @1772323200` → 2026-03-01T00:00:00Z).
- **`--suggest` fallback matches the task contract** — the instructions explicitly permit regular-search fallback until task-153's `/search/suggest` lands, and the limitation is documented in both `docs/cli.md` and the code comment pointing at the future endpoint.

## Sandbox limitation (infrastructure, not a code defect)

This sandbox cannot run TCP `accept()` (errno 95; recorded in `/tmp/stage/capabilities.json`, independently re-confirmed by launching an HTTP mock — connect succeeds, accept/recv fails with connection reset). A live round-trip against a real `gyre-server` is impossible here; the pre-existing `ws_integration` test fails identically at base and candidate for the same reason. Substitute verification performed: wire-param probes, serde-level response-shape probes, and full source match of client params/types against the server's `SearchParams`/response structs. Host/CI checks recorded in PROBES.md §5 (start server, create entities, exercise each flag end-to-end; ws_integration must pass on CI).

## Minor (non-blocking, recorded for completeness)

- Removing the `unavailable` early-return in `result_matches_filters` is not caught by any test (mutation M2 survived). It is behaviorally load-bearing only at the epoch boundary (`--since 1970-01-01` → cutoff 0, where `updated_at=0 < 0` is false and an unresolvable result would slip through the since filter); for any cutoff ≥ 1 and any non-empty status filter, the empty-status/zero-timestamp comparisons drop such results anyway. Shipped code is correct; a `result_matches_filters(None, Some(0), &unavailable)` assertion would close the gap.
- The stderr note says spec/commit "have no detail endpoint yet" — `GET /api/v1/specs/:path` does exist (returns `approval_status`/`updated_at`), but spec/commit documents are never indexed by the current server (only task/mr/agent write sites exist), so the branch is unreachable today and the note is user-facing text, not enforcement. Same for `--workspace`: the CLI correctly sends `workspace_id`, but all three index write sites store `workspace_id: None`, so workspace filtering returns nothing for indexed types today — a server indexing gap (coverage row 6 note), owned by task-201/203, not the CLI.
- `--limit 0` sends `limit=0` (server returns nothing) — degenerate input, unspecified by the task; harmless.
