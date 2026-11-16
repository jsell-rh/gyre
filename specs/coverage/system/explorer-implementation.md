# Coverage: Explorer Implementation

**Spec:** [`system/explorer-implementation.md`](../../system/explorer-implementation.md)
**Last audited:** 2026-09-29 (re-verified all 5 `verified` rows against current code: SavedViewRepository port (gyre-ports/src/saved_view.rs, CRUD + tenant-scoped delete defaults), SQLite adapter + migrations 000042/000044-46 (all 11 spec columns; FK on repo_id deliberately dropped in 000044 for workspace-scoped views `repo_id="__workspace__"`, dedup unique index added), 6 system defaults incl. all 4 spec views by name+query (saved_views.rs:190-223, lazy-seeded with upgrade reconciliation in list_views), 5 REST endpoints at /repos/:id/views (mod.rs:897-905) + WS CRUD in explorer_ws.rs, ExplorerChat.svelte saved-views dropdown/save button (list_views on socket open). 3 `n/a` rows spot-checked correct (Migration Plan context w/ phases tracked as rows 25-28; Testing heading-only w/ subsections rows 30-32; cross-reference). No reclassifications; task-assigned rows untouched.)
**Coverage:** 5/33

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Overview | 2 | task-assigned | task-065 | |
| 2 | Architecture | 2 | task-assigned | task-065 | |
| 3 | WebSocket Protocol | 2 | task-assigned | task-069 | |
| 4 | Endpoint | 3 | task-assigned | task-069 | |
| 5 | Messages: Client → Server | 3 | task-assigned | task-069 | |
| 6 | Messages: Server → Client | 3 | task-assigned | task-069 | |
| 7 | LLM Agent (Claude Agent SDK) | 2 | task-assigned | task-070 | |
| 8 | Agent Configuration | 3 | task-assigned | task-070 | |
| 9 | MCP Tools Available to the Agent | 3 | task-assigned | task-068 | |
| 10 | Agent System Prompt | 3 | task-assigned | task-070 | |
| 11 | Self-Check Loop | 3 | task-assigned | task-070 | |
| 12 | Saved Views | 2 | verified | - | Port trait + SQLite adapter + REST + frontend (ExplorerChat.svelte) |
| 13 | DB Schema | 3 | verified | - | Migration 000042 creates table; 000044-000046 refine FK/unique constraints. All spec fields present. |
| 14 | System Default Views | 3 | verified | - | 6 defaults (spec has 4 + 2 extras: Spec Coverage, Ungoverned Risk). Lazy-seeded on first list_views. |
| 15 | REST API | 3 | verified | - | Full CRUD at /repos/:id/views (saved_views.rs) + /workspaces/:id/explorer-views. Auth + tenant isolation. |
| 16 | Frontend Components | 2 | task-assigned | task-065 | Overview section |
| 17 | ExplorerCanvas (Svelte) | 3 | task-assigned | task-065 | |
| 18 | ExplorerChat (Svelte) | 3 | task-assigned | task-071 | |
| 19 | ExplorerView (Svelte) | 3 | task-assigned | task-071 | |
| 20 | Server Implementation | 2 | task-assigned | task-069 | |
| 21 | Explorer WebSocket Handler | 3 | task-assigned | task-069 | |
| 22 | Graph Summary MCP Tool | 3 | task-assigned | task-068 | |
| 23 | Dry-Run MCP Tool | 3 | task-assigned | task-068 | |
| 24 | Migration Plan | 2 | n/a | - | Context section — "green field, no migration needed" |
| 25 | Phase 1: Canvas + Filters | 3 | task-assigned | task-065 | |
| 26 | Phase 2: Chat + Agent | 3 | task-assigned | task-071 | |
| 27 | Phase 3: Saved Views | 3 | verified | - | DB migrations, REST CRUD, saved views dropdown + save button in ExplorerChat, system defaults seeded lazily. |
| 28 | Phase 4: Polish | 3 | task-assigned | task-078 | |
| 29 | Testing | 2 | n/a | - | Section heading only |
| 30 | Unit Tests | 3 | task-assigned | task-079 | |
| 31 | Integration Tests | 3 | implemented | task-080 | 4 new WS/REST integration tests in crates/gyre-server/tests/explorer_ws_integration.rs: full session lifecycle (connect → list_views → message turn → clean Close disconnect), agent view-query generation from a natural-language question over seeded graph data (focus scope validated against real graph nodes), self-check loop refinement of overly broad groups (scripted-LlmFactory drives the native agent path; broad group → refining status → exactly one refined view_query forwarded; mutation-checked — disabling refinement fails the test), saved-view REST round-trip (POST → list → GET exact query equality → DELETE → 404). WS save/load round-trip already covered by explorer_ws_save_and_load_view. |
| 32 | Visual Tests | 3 | implemented | task-080 | Playwright suite web/tests/e2e/explorer-visual.spec.js (deterministic mock graph + forced webfonts, baselines committed in -snapshots/): semantic zoom at 3 levels (zoom-level-0/1/2), view query rendering incl. groups/callouts/narrative + annotation bar (view-query-annotated, view-query-container), filter presets (filter-all/endpoints/types/calls/dependencies via query editor + filter_preset_buttons_show_correct_subsets clicking the real toolbar preset buttons), blast radius interactive mode (tiered coloring on node click + fixed-node dimming). Review repair: the preset tests compare at pixelmatch threshold 0.05 (the default 0.2 discounted the dark-theme dimming shifts — ~125k affected pixels sit at 0.1–0.15 normalized YIQ — so a dimming-disabled build passed; the tighter threshold counts them) and the preset-buttons test additionally asserts live-canvas visible-change counts per preset transition (zoom 239k px, Endpoints 99.6k px, Endpoints→Types 127k px of 363,924; with preset dimming disabled in production code the transitions repaint ≤19k/38 px, so the test fails independent of the baselines). Mutation-verified: nodeFilterOpacity() forced to 1.0 fails filter_preset_buttons_show_correct_subsets; swapping the Endpoints baseline for the Types or default-view baseline fails (126k/128k px differ at 0.05). All 16 tests pass in one browser process in sandbox; exact-head GitHub checks mandatory. |
| 33 | Relationship to Other Specs | 2 | n/a | - | Cross-reference section |
