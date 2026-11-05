# Review — task-069 (Explorer WebSocket Protocol & Server Handler)

Spec: `specs/system/explorer-implementation.md` §3–6, §20–21.
Base `722179f0` → candidate `4ca3692a` (net diff: `explorer_ws.rs` +118/−37,
`explorer_ws_integration.rs` +81, `specs/tasks/task-069.md` docs).
Verdict: **approved**.

## What the candidate ships

The WS surface (route, auth, all 4 client + 4 server message types, view
CRUD, streaming) pre-existed at base and is unchanged in substance. The
candidate is an audit + one real gap-fill: **status protocol conformance**.
Three send sites emitted free-form strings ("Thinking...", "Analyzing...",
"Synthesizing answer...") where spec §6 mandates exactly
`thinking | refining | ready`; the frontend (`ExplorerChat.svelte:408-410`)
maps only those three and silently drops anything else, leaving the status
indicator stale. The fix introduces `STATUS_THINKING`/`STATUS_REFINING`/
`STATUS_READY` constants used at all nine send sites, plus
`normalize_status()` guarding the swappable `GYRE_EXPLORER_SDK_PATH`
subprocess forward path. An adjacent UTF-8 byte-slice panic
(`&raw_preview[..500]`) was fixed via the shared `truncate_bytes()` helper
and its exemption entry removed.

## Independent evidence (all at exact candidate `4ca3692a`)

- **Route registered**: `crates/gyre-server/src/lib.rs:679-680` —
  `GET /api/v1/repos/:repo_id/explorer → explorer_ws::explorer_ws`, auth via
  `AuthenticatedAgent` extractor (`?token=` + ws-ticket flow also supported,
  with in-handler tenant/workspace/membership checks since the route sits
  outside the ABAC-body middleware).
- **Client dispatch**: all four spec messages (`message`, `save_view`,
  `load_view`, `list_views`) plus `delete_view`/`cancel` extensions are
  matched in the session loop (`explorer_ws.rs:618,886,1050,1392,1237,1384`).
- **Frontend claim verified**: `ExplorerChat.svelte:408-410` maps exactly the
  three spec values; anything else leaves `status` unchanged — the defect
  description is accurate.
- `cargo test -p gyre-server --lib explorer_ws` → **39 passed, 0 failed**
  (includes the 4 new status-protocol tests).
- `cargo test -p gyre-server --test explorer_ws_integration` →
  **9 passed, 0 failed** (all 8 pre-existing + new
  `explorer_ws_status_progression`). Note: the loopback listener **is**
  supported in this sandbox (`/tmp/stage/capabilities.json`,
  `tcp_listener_probe: supported`), so the implementer's "cannot run here,
  deferred to CI" claim was outdated for this environment — the tests were
  run directly and pass. The test drives a full message turn on the no-LLM
  fallback path and asserts: every observed status ∈ {thinking, refining,
  ready}, thinking observed, terminal ready observed, final text `done=true`,
  and a `view_query` received.
- **Mutation probe (test is not self-confirming)**: temporarily changed
  `STATUS_THINKING` back to `"Thinking..."` and re-ran
  `explorer_ws_status_progression` → **FAILED** at
  `explorer_ws_integration.rs:434` with `Off-contract status value on the
  wire: "Thinking..." (spec §6: thinking | refining | ready)`, exit 101.
  Working tree fully restored to the exact candidate afterwards
  (`git status` clean, `git diff HEAD --stat` empty).
- Static gates: `check-arch.sh`, `check-byte-slice-truncation.sh` (the
  explorer_ws exemption entry is gone — grep count 0),
  `check-abac-route-registry.sh`, `check-task-commit-attribution.sh` —
  all exit 0.

## Notes (non-blocking)

- `normalize_status()` maps any unknown substring-match heuristically
  (e.g. a hypothetical "not ready yet" from an alternate SDK script contains
  "ready" → `ready`). The wire still carries only spec values; the heuristic
  applies solely to off-spec third-party subprocess output. Acceptable
  design trade-off, documented in the source comment.
- Evidence files under `/tmp/stage/review-evidence/task-069-*`.
- Full `cargo test --all` and all-target clippy are left to verification
  per review scope; the focused suites covering every changed line are green.
