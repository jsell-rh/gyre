# Review — task-069 (Explorer WebSocket Protocol & Server Handler)

Spec: `explorer-implementation.md` §3–6, §20–21.
Comparison base: `75ada1c4073ee9d56d3a1aebf5c780582d8867cb` → candidate `0c12e20a94b11151fb47f754c2368f10d2a8fc90`.
Commits under review: `30dc2826` (the only product-surface commit in the range; verified listed in the task frontmatter — the remaining range commits `a338b7e5`/`2f092bcd`/`0c12e20a` touch only `specs/tasks/*.md`, plus two merge commits). Diff vs base touches exactly three files: `crates/gyre-server/src/explorer_ws.rs`, `crates/gyre-server/tests/explorer_ws_integration.rs`, `specs/tasks/task-069.md`.

## Round 1

### Task contract (audit + gap-fill)

The handler pre-existed at base; the task is an audit of §3–6/§20–21 with gap-fill. Audit results at candidate HEAD:

- **§4 Endpoint / route registration.** `WS /api/v1/repos/:repo_id/explorer` registered at `crates/gyre-server/src/lib.rs:675-680`, outside the ABAC-body middleware (WebSocket upgrades cannot go through body-reading middleware). Auth is enforced before upgrade via the `AuthenticatedAgent` extractor in the handler signature (rejects missing/invalid Bearer, invalid/expired ticket with 401), plus in-handler repo→workspace→tenant chain checks and workspace-membership/agent-scope verification (`handle_explorer_session`, explorer_ws.rs:274-430). `?token=` is accepted with a deprecation warning; the `POST /api/v1/ws-ticket` single-use ticket flow exists. No gap.
- **§5 Client messages.** `ExplorerClientMessage` (gyre-common/src/view_query.rs:1138-1163) carries `message` (text + canvas_state), `save_view`, `load_view`, `list_views`, plus `delete_view`/`cancel` extensions. All dispatched in the session loop (explorer_ws.rs:618, 886, 1050, 1237, 1384, 1392). No gap.
- **§6 Server messages.** `ExplorerServerMessage` (view_query.rs:1167-1191): `text{content, done}`, `view_query`, `views`, `status`, plus warning/error extensions. Streaming: `stream_text` (explorer_ws.rs:1550-1666) emits incremental chunks with `done: false` and the final chunk with the terminal flag; the SDK path forwards subprocess text through the same helper. No gap.
- **§21 handler shape.** Matches the spec sketch (`explorer_ws` upgrade → `handle_explorer_session` dispatch loop). No gap.
- **The one real defect at base, fixed by this candidate:** off-contract status values. Three send sites emitted free-form strings ("Thinking..."/"Analyzing..."/"Synthesizing answer...") where §6 mandates exactly `thinking | refining | ready`. Confirmed the frontend hazard is real: `web/src/lib/ExplorerChat.svelte:408-410` maps only those three literals and silently drops anything else. The fix is sound:
  - `STATUS_THINKING/REFINING/READY` constants are now the single source of truth; verified by grep that **no** `ExplorerServerMessage::Status` emission anywhere in the workspace uses a string literal anymore — all nine send sites use the constants, and the only `Status` construction with a variable is the SDK-forward path which routes through `normalize_status()` (explorer_ws.rs:1972).
  - `normalize_status()` (explorer_ws.rs:207-215) is a reasonable containment for the swappable `GYRE_EXPLORER_SDK_PATH` subprocess: case-insensitive substring match on the three spec values, unknown → `thinking`. Unit tests pin pass-through and off-contract mapping.
  - Mapped semantics are defensible: forced synthesis after max tool turns → `refining` (it is the final refinement pass); per-turn "Analyzing..." → `thinking`.
- **Adjacent fix (merged with base's independent fix):** the `&raw_preview[..500]` multibyte panic site now uses the shared `gate_executor::truncate_bytes()` helper (explorer_ws.rs:2935-2941); the byte-slice-truncation exemption file contains no explorer entries, and the gate passes.

### Probes (this sandbox — loopback listeners ARE permitted here, unlike the implementer's sandbox)

- `cargo test -p gyre-server --lib explorer_ws` → **39 passed, 0 failed** (includes the 4 new status-protocol tests; evidence: `review-evidence/lib-tests.txt`).
- `cargo test -p gyre-server --test explorer_ws_integration` → **9 passed, 0 failed** (evidence: `review-evidence/integration-all.txt`). This is real wire-level proof the implementer could not obtain: the new `explorer_ws_status_progression` test drives a full message turn over an actual WebSocket and asserts every observed status ∈ {thinking, refining, ready}, thinking observed, terminal ready observed, final text `done=true`, and `view_query` received. The sibling tests cover list_views (`views` response), save+load round-trip, delete, validation, rate/session limits. The status assertion is a genuine kill-test: reverting any reachable send site to a free-form literal fails the `Off-contract status value on the wire` assert.
- `cargo test -p gyre-server --lib` (full) → **1246 passed, 0 failed** (evidence: `review-evidence/lib-tests-full.txt`).
- Static gates at candidate HEAD: `check-arch.sh`, `check-byte-slice-truncation.sh`, `check-task-commit-attribution.sh` → all EXIT=0 (evidence: `review-evidence/gate-*.txt`).
- No frontend changes in the diff, so no frontend probe needed; the `ExplorerChat.svelte` status mapping (the hazard motivating the fix) verified by source inspection.

### Coverage notes (non-blocking)

- The LLM-loop status sites (per-turn `thinking` at explorer_ws.rs:2856, forced-synthesis `refining` at :3182) are not reachable by the no-LLM integration test; a mock-`LlmPortFactory` harness exists elsewhere (graph_integration.rs) but driving the tool-use/refinement loop through it would require an elaborate scripted tool dance. Regression protection there rests on the constants being the only status sources (grep-verified) plus the four unit tests pinning the constants and wire serialization. Acceptable for this task's audit+gap-fill shape; not a finding.
- Pre-existing, unchanged from base, out of candidate scope (noted for a future task): `stream_text`'s word-boundary fallback (`target = text.len().min(60)`, explorer_ws.rs:1600-1613) can slice at a non-char boundary if the first 60 bytes contain no space/newline and a multibyte character straddles byte 60 — a dynamic-index variant of the byte-slice-truncation hazard class, invisible to the literal-pattern gate. Byte-identical to base; not attributable to this candidate.

### Verdict (round 1)

**approved** — every acceptance criterion is met with real, independently probed evidence: route registered, all four client messages parsed and dispatched, all four server messages sent correctly, status progression thinking→(0–3 refining)→ready with spec-exact values enforced at every send site, streamed text with done-flag semantics, view CRUD over the WebSocket against real storage, auth enforced on upgrade, and the full gyre-server lib suite (1246) plus all 9 WS integration tests pass in this sandbox. No fake implementations, no test inflation, no gate weakening (the exemption file only shrank).
