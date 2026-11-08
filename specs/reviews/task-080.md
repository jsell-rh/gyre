# Review — task-080 (Explorer Integration & Visual Tests, §31–32)

Spec: `specs/coverage/system/explorer-implementation.md` §31 "Integration Tests", §32 "Visual Tests".
Candidate under review: `e1879c668` (base `124a852be`; attributed commits 2f14f11f, d4c1bd0dd, da74a27ff, 659f64e8 — all verified in ancestry). Diff vs base is test-only: `crates/gyre-server/tests/explorer_ws_integration.rs` (+4 §31 tests, ScriptedLlmFactory replacing MockLlmPortFactory wiring, lint cleanup), `web/tests/e2e/explorer-visual.spec.js` (pixelmatch threshold 0.05 on the six preset tests + new `filter_preset_buttons_show_correct_subsets` with baseline-independent canvas-pixel behavioral assertions), two new preset baselines, coverage matrix §31/§32 → implemented.
Verdict: **needs-revision** (one finding).

## Round 1

Test runs (exact candidate head, cold build, `/tmp/stage/bin/cargo` focused probes; working tree clean before and after each mutation, all mutations restored):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration` → **15 passed, 0 failed**.
- `cargo fmt --check -p gyre-server` → clean; `cargo clippy -p gyre-server --test explorer_ws_integration` → zero warnings for the file (crate-wide pre-existing warnings elsewhere untouched by this candidate).
- `cd web && npm ci && npm run build` → ok; produced asset hashes identical to the committed `web/dist` (committed bundle was current for this head).
- Playwright `explorer-visual` (server launched directly from the fresh debug binary, `GYRE_AUTH_TOKEN` exported into the Playwright process) → **16 passed** twice.
- `cd web && npm test` → **71 files, 1737 passed, 41 skipped, 0 failed**.
- Full evidence: `/tmp/stage/review-evidence/` (notes.md with commands, mutations, outputs).

Verified working (no findings):

- **§31 agent view-query generation** — `explorer_ws_agent_generates_view_query_from_question` seeds real graph nodes and asserts the production no-LLM fallback (`handle_no_llm_fallback`, real code in `explorer_ws.rs`) extracts "Space" from the question, validates it against the graph store, and emits a `focus` scope that deserializes against the ViewQuery grammar. By construction the assertion discriminates graph-backed validation (without a graph hit the fallback emits the first stop-word-filtered candidate, "change", not "Space").
- **§31 self-check loop** — `explorer_ws_self_check_loop_refines_broad_groups` drives the native LlmPort agent path via a test-local `ScriptedLlmFactory`; 25 seeded `node_N` nodes make the scripted broad group trip the dry-run breadth warning. **Mutation-verified**: `MAX_REFINEMENT_TURNS = 0` fails the test ("overly broad group must trigger a refining status"). The restored mainline tests (`explorer_ws_self_check_refinement_flow`, `explorer_ws_self_check_sends_best_version_after_cap`) cover the actionable-warning refinement and best-version-at-cap paths.
- **§31 saved view round-trip** — WS save→list→load (`explorer_ws_save_and_load_view`, pre-existing) and the new REST round-trip `explorer_rest_saved_view_round_trip` (POST 201 → list contains it → GET returns exactly-equal query JSON → DELETE 204 → 404), all green.
- **§32 visual tests** — all four scenarios present and passing against a fresh build of the current web source: semantic zoom (3 levels), view-query rendering (groups/callouts/narrative + annotation bar), filter presets (query-editor tests + the preset-buttons test clicking the real toolbar buttons), blast radius (tiered coloring + dimming). **Mutation-verified**: forcing `nodeFilterOpacity()` to `1.0` (preset dimming fully disabled, web rebuilt) fails `filter_preset_buttons_show_correct_subsets` at the baseline-independent behavioral assertion (19217 visible-change pixels < the 20,000 minimum; healthy ≈ 99.6k) — the prior review round's greenwash finding is genuinely repaired, and the tighter 0.05 threshold makes the Endpoints/Types baselines non-interchangeable.

Findings:

- [-] **F1 (moderate): the §31 lifecycle test's clean-disconnect assertion is vacuous — a tokio timeout counts as "closed".** In `explorer_ws_full_session_lifecycle` (`crates/gyre-server/tests/explorer_ws_integration.rs`), the disconnect loop wraps `stream.next()` in `tokio::time::timeout(remaining, …)` and its bare `Err(_)` arm — which fires when the deadline elapses with no Close reply and no stream end, i.e. the server is still holding the socket — sets `closed = true`. The final assertion ("server must close the explorer session after the client sends Close") therefore passes on exactly the hang it claims to reject, contradicting the shipped note "server confirmed closing the socket (Close reply or stream end, never a 5s hang)". Reproduced on the exact head: mutating `explorer_ws.rs`'s idle-loop Close/None arm to `std::future::pending::<()>().await` (session parks forever, socket held open, no Close reply) leaves the test green in 5.86s — the full 5s deadline elapses and the timeout arm marks it closed (evidence: `/tmp/stage/review-evidence/notes.md` §7). The other three elements of the lifecycle scenario (connect, message turn, response) are genuinely asserted and would fail if broken; only the disconnect element is unproven. Remedy: treat `Err(_)` (Elapsed) as failure — only a Close reply (`Ok(Some(Ok(Message::Close(_))))`), stream end (`Ok(None)`), or a stream error may set `closed = true`.

Minor (not blocking, recorded for completeness):

- `Ok(Some(Err(_)))` (WebSocket stream error) also counts as "closed" — lenient, but defensible since the connection is gone either way; folding it into the fix is optional.
- An earlier, weaker mutation attempt (Close arm → `continue` only) did **not** hang the server because axum's receiver yields `None` after Close, hitting the `None => break` arm — documented in the evidence so a future repair doesn't re-test with an ineffective mutation.

Scope notes:

- The wrapped review-stage cargo rejects workspace-wide suites; `cargo test --all` and exact-head GitHub CI remain verification-stage obligations (the focused equivalents of every touched surface were run here).
- No production code, scripts, or unrelated files were modified by the candidate; both mutations applied during review were restored and the full 15-test suite re-run green afterwards (`git status --short` clean).
