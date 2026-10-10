# Review — task-217 (Repair verified failure on main f4acb4ebcaf9 — explorer WS session registry)

Task: `specs/tasks/task-217.md` — repair the verified `cargo test --all` failure on main: `explorer_ws_delete_view` panicking at `crates/gyre-server/tests/explorer_ws_integration.rs:294` ("Deleted view should not appear in list"), plus the attribution-drift baseline finding.
Candidate under review: `e84f7c1202c3d0678d7448a29c63c9e4263ea83b` (assignment base `653a696f0565165cc5dfa2c8f1050a25fbf2c590`).
Diff vs base: `c3cba0a6` production fix (4 Rust files: `explorer_ws.rs` +192/−36, `lib.rs` +6, `mem.rs` +1, `middleware.rs` +1) + docs-only commits (`4b0b73b4`, `b1ff7f6d`, `5218b0a4`, merge `5c86bb37`, `e84f7c12` — specs/tasks only).
Verdict: **needs revision** (one test finding; the production fix itself is sound).

## What the fix is

Base `653a696f` enforced the per-user concurrent explorer-WS session cap (default 3, `GYRE_EXPLORER_MAX_SESSIONS`) through a **process-global** `static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<String, Vec<SessionSlot>>>>` keyed by `tenant_id:agent_id`. Every `AppState` is an independent server (own storage, own port), but the map was shared process-wide. The `explorer_ws_integration` binary spawns one server per test — 7 tests, all authenticating as the same dev user (`default:system`), run concurrently by default — so the 4th concurrent registration evicted the oldest **live** session on a *different* server, closing that socket mid-test. That matches the durable CI log exactly: the binary finished in 0.13s (no 5s read deadline elapsed — reads returned `None` instantly off a closed socket), the test passed save+list, then panicked at the final assert with `found_deleted` still `true`.

The candidate replaces the global with `ExplorerSessionRegistry` held per-`AppState` (`state.explorer_sessions: Arc<ExplorerSessionRegistry>`), wired in `build_state` (`lib.rs:900`), the mem test state (`mem.rs:3281`), and the middleware test-state clone (`middleware.rs:233`, sharing the base registry — correct: that helper clones one logical state). `register` returns `(id, Arc<Notify>)`; the session loop selects on `shutdown_notify.notified()` and closes with "Session replaced by a newer connection."; a `SessionGuard` drops the slot on exit. The old statics are fully removed (`grep ACTIVE_SESSIONS|SESSION_ID_COUNTER` → nothing).

## Verified working (no findings)

- **Failure mechanism is correctly diagnosed and the fix removes it.** Read base vs candidate side by side; the eviction path the CI log shows (`read_msg` → `None` on a replaced socket) only existed because unrelated servers shared one map. Per-`AppState` registry scoping matches the deployment unit the cap is documented to bound ("per-user concurrent-session cap bounds concurrent LLM cost for one server deployment").
- **State scoping decision is defensible, not a dodge of the port-store invariant.** `check-in-memory-state-stores.sh` targets durable `Arc<Mutex<HashMap/Vec>>` store *aliases* in `AppState`; live TCP sessions cannot outlive the process, so a port-backed registry would be over-engineering. The registry is a typed struct with a rationale comment, not a bare alias. Gate passes at candidate: `OK: no in-memory Arc<Mutex<HashMap/Vec<...>>> store aliases in non-test server code.` (exit 0).
- **Registry semantics are correct and covered by real unit tests.** `register` evicts oldest-first beyond `max.max(1)` and signals exactly the evicted slot's `Notify` (permit semantics asserted via a later `notified()` await); `release` is idempotent and cleans up the empty key; zero-cap admits the registering session instead of panicking. `cargo test --offline -p gyre-server --lib explorer_ws` at candidate → **39 passed; 0 failed** (evidence: `cargo-test-explorer-ws-at-candidate.txt`, `explorer-ws-session-registry-tests.txt`).
- **No scope/auth weakening in the handler.** The registration block was replaced without touching the repo→workspace→tenant chain or the membership/agent-workspace checks (read `explorer_ws.rs:302-460`).
- **Changed-line gates pass at candidate:** `check-rustfmt-diff.py 653a696f` → clean, exit 0; `check-task-commit-attribution.sh` → OK exit 0; `check-arch.sh` → OK exit 0; `check-in-memory-state-stores.sh` → OK exit 0 (evidence files under `/tmp/stage/review-evidence/`: `rustfmt-diff-at-candidate.txt`, `attribution-at-candidate.txt`, `arch-at-candidate.txt`, `mem-stores-at-candidate.txt`).
- **Tree restored after all probes:** working tree clean, HEAD tree `ff4ba617` equals candidate tree; post-restore `explorer_ws` run still 39/39 (`post-restore-explorer-ws.txt`).

