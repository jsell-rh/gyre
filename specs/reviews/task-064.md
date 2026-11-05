# Review: task-064 — View Query Grammar: Interaction Context, Composition Examples & Integration

Spec: `view-query-grammar.md` §10–16 (task rows 10–16 of `specs/coverage/system/view-query-grammar.md`).
Candidate: `505a0537` (production changes in listed checkpoint commit `8f650148`, merged with base
`0cb71015` as `7a18a289`; the three unlisted branch commits `505a0537`/`a7241056`/`ffc6e645` touch
`specs/tasks/task-064.md` only — process/docs, no product surface).

## Round 1 (2026-11-05) — NOT APPROVED

### What was independently verified as real

Evidence: `/tmp/stage/review-evidence/task-064-round2/` (manifest + logs; all commands run at the
exact candidate on the assigned checkout).

- **Wire-format fix is real.** `ExplorerChat.svelte` sendMessage now sends
  `toWireCanvasState(canvasState)`; the helper (`web/src/lib/canvas-state.js`) normalizes the internal
  camelCase state to the §10 snake_case shape. The claimed base defect is genuine: the server
  `CanvasState` serde only knows `selected_node`, so the raw internal state's `selectedNode` was
  silently dropped (the new vitest regression guard demonstrates the drop is real, and
  `test_canvas_state_section_10_full_deserialization` proves the server now parses the exact §10
  JSON, all 7 fields + 3 interaction shapes). Server-side `$clicked` resolution
  (`selected_node_id` at explorer_ws.rs:2806) consequently receives the selection again.
- **Composition tests are real, not mirrored logic.** `crates/gyre-domain/tests/view_query_composition.rs`
  uses the byte-exact §11–15 spec JSON → `ViewQuery` deserialize → `validate` →
  `gyre_domain::view_query_resolver::dry_run` — the same production resolver used by the WS agent
  loop and the graph API (explorer_ws.rs:2890-2920, api/graph.rs:1562). Assertions include
  exclusions, BFS depths, strict-`>` boundary, Calls-only test reachability, per-seed concept
  expansion, and annotation template resolution. Ran
  `cargo test -p gyre-domain --test view_query_composition` → **8 passed, 0 failed**.
- **System default view addition is real.** "High-Risk Untested Code" added to
  `system_default_views()` (saved_views.rs:203) with the exact §14 shape; the lazy-seeding
  reconcile in `list_views` (saved_views.rs:139-175) back-fills it for existing repos, so this is
  not new-repos-only. Shape asserted in saved_views.rs/explorer_views.rs/explorer_ws.rs tests;
  ran `cargo test -p gyre-server --lib saved_views` → **13 passed**, and
  `--test explorer_ws_integration` → **8 passed**.
- **Committed dist is not stale.** `web/dist/assets/index-Cj-H2Guz.js` contains the new code
  (`selected_node` ×3, `recent_interactions` ×12, `to_level`, `zoom_in`/`zoom_out`, `base_level`).
- **Web tests.** `npm ci` then `npx vitest run src/__tests__/canvas-state.test.js` → **14 passed,
  0 failed** (wire normalization, zoom recording/coalescing/threshold, non-user-zoom exclusion at
  the pure-function level, camelCase-drop regression guard).
- Click interactions remain legacy strings (`click:Name(type)`, ExplorerCanvas.svelte:4171) but the
  server `InteractionRecord` string parser resolves them to `{action, node, detail}` — semantically
  equivalent to the §10 example; acceptable.

### Findings (blocking)

1. **§10 incomplete at the LLM boundary** — `crates/gyre-server/src/explorer_ws.rs` (~:2596-2672).
   §10 says "When the LLM is invoked, it receives the current canvas state" and its example includes
   `zoom_level` and `active_filter`. The candidate fixed the client→server wire format and the
   server deserialization, but the `canvas_json` built for the LLM `<canvas_state>` context still
   surfaces only selected_node / visible_groups / active_lens / recent_interactions (+ the new
   to_level/preset) / active_query — `zoom_level` and `active_filter` are dropped after
   deserialization and never reach the LLM; the system-prompt field docs (~:3480-3486) omit them
   too. The omission existed at base, but §10 is this task's assigned section and coverage row 10
   is marked implemented with the claim "Full §10 shape now reaches the agent" — inaccurate for the
   consumer the spec names. Fix is small: insert both fields into `canvas_json` and document them.

2. **Zoom tracking never disarms** — `web/src/lib/ExplorerCanvas.svelte`. `zoomTrackArmed` is set
   true on the first wheel/keyboard/pinch zoom (:4039/:4659/:4666/:4779) and never reset; since
   `trackZoomChange()` runs every frame from `lerpCam()` (:3871-3872), any later programmatic zoom
   >20% — drill transitions (:3855-3862) and query-driven `"fit"` zoom — is recorded as a user zoom
   interaction or coalesced into (overwriting) the user's previous zoom entry. Probe
   (`zoom-arming-probe.mjs`, real `recordZoomInteraction` + the component's loop semantics): after
   one user wheel-zoom 1.0→1.5, a query-driven fit to 2.8 rewrites the entry to
   `{action:'zoom_in', to_level:2.8}`. This contradicts the shipped claim that drill/query-driven
   zoom is excluded (only the initial auto-fit is, since it precedes arming) and pollutes the §10
   `recent_interactions` context the LLM receives. Fix: disarm / re-baseline when a programmatic
   zoom source takes control.

### Non-blocking notes

- `system_default_view_shapes_resolve` duplicates the saved_views.rs JSON strings as local
  constants (gyre-domain cannot import gyre-server). Drift risk is bounded by the exact-shape
  assertions in saved_views.rs tests, but a shared fixture (e.g. in gyre-common) would be stronger.
- Coverage row 16 (Relationship to Other Specs) is marked implemented with concrete producer/
  consumer links; the links exist and are exercised — acceptable for a relationship section.

### Verdict

**Not approved.** Two defects, both small fixes, in the task's assigned §10 territory: the LLM
context drops 2 of the 7 specced canvas_state fields, and zoom interaction tracking misattributes
programmatic zooms as user interactions after the first user gesture. The composition-example
testing (§11–15) and the §14 default view are solid and reproduced green; §10 needs one more pass.
