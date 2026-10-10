# Review — task-217 (Repair verified failure on main f4acb4ebcaf9)

Spec: task contract "Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions."
Assignment base: `7c6ac232ad1e43c977540034381e41c47548aa81`, candidate `357369a8faf52a110b445420c33dbf54bf81ee3c`.
Commits under review (scoping per `commits:` frontmatter): `c3cba0a6` (per-instance `ExplorerSessionRegistry` production fix), `e2cefb81` (round-4 test repair). `357369a8`/`c6e20dd6` are specs-only record-keeping.
Verdict: **complete** (round-4 review finding repaired and independently verified).

## Round 4 review repair — verification

The prior round's single finding (category `test`): `session_registry_is_per_instance_not_process_global` asserted only `live_count()==3` on two registries and discarded the returned `Notify` handles; under a process-global slot map both count assertions hold, so the test passed with the exact base flaw re-introduced.

**Fix verified (`e2cefb81`, test-only, +42/−10):** the test now keeps reg_a's three shutdown handles; reg_b fills to the same cap for the same user and evicts its own oldest (4th registration); a positive control asserts reg_b's own evicted session WAS signalled (proving the negative probe detects real signals), then the regression loop asserts none of reg_a's handles are signalled. This is precisely the remedy the prior review requested.

**Independent probes (this sandbox, at candidate `357369a8`, evidence under `/tmp/stage/review-evidence/`):**

- `cargo test --offline -p gyre-server --lib session_registry` → **4/4 pass, exit 0** (`session-registry-tests.txt`).
- `cargo test --offline -p gyre-server --lib explorer_ws` → **39/39 pass, exit 0** (`explorer-ws-tests.txt`), re-confirmed after all mutation probes on the restored tree.
- **Mutation probe (corrected single module-level static — the exact base flaw):** replaced the registry's per-instance `next_id`/`sessions` fields with module-level `MUTATION_NEXT_ID`/`MUTATION_SESSIONS` statics shared across all `ExplorerSessionRegistry` instances. Isolated run `cargo test --offline -p gyre-server --lib session_registry_is_per_instance_not_process_global` → **FAILED, exit 101, panicked at the regression assertion "registry B must not signal registry A's sessions"** (explorer_ws.rs:4401). Crucially, the count assertions passed under the mutation (both registries reported 3 — the shared map holds exactly `max` slots), exactly the prior finding's mechanism; the *signal* assertion is what fires. The regression is now genuinely pinned. (`mutation3-process-global.txt`. An earlier probe draft with per-function `static` items was discarded — function-local statics are distinct storage, so it failed spuriously on a `live_count` read of an empty map; the corrected probe shares one static as the real base code did.)
- **Production code unchanged by the repair:** `git diff e84f7c12 e2cefb81 -- crates/gyre-server/src/explorer_ws.rs` shows only test changes plus an upstream refactor of the `execute_tool` search filter (came in via the merge, `view_query_resolver::search_graph_nodes` — not this task's surface). The per-instance registry from `c3cba0a6` is intact: `register`/`release` operate on `self.next_id`/`self.sessions`; no residual references to the removed `ACTIVE_SESSIONS`/`SESSION_ID_COUNTER` statics anywhere in `crates/` (`git grep` empty).
- **Wiring complete:** all three `AppState` constructions initialize `explorer_sessions` (`build_state` lib.rs:900, mem test state mem.rs:3281, middleware test clone middleware.rs:233). `SKIP_WEB_BUILD=1 cargo build -p gyre-server --offline` → exit 0.
- **Gates:** `check-arch.sh` exit 0, `check-in-memory-state-stores.sh` exit 0 (registry is ephemeral connection state, deliberately not port-backed — documented in the struct docs and consistent with the check's intent), `check-task-commit-attribution.sh` exit 0 (`gates.txt`).
- Tree clean at candidate before and after all probes (`head-commit.txt`; source restored after each mutation, md5 `960b365307e1b8f9f12f72b22404203c` matches candidate).

## Transport restriction (recorded, not a code defect)

This sandbox cannot `accept(2)` (`capabilities.json`: errno 95, ENOTSUP), so `explorer_ws_integration` (7 tests) and the 9 listener-dependent lib tests cannot run here. Host verification remains required: `cargo test -p gyre-server --test explorer_ws_integration` (expects 7/7) and `cargo test --all`. The production fix's causal chain (per-`AppState` registry → no cross-server eviction for the shared `default:system` dev user → no mid-test socket close → delete-view list read no longer truncated) is verified at the unit level by the pinned regression test above.
