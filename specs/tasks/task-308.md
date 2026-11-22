---
title: "SQLite as the default durable storage backend (with explicit in-memory opt-in and stateless harness migration)"
spec_ref: "Spec gap spec-f907eda3bd014fe546b838e2 / sqlite-default-storage / 4156114b8040"
depends_on: []
progress: ready-for-review
commits: ["b650a175b7240c163bb7ecd896d60031ee264328", "87e0877842f7527a40bbcbe4951b07bcd341d076"]
---

## Required behavior

Per specs/development/architecture.md §Storage: 'SQLite as the default (simple, vertical scaling, zero ops)'. Today crates/gyre-server/src/lib.rs build_state selects storage purely from GYRE_DATABASE_URL: SQLite only when the variable is set to a non-postgres URL, and with it unset (the shipped/documented default) every port falls back to an in-memory implementation — a server started with `cargo run -p gyre-server` persists nothing across restarts, and docs/server-config.md documents the inverse of the spec ('Unset = in-memory (default, stateless)'). Implement: (1) Default durable SQLite — when GYRE_DATABASE_URL is unset, build_state opens gyre_adapters::SqliteStorage at a platform-appropriate absolute default data path (XDG data home, e.g. ~/.local/share/gyre/gyre.db on Linux, via the dirs crate or equivalent; parent directories auto-created; NEVER a bare relative filename resolved against cwd — see scripts/check-relative-path-defaults.sh). SqliteStorage::new already runs the Diesel migrations; every port must be wired to SqliteStorage through the existing store! macro so the default deployment persists all entities across restarts with zero operator configuration. (2) Explicit in-memory opt-in — the previous stateless behavior is reachable ONLY via an explicit sentinel (e.g. GYRE_DATABASE_URL=memory), filtered out before the sqlite-path branch so it is never parsed as a sqlite file path; the in-memory policy-store sharing semantics for workspace/policy repos (the 'pure in-memory mode' sharing note in build_state) must be preserved under the sentinel. Explicit sqlite://path and postgres:// URLs keep their current selection behavior unchanged. (3) Test-harness migration — every test that relies on the in-memory default must explicitly opt in: the in-process integration tests calling build_state (112 call sites across crates/gyre-server/tests/*.rs) set the sentinel (or an equivalent explicit code path) so no test run writes to the real default data path or the repository working tree; tests that specifically exercise the SQLite default use isolated temp paths. (4) Process-level harness (controller review feedback id df78f83918d8549c48c15ea5:playwright-e2e-in-memory-optin) — update web/playwright.config.js webServer.env to set the explicit in-memory sentinel so the e2e suite keeps running stateless; .github/workflows/e2e.yml drives this same playwright config, and scripts/e2e-flow.sh requires an externally started server (verified: no other harness spawns the server binary). Audit for any other harness that spawns the gyre-server binary and give it the same explicit opt-in. (5) Update docs/server-config.md (GYRE_DATABASE_URL row: default is now durable SQLite at the documented absolute data path; memory sentinel for stateless runs; explicit sqlite:// and postgres:// URLs unchanged) so operator documentation matches the new shipped default.

## Acceptance Criteria

- [ ] With GYRE_DATABASE_URL unset, `cargo run -p gyre-server` boots on SQLite at the default absolute data path (path logged at startup): an entity created via the API before a full server stop/restart (e.g. a workspace via POST /api/v1/workspaces) is retrievable after the restart — a real persistence round-trip against real storage, including migrations having run on the fresh file.
- [ ] With GYRE_DATABASE_URL=memory the server is stateless exactly as before (state lost across restart) and no database file is created at the default data path.
- [ ] Explicit sqlite://<path> and postgres:// URLs select their backends exactly as before (no regression in backend selection or in the in-memory policy-store sharing under the sentinel).
- [ ] No test run creates or modifies a gyre.db/SQLite file in the repository working tree or at the user's real default data path; the playwright e2e webServer runs on the in-memory opt-in — an e2e run creates no durable file at the default path, and repeated local runs with reuseExistingServer behave identically (no state bleed across runs).
- [ ] docs/server-config.md documents the new default, the memory sentinel, and the default data path accurately.

## Verification intent