## Finding (needs revision)

**F1 — the regression test does not pin the regression** (`crates/gyre-server/src/explorer_ws.rs`, `session_registry_is_per_instance_not_process_global`, ~lines 4397-4419; claim in `specs/tasks/task-217.md` Round 2: "two independent registries never evict each other's sessions (the regression, now pinned by test)").

Mutation probe (temporary, restored and re-verified after): re-introduced the exact base flaw by making `register`/`release`/`live_count` use process-global statics instead of `self` fields, then ran the test in isolation:

```
$ cargo test --offline -p gyre-server --lib session_registry_is_per_instance_not_process_global
test explorer_ws::tests::session_registry_is_per_instance_not_process_global ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1199 filtered out
exit 0
```

It also passes with `--test-threads=1`. Mechanism: the test registers 3 sessions on registry A then 3 on registry B for the same user and asserts `live_count()==3` on both. Under a global map, B's registrations evict A's slots and the shared map still ends with exactly 3 slots — both count assertions hold. The `register` return values (the `Arc<Notify>` eviction signals — the observable that actually closed the socket in the integration failure) are discarded, so eviction is never observed. In a full parallel run the mutation does fail sibling tests (`evicts_oldest_beyond_cap`, `zero_cap`), but only via cross-test pollution of the shared static, not via this test's own assertions.

This is exactly the review-policy class "a test that still passes with the relevant production behavior disabled is not proof." The production fix is fine; the *claim that the unit suite now pins the regression* is what's hollow.

Suggested fix (one test): keep one of registry A's returned `Notify` handles and assert it is never signalled after registry B fills to the cap (mirroring the negative-signal assertions already present in `session_registry_evicts_oldest_beyond_cap`), e.g.:

```rust
let (_id_a0, sig_a0) = reg_a.register(user, 3);
reg_a.register(user, 3);
reg_a.register(user, 3);
reg_b.register(user, 3);
reg_b.register(user, 3);
reg_b.register(user, 3);
assert_eq!(reg_a.live_count(user), 3);
assert_eq!(reg_b.live_count(user), 3);
// A's live session must never be signalled by B filling up — this is the
// cross-server eviction the integration binary caught.
tokio::time::timeout(std::time::Duration::from_millis(50), sig_a0.notified())
    .await
    .expect_err("registry B must not signal registry A's live session");
```

(the test becomes `#[tokio::test]`). Under the mutation this fails (B's registrations evict A's slot and signal it); under the real code it passes.

## Host verification required (sandbox transport restriction, not a code defect)

This sandbox cannot run listener-dependent tests: `accept(2)` returns ENOTSUP (errno 95; `/tmp/stage/capabilities.json` `tcp_listener_probe`). Record for the verifier:

- `cargo test -p gyre-server --test explorer_ws_integration` — expects 7/7 (the repaired failure).
- `cargo test --all` — expects full green (baseline log: 1194 lib tests passed there; only `explorer_ws_delete_view` failed).
- After fixing F1: `cargo test -p gyre-server --lib session_registry` and a re-run of the mutation probe (`mutation1-analysis.md` documents the exact edit) to confirm the test now fails under it.

## Evidence index (`/tmp/stage/review-evidence/`)

- `mutation1-analysis.md`, `mutation1-cmd.txt`, `mutation1-isolated-test-result.txt`, `mutation1-process-global-result.txt` — F1 mutation probe (isolated pass, exit 0; parallel-run pollution failures).
- `cargo-test-explorer-ws-at-candidate.txt`, `explorer-ws-session-registry-tests.txt` — 39/39 at candidate.
- `post-restore-explorer-ws.txt` — 39/39 after restoring the pristine file.
- `rustfmt-diff-at-candidate.txt`, `attribution-at-candidate.txt`, `arch-at-candidate.txt`, `mem-stores-at-candidate.txt` — gates at candidate, all exit 0.
- `explorer-ws-before-mutation.sha256` — pre-mutation checksum; restored file matches (`b7804fb0…`).
- `build-lib-test.txt`, `cargo-fetch*.txt` — offline build proof (lib test build exit 0).
