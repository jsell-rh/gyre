# Review — task-080 (Explorer Integration & Visual Tests)

Spec: `specs/coverage/system/explorer-implementation.md` §31 Integration Tests, §32 Visual Tests.
Commits under review (scoping per `commits:` frontmatter): `ce4f2044` (disconnect-fix repair), `2f14f11f` (rustfmt/clippy repair), `d4c1bd0d` (preset-test kill-property repair), `da74a27f` (restored mainline self-check tests), `659f64e8` (initial delivery). All five present on the branch; diff base `124a852b` → candidate `9a6930e` is test-only (`crates/gyre-server/tests/explorer_ws_integration.rs`, `web/tests/e2e/explorer-visual.spec.js`, 2 committed baselines) plus task/coverage docs — no production code touched.
Verdict: **complete** (round 2 — round 1's sole finding, the vacuous clean-disconnect check, is fixed and independently re-verified).

## Round 2 (this review, candidate `9a6930e`)

Relationship to round 1 (checkpoint `e2b225a` on the sibling attempt branch): web files and production Rust code are byte-identical between `e2b225a` and this candidate (`git diff e2b225a..9a6930e -- web/` empty; the only code delta is the test-side disconnect fix in `ce4f2044` plus docs). Round 1 verified the full §31/§32 inventory (15/15 WS/REST integration tests, 16/16 Playwright visual tests, vitest, fmt/clippy, preset-dimming and refinement-cap mutations) and found exactly one defect: the clean-disconnect loop in `explorer_ws_full_session_lifecycle` treated the tokio timeout (Elapsed) as success, so a server holding the socket open for the full 5s deadline passed the disconnect assertion vacuously.

The fix (`ce4f2044`) splits the terminal arm of the disconnect loop: a stream error (`Ok(Some(Err(_)))`) still counts as closed (the socket is gone), but `Err(_)` (Elapsed — no Close reply, no stream end within the deadline) now breaks *without* setting `closed`, so the final assertion "server must close the explorer session after the client sends Close" fails on a hang. Only a Close reply, stream end, or stream error count as closed.

Independent verification at the exact candidate head (evidence: `/tmp/stage/review-evidence/`, index in `notes.md`):

- `cargo test -p gyre-server --test explorer_ws_integration` → **15 passed, 0 failed**.
- **Disconnect-fix mutation**: parked the server's idle-loop Close/None arm (`crates/gyre-server/src/explorer_ws.rs:548`) on `std::future::pending::<String>().await` → `explorer_ws_full_session_lifecycle` **FAILS** at the disconnect assertion (test line 814) after the 5s deadline. Source restored; unmutated test passes. Round 1's vacuous pass is closed — the test now proves the server actually closes the session.
- `cd web && npm ci && npm test` → **1737 passed, 0 failed** (41 skipped, pre-existing).
- Playwright `explorer-visual` full suite (directly-launched prebuilt debug server, `GYRE_PORT=2222`/`GYRE_AUTH_TOKEN=e2e-test-token`, `reuseExistingServer`, fresh `web/dist` built from current source) → **16 passed**, covering all four §32 scenarios: semantic zoom (3 levels), view-query rendering (groups/callouts/narrative + annotation bar), filter presets (query-editor + real toolbar buttons), blast radius (tiered coloring + fixed-node dimming).
- **Preset kill-property mutation re-check**: `nodeFilterOpacity()` forced to `return 1.0` in `web/src/lib/canvas-filters.js` (dimming disabled), dist rebuilt → `filter_preset_buttons_show_correct_subsets` **FAILS** at the behavioral assertion (19,217 visible canvas changes < 20,000 minimum). Source restored, dist rebuilt deterministically (tree clean); unmutated test passes.
- `cargo fmt --check -p gyre-server` → exit 0; `cargo clippy -p gyre-server --test explorer_ws_integration` → exit 0, zero diagnostics referencing the test file.
- Working tree clean at candidate head after all probes; both mutation files restored and the web/dist rebuild reproduces the committed bytes.

Acceptance criteria: all eight checked — lifecycle (connect/message/response/disconnect, with the disconnect now genuinely asserted), agent view-query generation from a natural-language question (no-LLM fallback path, focus scope validated against seeded graph nodes), self-check refinement of overly broad groups (scripted `LlmPort`, `MAX_REFINEMENT_TURNS=0` mutation verified in round 1 on identical production code), saved-view round-trip via WS and REST, semantic zoom, view-query rendering, filter presets, and the full test suites pass.

Sandbox restrictions (for remote verification, not code defects): loopback HTTP from agent-issued curl is SSRF-guarded, so the server was launched directly outside the wrapped cargo (matching the documented recipe); no LLM credentials in-sandbox, so LLM-dependent paths use the production no-LLM fallback and a scripted in-process `LlmPort` (the WS protocol, dry-run, and refinement loop under test are real production code paths). GitHub CI on the exact head remains mandatory before merge.

Findings:

- (none)