Boot the real server binary with GYRE_DATABASE_URL unset, create a workspace via POST /api/v1/workspaces, terminate the server, restart it, and GET the workspace by id: on the current code (in-memory default) the GET returns 404 because all state died with the process — the real defect this probe must expose; with the fix the workspace is served from the SQLite default. A second probe: run the playwright e2e suite twice locally with reuseExistingServer and assert no database file appears at the default data path — without the webServer env opt-in the harness silently boots on durable storage outside the working tree.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/architecture.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e`.

[
  {
    "id": "df78f83918d8549c48c15ea5",
    "decision": "gap",
    "requirement_quote": "- **Abstracted** - storage is behind a port (hexagonal architecture). Implementation is swappable.\n- **SQLite** as the default (simple, vertical scaling, zero ops).\n- **PostgreSQL** supported as an alternative for multi-node / enterprise deployments.\n- Do not leak storage implementation details into domain logic.",
    "detail": "Three of the four bullets are satisfied; the 'SQLite as the default' bullet is not, so the section is a gap. (1) Abstraction/swappability: satisfied - every repository trait lives in gyre-ports (e.g. gyre_ports::storage::StoragePort) and AppState wires each port via the store! macro, which selects PgStorage, SqliteStorage, or a mem implementation for the same trait. (2) SQLite as the default: GAP - storage selection is purely environmental: SQLite is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL, and with the variable unset (the shipped and documented default) no durable storage is opened at all; every port falls back to an in-memory implementation, so a server started with 'cargo run -p gyre-server' and no env persists nothing across restarts. docs/server-config.md documents the inverse of the spec: 'Unset = in-memory (default, stateless)'. A fix must also handle process-level harnesses: web/playwright.config.js boots the real server binary with only GYRE_PORT/GYRE_AUTH_TOKEN, i.e. on today's in-memory default - if SQLite becomes the default without an explicit in-memory opt-in for such harnesses, e2e runs silently become durable outside the working tree (controller review feedback on the prior plan's sqlite-default-storage task). (3) PostgreSQL: satisfied - a postgres:// URL constructs PgStorage, preferred by the store! wiring; crates/gyre-adapters/src/postgres/ implements the port traits across 50+ repository modules. (4) No storage leak into domain: satisfied - gyre-domain has no storage dependencies and scripts/check-arch.sh mechanically forbids rusqlite/sqlx/gyre-adapters in the domain crate.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "let db_url = std::env::var(\"GYRE_DATABASE_URL\").ok();",
        "detail": "Storage backend selection is purely environmental: SQLite (SqliteStorage) is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL. With the variable unset - the shipped default - db_url is None and no durable storage is opened at all."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "    macro_rules! store {\n        ($trait:ty, $mem:expr) => {\n            if let Some(ref d) = pg_db {\n                Arc::clone(d) as Arc<$trait>\n            } else if let Some(ref d) = sqlite_db {\n                Arc::clone(d) as Arc<$trait>\n            } else {\n                Arc::new($mem) as Arc<$trait>\n            }\n        };\n    }",
        "detail": "Every repository port in AppState falls back to an in-memory implementation when neither PgStorage nor SqliteStorage is configured: the default deployment is stateless in-memory, not SQLite. This contradicts 'SQLite as the default (simple, vertical scaling, zero ops)'. The abstraction/swappability bullets are satisfied (same port trait, three interchangeable implementations), but the default-storage bullet is unimplemented."
      },
      {
        "file": "docs/server-config.md",
        "quote": "| `GYRE_DATABASE_URL` | _(unset -- in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |",
        "detail": "The operator documentation confirms the default is the inverse of the spec: in-memory is the default and SQLite is an opt-in via an explicit sqlite:// URL, so 'SQLite as the default' is not the shipped behavior."
      },
      {
        "file": "web/playwright.config.js",
        "quote": "      GYRE_PORT: '2222',\n      GYRE_AUTH_TOKEN: 'e2e-test-token',",
        "detail": "The process-level e2e harness boots the real server binary ('cargo run -p gyre-server' locally, the release binary in CI) with only GYRE_PORT and GYRE_AUTH_TOKEN - i.e. on today's in-memory default, with no GYRE_DATABASE_URL. Any change that makes SQLite the default must give such harnesses an explicit in-memory opt-in (controller review feedback on the prior plan's sqlite-default-storage task), or e2e runs silently become durable at the default path outside the working tree."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "let pg_db: Option<Arc<gyre_adapters::PgStorage>> = db_url",
        "detail": "PostgreSQL support is real production wiring: a postgres:// URL constructs PgStorage, which the store! macro prefers over SQLite for every port - satisfying 'PostgreSQL supported as an alternative'."
      },
      {
        "file": "crates/gyre-adapters/src/postgres/mod.rs",
        "quote": "use gyre_ports::storage::StoragePort;",
        "detail": "PgStorage is an adapter implementing the port traits defined in gyre-ports (the postgres/ directory holds 50+ repository modules with 95 'impl ... for PgStorage' blocks), confirming implementation swappability behind the same port abstraction."
      },
      {
        "file": "crates/gyre-domain/Cargo.toml",
        "quote": "# INVARIANT: gyre-domain MUST NOT depend on gyre-adapters or any infrastructure crate.",
        "detail": "No storage implementation details leak into domain logic: gyre-domain's dependency list contains no storage crate (no diesel, rusqlite, or sqlx), and scripts/check-arch.sh mechanically forbids those dependencies in the domain - the fourth bullet is satisfied."
      }
    ]
  }
]

## Shipped

SQLite is now the default durable storage backend, with an explicit `memory`
sentinel for stateless runs and every in-process/process-level harness migrated
to an explicit opt-in. Implementation: commit 87e08778; formatting repair from
verification run 3b0a9f748dfb437ead09614227e5fbb8 (rustfmt on the storage
selection code, no behavior change): commit b650a175.

### Behavior

- `crates/gyre-server/src/lib.rs`: backend selection is a `StorageSelection`
  enum resolved from `GYRE_DATABASE_URL` — `memory` → in-memory (filtered out
  before the sqlite-path branch so the sentinel is never parsed as a file
  path), `postgres://`/`postgresql://` → PgStorage, `sqlite://<path>`/bare
  path → SqliteStorage at that path (both unchanged from before), and **unset
  → durable SqliteStorage at the platform default data path** via the
  `dirs`-based `default_sqlite_db_path()` (`$XDG_DATA_HOME/gyre/gyre.db`,
  default `~/.local/share/gyre/gyre.db` on Linux; `~/Library/Application
  Support` on macOS; `%APPDATA%` on Windows — always absolute, parent dirs
  auto-created, path logged at startup). `SqliteStorage::new` runs the Diesel
  migrations, and every port wires through the unchanged `store!`/
  `store_shared!` macros, so the default deployment persists all entities
  across restarts with zero operator configuration. The in-memory
  policy-store/workspace-store/meta-spec-binding/judgment-ledger sharing
  semantics are preserved verbatim under the sentinel (both `pg_db` and
  `sqlite_db` are `None` exactly as before).
