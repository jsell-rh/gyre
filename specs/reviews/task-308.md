# Review — task-308 (SQLite as the default durable storage backend)

Spec: `specs/development/architecture.md` §Storage ("SQLite as the default (simple, vertical scaling, zero ops)").
Candidate under review: `c5167ddd` (branch `pipeline/task-308/de140fd4e4f244feb42b723b93033c13-2`, base `bbcad833`; single product commit `87e08778`, correctly attributed in the task frontmatter; the two other branch commits touch only the task file).
Verdict: **approved** — no findings. Backend selection, harness migration, and documentation all verified by inspection plus a full `cargo check -p gyre-server --tests`; gyre-server test *execution* is blocked by this sandbox's cargo wrapper (4 GiB codegen rule) and is recorded for the independent verifier — an infrastructure restriction, not a code defect.

## Probes run

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (8m27s; includes the new `tests/default_sqlite_storage.rs`).
- `bash scripts/check-relative-path-defaults.sh`, `check-in-memory-state-stores.sh`, `check-arch.sh` → all exit 0.
- `rustfmt --edition 2021 --check` on the new test file → clean.
- `cargo test -p gyre-server --test default_sqlite_storage` and `--lib storage_selection_matrix` → **exit 64, rejected by the sandbox wrapper** ("gyre-server code generation plus the model exceeds 4 GiB"). Deferred to the verifier (list below).
- Evidence: `/tmp/stage/review-evidence/` (summary.md + logs).

## Verified working

- **Default durable SQLite (AC 1).** `StorageSelection::resolve` in `crates/gyre-server/src/lib.rs`: `None` → `SqliteDefault(default_sqlite_db_path())` = `dirs::data_dir()/gyre/gyre.db` — always absolute (XDG data home on Linux, Application Support on macOS, %APPDATA% on Windows), parent auto-created in the default branch only, path `tracing::info!`-logged at startup (telemetry initialized in `main` before `build_state`). `SqliteStorage::new` runs the Diesel migrations + FTS5 DDL (`sqlite/mod.rs:133-141`), and the unchanged `store!`/`store_shared!` macros wire every port to it, so the zero-config default deployment persists all entities. `main.rs` bootstraps the default tenant and seeds policies idempotently after `build_state`, so the API-level round trip works on a fresh file.
- **Memory sentinel (AC 2).** `Some("memory")` matches before the sqlite-path branch, so it is never parsed as a file path; both `pg_db` and `sqlite_db` are `None`, preserving the pure-in-memory sharing semantics (policy store, workspace store, meta-spec bindings, judgment ledger) verbatim — those stores key off "no DB backend", exactly the old unset condition. `default_sqlite_db_path()` is not even evaluated on this branch (lazy in the `None` arm only), so no directory or file is created.
- **No backend-selection regression (AC 3).** `postgres://`/`postgresql://` → PgStorage; any other value → SqliteExplicit with the identical `strip_prefix("sqlite://")` of the old code, including the unchanged no-`create_dir_all` behavior for explicit paths. The new `storage_selection_matrix` unit test pins the whole matrix (sentinel-first, both postgres spellings, sqlite:// and bare paths, unset → absolute default).
- **Harness migration is complete (AC 4).** Mechanical per-function scan of all 121 `build_state` call sites across `crates/gyre-server/tests/*.rs` plus the in-crate `#[cfg(test)]` suites: every call is immediately preceded by the Once-guarded `gyre_server::select_in_memory_storage()`, sets `GYRE_DATABASE_URL=memory` under ENV_LOCK, or (pre-existing sqlite binaries) points at an isolated temp `sqlite://` URL under ENV_LOCK. The `remove_var("GYRE_DATABASE_URL")` sites were checked for the new hazard class (a parallel `build_state` observing unset would now open the operator's real default): every such binary either has a single test or serializes all env-poking tests on ENV_LOCK — no observable unset window. `web/playwright.config.js` webServer.env sets `GYRE_DATABASE_URL: 'memory'` (controller feedback `df78f83918d8549c48c15ea5:playwright-e2e-in-memory-optin`); `.github/workflows/e2e.yml` drives this config; `scripts/e2e-flow.sh` spawns no server; no other harness spawns the binary; gyre-cli tests use a mock server.
- **Docs (AC 5).** `docs/server-config.md` (GYRE_DATABASE_URL row) and `README.md` accurately document the durable default, the default data path, the sentinel, and the unchanged explicit-URL behavior; no stale "unset = in-memory" text remains in docs/.
- **Test quality.** The new `default_sqlite_storage.rs` tests are real regression probes: on the pre-change code `unset_env_boots_durable_sqlite_at_default_path` fails at its first assertion (`state.storage.is_none()`), and `memory_sentinel_is_stateless` fails because "memory" was a sqlite file path. XDG redirection isolates the "default path" into a temp dir; ENV_LOCK serializes the env windows; the binary deliberately avoids the sticky `select_in_memory_storage` sentinel.
- **Scope hygiene.** Diff touches only storage selection, test opt-ins, harness env, docs, and the `dirs` dependency (+ lockfile). No `web/dist/` change (generated output owned by the verifier — no observation to record). Task text honestly leaves acceptance checkboxes unticked pending independent verification.

## Verifier must run (blocked in this sandbox by the cargo wrapper's 4 GiB rule)

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test default_sqlite_storage`
2. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib storage_selection_matrix` (plus the usual `--lib`)
3. Real-binary round trip: isolated `HOME`/`XDG_DATA_HOME`, `GYRE_DATABASE_URL` unset — boot, POST /api/v1/workspaces, full stop, restart, GET by id → 200; `gyre.db` exists under the redirected data home
4. `GYRE_DATABASE_URL=memory`: state lost across restart; no `gyre.db` at the default data path
5. `cd web && npm ci && npx playwright test` (twice, reuseExistingServer) — no durable file at the default data path
6. Standard full workspace suite and remaining mechanical checks
