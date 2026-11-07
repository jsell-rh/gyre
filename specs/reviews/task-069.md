# Review — task-069 (Explorer WebSocket Protocol & Server Handler)

Spec: `explorer-implementation.md` §3–6, §20–21.
Comparison base: `ad417f8a5d34d8299065ea2f2d1585d47128b401` → candidate
`657c6fa234c269d5840a49333d2145b54a9d77ab`.
Attributed commits: `dc85eb6a`, `30dc282` — both verified ancestors of the
base; this round is a requirements-changed repair whose only delta is the
task doc (progress label + shipped notes). The merge commit `2dc9e4e4b`'s
tree is byte-identical to base, so the "no code changes required" claim is
accurate and all code verification below is against the retained source at
HEAD (identical to base for every file except `specs/tasks/task-069.md`).

## Verified working

- **§4 Endpoint.** `WS /api/v1/repos/:repo_id/explorer` registered at
  lib.rs:688-689, outside the ABAC-body middleware with documented rationale
  (WebSocket upgrades cannot pass body-reading middleware). Per-handler
  authorization is real and complete: `AuthenticatedAgent` token validation,
  then inside `handle_explorer_session` — repo lookup, workspace→tenant match
  (deny on mismatch/not-found/lookup-error), workspace membership check for
  user tokens, agent workspace-scope check for agent tokens (deny on record
  not found), documented system-token bypass. Every deny path sends an error
  and closes. `POST /api/v1/ws-ticket` issues short-lived single-use tickets
  so real tokens don't leak in URL query params. `check-abac-route-registry.sh`
  passes.
- **§5 Client messages.** All four spec types (`message`, `save_view`,
  `load_view`, `list_views`) parsed and dispatched in the session loop
  (explorer_ws.rs:627, 895, 1059, 1401), plus `delete_view` and `cancel`
  extensions. `ExplorerClientMessage` (view_query.rs:1154-1179) carries the
  spec fields incl. full `canvas_state`.
- **§6 Server messages.** `ExplorerServerMessage` (view_query.rs:1183-1207):
  `Text{content, done}`, `ViewQuery{query, explanation}`, `Views{views}`,
  `Status{status, ...}` (+ Warning/Error extensions). Status protocol uses
  `STATUS_THINKING`/`STATUS_REFINING`/`STATUS_READY` constants — grep confirms
  every send site uses a constant or `normalize_status()`; no free-form
  status literals remain outside tests/comments. `normalize_status()` guards
  the swappable-SDK forward path (unknown → thinking). The frontend
  (ExplorerChat.svelte:397-412) maps only the three spec values, confirming
  the fixed defect (off-contract statuses silently dropped, stale indicator)
  was real.
- **§20–21 Handler.** `explorer_ws` upgrades to `handle_explorer_session`
  with per-user session limits (tenant-scoped keys, oldest-evicted,
  panic-safe `SessionGuard`), message-size and rate limits, view CRUD with
  scoping/creator-or-admin delete. Char-boundary-safe preview truncation via
  `gate_executor::truncate_bytes` (the `&raw_preview[..500]` byte-slice panic
  hazard is gone; the `byte-slice-truncation-exemptions.txt` entry is
  confirmed removed — grep finds no explorer entry).
- **Test evidence (this sandbox, loopback listeners supported):**
  `cargo test -p gyre-server --lib explorer_ws` → 43/43 pass; `cargo test -p
  gyre-server --test explorer_ws_integration` → 11/11 pass over a real
  loopback transport, including `explorer_ws_status_progression` (drives a
  real message turn, asserts every observed status ∈ {thinking, refining,
  ready}, thinking observed, terminal ready, text done=true, view_query
  received — fails on any off-contract status) and
  `explorer_ws_connect_and_list_views` (send `list_views`, receive `views`).
  Static gates at HEAD all pass: arch, byte-slice-truncation,
  abac-route-registry, task-commit-attribution.
- **Coverage matrix:** rows §3–6, §20–21 remain `task-assigned` — no false
  `verified` markings.

## Verdict

**approved** — the task contract is satisfied by real, tested production
code at the candidate. Evidence under `/tmp/stage/review-evidence/task-069-*`.
