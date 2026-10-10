# Review — task-214 (Repair verified failure on main f4acb4ebcaf9)

Assigned candidate: `ba1505fba6ed733e8562eb0fd7f0e4c93666dba7` (branch `pipeline/task-214/1ed80b0d9d7b426eac90a284bbc982a0-1`).
Assignment base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20` (the task's original base `f4acb4eb` had advanced via base merges; the assignment base is the recorded merge parent).
Task contract: reproduce and repair the verified upstream failure (task-189 attribution drift at `f4acb4eb`), plus durable verification failures introduced by base merges. No weakened checks, no skips/exemptions, no blocked feature implementation.

Verdict: **approved** (this review). Candidate `ba1505fb`.

## Product diff under review (`git diff 27bd585c..ba1505fb`)

Non-merge commits in range, product surface:

- `96d2a28c` — explorer WS per-user session cap moved from process-global `ACTIVE_SESSIONS` static into `AppState` as `ExplorerSessionRegistry` (`crates/gyre-server/src/explorer_ws.rs` +200/−36; wiring in `lib.rs`, `mem.rs`, `middleware.rs`).
- `8e585249` + `52a7d4af` — byte-slice truncation fix: `&raw_preview[..500]` replaced by `truncate_invalid_query_preview()` routing through `gate_executor::truncate_bytes` (char-boundary-safe, task-095 F4 pattern); its frozen exemption entry removed; multibyte truncation test corrected to the real char boundary (498 for 3-byte chars under a 500-byte cap).
- `d1872f67` — task-155 attribution drift repair (re-introduced by the assignment's own base merge): full SHA `27bd585ca7eb429905ccbded1f48b4d0167c0c20` appended to `specs/tasks/task-155.md` `commits:` frontmatter.
- Everything after `d1872f67` (`de7f3d18`, `cc41876b`, `ba1505fb`) touches only `specs/tasks/task-214.md` (evidence documentation + frontmatter recording).
- `web/dist` churn is a rebuild artifact of the branch checkpoints, not a source change: `web/src` is byte-identical between base and candidate (`git diff 27bd585c ba1505fb -- web/src` is empty). The rebuilt bundle actually *fixes* a stale base dist (base src already contained task-196's `slice(-20)` history cap; base dist did not; candidate dist does).

The task-189 baseline failure itself (`f4acb4eb` unlisted in task-189's `commits:`) was repaired in an earlier round on this branch: `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` is the last entry in `specs/tasks/task-189.md:8`. `specs_assist.rs` (the baseline log's rustfmt/clippy failures) is untouched by the entire range — those were artifacts of the superseded attempt checkout, as the task record claims.

## Independent verification (this review, at `ba1505fb`)

Mechanical gates (all exit 0; evidence in `/tmp/stage/review-evidence/`):

- `scripts/check-task-commit-attribution.sh` → OK (assignment's baseline failure class; the gate that failed at `f4acb4eb`).
- `scripts/check-byte-slice-truncation.sh` → OK (durable finding `e24468c2` class).
- `scripts/check-in-memory-state-stores.sh`, `scripts/check-relative-path-defaults.sh` → OK.
- `python3 scripts/check-rustfmt-diff.py 27bd585ca7eb429905ccbded1f48b4d0167c0c20` → changed lines clean (5 Rust files).
- Frozen baselines intact: `scripts/task-commit-attribution-exemptions.txt` still exactly its 3 documented entries; `scripts/byte-slice-truncation-exemptions.txt` *lost* one entry (the fixed `raw_preview` line — removal is the correct direction; the file shrinks, never grows). No check, skip, or gate weakened anywhere in the diff.

Failure reproduction (independent):

- `bash scripts/check-byte-slice-truncation.sh` at `e5995bcb` (source tree of durable finding `e24468c2`): **exit 1**, exactly the two recorded sites (`&raw_preview[..500]` at explorer_ws.rs:2937, `&d[..100]` at :3492) — the durable finding is real and the recorded reproduction is truthful. At candidate HEAD the same gate is clean. Evidence: `byteslice-e5995bcb-before.txt`.

Unit tests (focused probe, pristine candidate tree):

- `cargo test -p gyre-server --lib explorer_ws::` → **42 passed, 0 failed** (1170 filtered), including all 4 `ExplorerSessionRegistry` tests and all 3 `truncate_invalid_query_preview` tests. Evidence: full run in this session's job log.

Mutation probe (regression proof — a test that passes with the production behavior disabled is not proof):

- Temporarily routed `ExplorerSessionRegistry`'s state through a process-global `static GLOBAL_SESSIONS: LazyLock<Mutex<HashMap<...>>>` (the exact pre-fix regression class: state shared across `AppState` instances), keeping the public API identical. Result: **2 of 4 registry tests FAILED** — `session_registry_evicts_oldest_beyond_cap` ("session 1 must not be signalled") and `session_registry_release_frees_slot_and_is_idempotent` ("live session must not be signalled after a release+register") — because one registry's registrations evicted another registry's live sessions. Pristine source restored (verified: `git diff` empty, SHA `12abe377...` byte-identical), and the same 4 tests re-run green. Evidence: `mutation-process-global-session-registry.txt`.
- Honest caveat: `session_registry_is_per_instance_not_process_global` in isolation does NOT fail under this mutation (two registries reading one global map still each report `live_count == 3`); only the eviction-signalling assertions catch it. The suite as a whole pins the regression, which is what matters — recorded so the weak individual test is a known quantity, not a hidden one.

Code-path analysis (registry fix):

- `register()` (`explorer_ws.rs:139-160`): monotonic per-registry IDs, oldest-first eviction signals exactly the evicted slot's `Notify`, `max.max(1)` admits the registering session under a 0 cap (no eviction-loop spin). `release()` (`:163-171`) is idempotent and cleans up the empty user key. `SessionGuard` (`:285-300`) releases on drop — panic-safe, and now holds `Arc<ExplorerSessionRegistry>` from `state.explorer_sessions` so release targets the owning server's registry.
- Wiring is complete and consistent: `AppState.explorer_sessions` (`lib.rs:242`, built at `lib.rs:900`), test-state (`mem.rs:3281`), middleware test clone (`middleware.rs:233` — clones the Arc, sharing the registry as production does). No process-global static remains (`ACTIVE_SESSIONS`/`SESSION_ID_COUNTER` grep: zero matches).
- Eviction semantics are production-real: the `notified()` branch in the session's select loops (`explorer_ws.rs:543`, `:788`) sends the same "Session replaced by a newer connection." error the pre-fix code sent — same user-visible contract, now scoped to the correct server.
- `truncate_bytes` visibility change (`gate_executor.rs:845`, private→`pub(crate)`) adds no external API and all six pre-existing call sites are unchanged.

Auth/scope safety: the diff does not touch authentication, ABAC registration, tenant/workspace scoping, or any route handler; the WS session-key is built from the authenticated `auth.tenant_id`/`auth.agent_id` (unchanged), and per-repo tenant/membership checks in the handler are unchanged.

## Host verification requirements (sandbox transport restriction, not a code defect)

This sandbox cannot run the WS integration binary: `accept(2)` returns `ENOTSUP` (errno 95; `/tmp/stage/capabilities.json`). The integration harness binds a real `TcpListener` (`tests/explorer_ws_integration.rs:21`). Reproduction here is the durable CI log plus the mutation probe above. Host verification must run:

1. `cargo test -p gyre-server --test explorer_ws_integration` — expects 7/7 (the recorded upstream failure was `explorer_ws_message_too_long` receiving the eviction error instead of "Message too long", race-dependent 6/7).
2. `cargo test --all`.
3. GitHub CI on the pushed branch `pipeline/task-214/1ed80b0d9d7b426eac90a284bbc982a0-1` (candidate `ba1505fb` confirmed pushed to `origin`).

## Findings

None blocking. Two non-blocking observations recorded for completeness:

1. `session_registry_is_per_instance_not_process_global` alone does not discriminate the process-global regression (see mutation caveat above); the suite does. Not a defect — the assertion set as a whole is the regression pin.
2. The branch carries rebuild churn in `web/dist` via pipeline checkpoint commits. The final bundle is a faithful build of the candidate's `web/src` (which is identical to base), and the rebuild corrects a stale base dist; no source/bundle divergence exists at the candidate tip.

Review method note: this review's mutation probe modified `crates/gyre-server/src/explorer_ws.rs` temporarily and restored it byte-for-byte (verified empty `git diff` and matching SHA `12abe377216d72537254fd774142588ee938cbea9cc1cbc8512df0d1c7721ca8`). No production code, scripts, verifiers, or specs were modified.
