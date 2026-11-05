# Review — task-069 (Explorer WebSocket Protocol & Server Handler)

Spec: `explorer-implementation.md` §3–6, §20–21.
Comparison base: `3c39ab1f796bcf52ac0b95d60a4101e27aa69fe8` → candidate `34cee70d`.
Task source commit: `30dc2826` (per task frontmatter; verified — the only
product-surface commit in the range, the rest are merge/chore bookkeeping).
Diff scope: `explorer_ws.rs` (+118/−37), `explorer_ws_integration.rs` (+78),
`specs/tasks/task-069.md` (doc only). No unrelated files touched.

## Round 1

### Audit of the pre-existing surface (task is audit + gap-fill)

All acceptance-criteria items verified present and real at the candidate HEAD:

- **Route registered** at `GET /api/v1/repos/:repo_id/explorer`
  (lib.rs:677–679). WebSocket upgrades cannot pass body-reading ABAC
  middleware, so auth is enforced in-handler: `AuthenticatedAgent` extractor
  (token-validated), repo lookup → workspace → tenant chain with an explicit
  access-denied error on mismatch (explorer_ws.rs `handle_explorer_session`),
  plus a `POST /api/v1/ws-ticket` short-lived single-use ticket flow so real
  tokens don't ride query parameters. `check-abac-exempt-handlers.sh` passes
  (89 handlers checked).
- **Client messages**: `ExplorerClientMessage` (view_query.rs:1138) has all
  four spec types (`message` + `canvas_state`, `save_view`, `load_view`,
  `list_views`) plus `delete_view`/`cancel` extensions; the session loop
  dispatches every variant (explorer_ws.rs:618, 886, 1050, 1237, 1392).
- **Server messages**: `ExplorerServerMessage` (view_query.rs:1167) has all
  four spec types (`text` with `done: bool`, `view_query`, `views`,
  `status`) plus `warning`/`error`. Streaming sends `done: false` per chunk
  (line 1616, `done: is_last && done` at 1657) and `done: true` at completion
  (1195, 3054, 3114).
- **View CRUD over WebSocket**: `save_view` persists via the saved-views port
  (with workspace-scope handling), `load_view` sends back `view_query`,
  `list_views` returns `views`. Proven live by the integration suite below.
- **Status progression**: `MAX_REFINEMENT_TURNS = 3`; `thinking` on agent
  start, `refining` in the dry-run self-check loop, `ready` after the final
  view query is emitted.

### The candidate's delta — status protocol conformance (real defect, real fix)

The one genuine gap found by the audit: three send sites emitted off-contract
status strings ("Thinking..."/"Analyzing...", "Synthesizing answer...") where
spec §6 mandates exactly `thinking | refining | ready`, and the frontend
(`ExplorerChat.svelte`) maps only those three — anything else leaves the
status indicator stale. The fix:

- `STATUS_THINKING`/`STATUS_REFINING`/`STATUS_READY` constants as the single
  source of truth; all nine status send sites in `explorer_ws.rs` use them
  (verified by grep — zero remaining off-contract literals; the only
  `ExplorerServerMessage::Status` construction sites are the shared
  `send_status_full` helper and a test).
- `normalize_status()` guards the swappable SDK subprocess forward path
  (`GYRE_EXPLORER_SDK_PATH`) so an alternate script's free-form statuses are
  mapped into the protocol before hitting the wire.
- Adjacent panic hazard fixed in the same path: the `&raw_preview[..500]`
  byte-slice (panicked on multibyte UTF-8 at the boundary) now routes through
  the shared `gate_executor::truncate_bytes()`. The base independently fixed
  this site too; the merge resolution correctly kept the shared helper, and
  the corresponding `byte-slice-truncation-exemptions.txt` entry is gone
  (exemptions shrink). `check-byte-slice-truncation.sh` passes.

### Probes (this sandbox, 2026-10-10; evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib explorer_ws` → **39 passed, 0 failed**
  (includes the 4 new: constants match spec literals exactly; `Status`
  serializes spec values end-to-end through the serializer; `normalize_status`
  passes known values through and maps off-contract literals to valid values).
- `cargo test -p gyre-server --test explorer_ws_integration` → **9 passed,
  0 failed, live** — including the new `explorer_ws_status_progression` (every
  observed status ∈ {thinking, refining, ready}, thinking observed, terminal
  ready observed, text done=true + view_query received) and the pre-existing
  `connect_and_list_views`, `save_and_load_view`, `delete_view`,
  `save_view_validation`, `rate_limiting`, `session_limit_per_server_instance`.
  Note: the implementer reported loopback `accept()` as seccomp-blocked in
  their sandbox and deferred these to CI; in this sandbox the listener probe
  is supported and the full suite runs and passes — recorded here as the
  primary evidence, no CI deferral needed.
- **Mutation probe (restored, byte-identical after):** reintroducing the
  historical off-contract literal `"Thinking..."` at the session-loop status
  send makes `explorer_ws_status_progression` fail with
  `Off-contract status value on the wire: "Thinking..."` — the test genuinely
  kills the bug class on the exercised path, it is not self-confirming.
- Static gates at candidate HEAD: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-task-commit-attribution.sh`,
  `check-inert-enforcement.sh`, `check-byte-slice-truncation.sh` — all pass.

### Merge-scope check

Base `3c39ab1f` independently fixed the same truncation site and refactored
the session registry into AppState-owned `ExplorerSessionRegistry`; the merge
resolution is correct (shared helper kept, status work intact) and the
base→candidate diff touches only the three expected files.

### Verdict (round 1)

**approved** — the audit verified every acceptance criterion against real
code, the one real gap (off-contract status values) is fixed with a single
source of truth plus a normalization guard on the swappable subprocess path,
all focused tests pass live in this sandbox, and a mutation probe proves the
new integration test fails when the defect is reintroduced. No findings.