- New `gyre_server::select_in_memory_storage()` (Once-guarded env sentinel) is
  the in-process opt-in used by test harnesses.

### Harness migration

- All in-process harnesses calling `build_state` opt in explicitly: the
  integration test binaries under `crates/gyre-server/tests/` call
  `select_in_memory_storage()` (api_integration, auth_integration,
  conversation_integration, e2e_ralph_loop, explorer_ws_integration,
  git_integration, graph_integration, m18_oidc_integration,
  message_size_and_key_rotation, spec_enforcement, task134_review_probe) or set
  `GYRE_DATABASE_URL=memory` directly; the pre-existing sqlite-exercising
  binaries (dependency_persistence, message_persistence, search_wiring,
  scim_*, task107_*, task163/164, tenant_isolation, workspace_scoping,
  abac_middleware, hierarchy_cascade) already point the variable at isolated
  temp files. The in-crate unit suites (api/attestations.rs,
  gate_executor.rs, merge_attestation.rs) use `select_in_memory_storage()`
  via their `test_state()` helpers. No test run touches the operator's real
  default data path or the working tree.
- `web/playwright.config.js` webServer.env sets `GYRE_DATABASE_URL: 'memory'`
  (controller review feedback
  df78f83918d8549c48c15ea5:playwright-e2e-in-memory-optin);
  `.github/workflows/e2e.yml` drives this same config. Audited all other
  harnesses: `scripts/e2e-flow.sh` requires an externally started server and
  spawns no server binary — playwright is the only process-level spawner.
- `docs/server-config.md` and `README.md` document the new default, the
  `memory` sentinel, and the default data path.

### Test evidence

- New `crates/gyre-server/tests/default_sqlite_storage.rs` (isolated temp
  dirs, XDG_DATA_HOME redirected so the "default path" is a temp dir):
  `unset_env_boots_durable_sqlite_at_default_path` (workspace survives a full
  state rebuild, gyre.db exists at the default path),
  `memory_sentinel_is_stateless` (no durable backend, state lost across
  rebuild, no file at the default path), `explicit_sqlite_url_selects_that_path`
  (explicit URL selects that exact path, durable). Plus
  `storage_selection_matrix` unit test in lib.rs (sentinel filtered before the
  sqlite branch; postgres/postgresql spellings; sqlite:// and bare paths;
  unset → absolute default path).
- Verification run 3b0a9f748dfb437ead09614227e5fbb8: every gate passed except
  rustfmt on three changed lines of lib.rs; repaired in b650a175
  (formatting-only). Post-fix `cargo fmt -p gyre-server -- --check` exit 0
  (see /tmp/stage/review-evidence/rustfmt-repair.md and
  rustfmt-check-after-fix.log).
- Deferred to the independent verifier (large gyre-server test compilation is
  excluded from this sandbox by the 4 GiB budget): run
  `cargo test -p gyre-server --test default_sqlite_storage`,
  `cargo test -p gyre-server --lib`, and the real-binary persistence
  round-trip (boot with GYRE_DATABASE_URL unset, POST /api/v1/workspaces,
  stop, restart, GET by id; plus GYRE_DATABASE_URL=memory statelessness and
  the playwright e2e no-durable-file probe). Acceptance checkboxes remain
  unticked pending that verification.
