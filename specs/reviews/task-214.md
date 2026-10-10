# Review — task-214 (Repair verified failure on main f4acb4ebcaf9)

Spec: task contract "Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions." Verified failure per `GYRE_BASELINE_FAILURE_JSON`: `bash scripts/check-task-commit-attribution.sh` fails on base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` — task-189's product-surface commit `f4acb4eb` is absent from `specs/tasks/task-189.md`'s `commits:` frontmatter.

Candidate under review: `e5995bcb4513961cbea32f6d3c43f3c80a31eb4e` (assigned base `653a696f0565165cc5dfa2c8f1050a25fbf2c590`). Verdict: **approved**.

## Diff scope

`git diff 653a696f..e5995bcb --name-only` is exactly five files: `crates/gyre-server/src/explorer_ws.rs` (+192/−36), `lib.rs` (+6), `mem.rs` (+1), `middleware.rs` (+1), and the new `specs/tasks/task-214.md` (+94). Diff from base for `scripts/`, `web/`, and `crates/gyre-server/tests/` is **empty** — no check weakened, no exemption file touched, no integration test modified to accommodate the fix. `git status --porcelain` clean at HEAD.

## 1. Baseline failure — attribution drift (f4acb4eb / task-189)

- Reproduction confirmed historically: the process-global-static tree at `f4acb4eb` (ancestor of assigned base, confirmed via `git merge-base --is-ancestor`) is where the baseline log was captured; the task-189 drift (`f4acb4eb` absent from task-189's `commits:` frontmatter) was real.
- **State at assigned base:** `bash scripts/check-task-commit-attribution.sh` at `653a696f` → OK, exit 0 ([attribution-base.txt]). The drift was already repaired upstream by `a1751da1` (task-212, ancestor of base, confirmed by ancestry and by `git log e5995bcb -- specs/tasks/task-189.md` showing `a1751da1` as the last commit touching that file). No new drift was introduced on the path `f4acb4eb → 653a696f`: task-200's `6bf777a6` product-surface commit is recorded in task-200's frontmatter at base (verified by grep).
- At candidate HEAD the check exits 0 ([attribution-candidate.txt]); the candidate's product-surface commit `96d2a28c` is recorded in task-214's `commits:` frontmatter (full SHA), and the remaining branch commits (`1f7876ab`, `2bbc4f8e`, `e5995bcb`) touch only `specs/tasks/task-214.md` — outside the check's product-surface definition. Exemption file untouched.
- The baseline log's rustfmt/clippy failures on `specs_assist.rs` do **not** reproduce: `python3 scripts/check-rustfmt-diff.py f4acb4eb...` at this tree reports changed lines clean (7 Rust files). `f4acb4eb`'s own diff touches `personas.rs`, not `specs_assist.rs` — those lines came from the superseded checkout `11d68489`, as the task record claims.

## 2. Durable finding repair — explorer WS session cap (durable finding `6db444f0`, `cargo test --all` exit 101)

- **Root cause confirmed from source:** at `f4acb4eb`, the per-user cap was enforced via process-global `static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<...>>>` (explorer_ws.rs lines 97/110 at that commit). The integration harness (`explorer_ws_integration.rs:20-28`) spawns one axum server per test (`TcpListener::bind` + `tokio::spawn`), all tests authenticate as the same dev user `default:system`, cap 3, tests run concurrently — so the 4th concurrent registration evicted the oldest live session on a *different* server, which received "Session replaced by a newer connection." instead of the expected error. That is exactly the recorded panic at `explorer_ws_integration.rs:340` (6/7 passed, race-dependent).
- **Fix is correct and complete:** `ExplorerSessionRegistry` (named struct, not a bare alias) is a field on `AppState` (`pub explorer_sessions: Arc<ExplorerSessionRegistry>`), constructed in `build_state` and in every other AppState construction site — `grep -rn explorer_sessions crates/gyre-server/src` shows exactly 4 sites (lib.rs field+wiring, mem.rs test-state, middleware.rs clone) and the compile passing proves none missed. `register()` returns `(id, Arc<Notify>)`; `SessionGuard` releases the slot on drop (panic-safe); eviction removes the oldest slot and signals *only* that session's Notify; `max==0` is treated as 1 (documented, prevents an infinite eviction loop on an empty list). Production semantics unchanged: one AppState per server process, so the per-user cap still bounds concurrent sessions per deployment.
- **Identity with the reviewed task-217 commit:** `git diff c3cba0a6 96d2a28c -- <the 4 rust files>` is empty (0 bytes) — byte-identical to the fix reviewed on the task-217 branch line (task-217 record, round 2, documents the same durable finding and 39/39 unit evidence). `c3cba0a6` never landed on main (not an ancestor of the candidate), so porting it here is the correct way to deliver that repair to this line.
- **Not port-backed, deliberately:** live WS sessions are ephemeral process-local state (a TCP connection cannot outlive its process); the registry is a named struct field, so `check-in-memory-state-stores.sh` passes (exit 0) and its rationale for durable stores (restart-orphaned records, 404-on-acceptance-endpoint) does not apply to connection-tracking state.

## Independent probes (evidence: `/tmp/stage/review-evidence/`, `summary.txt`)

- `bash scripts/check-task-commit-attribution.sh` at candidate → OK, exit 0; at assigned base → OK, exit 0 (upstream repair confirmed present at base).
- `python3 scripts/check-rustfmt-diff.py 653a696f...` → changed lines clean (4 Rust files), exit 0.
- `bash scripts/check-in-memory-state-stores.sh` → OK, exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib explorer_ws::` → **39 passed; 0 failed; 1161 filtered out**, exit 0 ([cargo-test-explorer-ws-review.txt]), including the 4 new registry tests: `session_registry_evicts_oldest_beyond_cap` (eviction signals exactly the oldest session's Notify, never a live one), `session_registry_release_frees_slot_and_is_idempotent`, `session_registry_is_per_instance_not_process_global` (the pinned regression: two independent registries at cap 3 do not evict each other — this is the failure mode the durable finding recorded), `session_registry_zero_cap_admits_registering_session`. These are real behavioral tests against the production registry type, not mirrored logic: they assert on Notify signalling and live counts through the public `register`/`release` API.

## Transport restriction (not a code defect)

This sandbox cannot run the WS integration binary: `capabilities.json` `tcp_listener_probe` — `accept(2)` returns `ENOTSUP` (errno 95), and `WsCtx::new` binds a real `TcpListener`. Host/CI verification must run: `cargo test -p gyre-server --test explorer_ws_integration` (expects 7/7) and `cargo test --all`, plus clippy `--all-targets` (the baseline-log clippy line was a superseded-checkout artifact and does not reproduce against this tree's diff base).

No findings. The fix is causal, minimal, scoped to the real defect, byte-identical to the independently reviewed task-217 repair of the same finding, and the attribution contract holds at both base and candidate with no gate weakened.
