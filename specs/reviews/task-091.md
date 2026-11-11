# Review — task-091: HSI Scoped Inline Chat + Hard Interrupt

**Candidate:** cc566ce7de52263e5de1aa1fb4a9bbacc7d447cf
**Base:** 65f71e657025677bc39bb349a214ad4099439065
**Round:** 5 (independent review)
**Verdict:** APPROVED

## Scope of this round

The assignment base already contains the merged, earlier-verified rounds of
task-091 (InlineChat component, DetailPanel chat + hard interrupt, MR chat,
Briefing Q&A, Directed-tier delivery). This candidate's delta is:

1. **Role-based orchestrator recipient repair** (3e22e490, requirements gap
   d382faa3): `AgentResponse.orchestrator_type` exposed on every agent
   list/get endpoint; `WorkspaceHome.svelte` selects the workspace
   orchestrator by `orchestrator_type === 'workspace_orchestrator'` instead
   of name-substring; `OrchestratorAgentResponse` carries the key exactly
   once via the flattened `AgentResponse`.
2. **rustfmt fix** (76b3da22e): the sole failure of verification run 635d3a33.

## Evidence (logs under /tmp/stage/review-evidence/task-091-round5/)

- `npm ci` + `npx vitest run` on the four task-091 suites
  (WorkspaceHomeSections, DetailPanelChat, InlineChat, Briefing):
  **4 files, 97 passed | 41 skipped, exit 0** (skips are pre-existing
  `describe.skip` blocks).
- **Mutation probe:** reverting the role filter to the old name-substring
  convention makes exactly the two new d382faa3 regression tests fail
  (2 failed | 38 passed); source restored clean afterward. The tests kill
  the production behavior — not self-confirming.
- `cargo fmt -p gyre-server --check`: **clean** (635d3a33 failure fixed).
- `cargo check -p gyre-server`: **clean** (7m16s); build.rs web rebuild
  succeeded, proving current `web/src` builds (regenerated dist restored).
- `scripts/check-task-commit-attribution.sh`: passes against the committed
  candidate tree (the working tree's executor-staged uncommitted edit trims
  the commits list and fails the script on the working tree only — not
  candidate content).
- Contract inspection: human steering/priority/MR-feedback messages send
  Custom `user_message` with `tier: "directed"` (server opts Custom into
  Directed); Pause sends `status_update` (natively Directed) with
  `{status: "pause_requested", summary: "Human requested pause"}`; Stop
  confirms then calls `POST /admin/agents/:id/kill`; Message focuses the
  inline chat; `GET /agents/:id/messages` (message.poll) drains persisted
  Directed messages; `origin_from_auth` allows User-origin Directed→Agent
  delivery; all four scoped-chat contexts render the recipient indicator
  (`Message to X ▸`), with real assertions in the passing suites.

## Verifier instructions (server tests, not run here per stage budget rules)

- `cargo test -p gyre-server --lib api::agents` — must include the new
  `agent_response_exposes_orchestrator_type` (compiles via cargo check;
  assertions match the `AgentResponse::from` path).
- `cargo test -p gyre-server --lib api::orchestrator api::spawn` and the
  mcp suites.
- Full `npm test` in `web/` (four task-091 suites already pass here;
  ExplorerCanvas/FlowRenderer/MoldableView/ExplorerViewAskViewSpec failures
  are pre-existing on main).

## Notes

- Committed `web/dist` predates the role-based repair; per the standing
  pipeline protocol (bfe34dfa disposition) dist is executor-restored from
  base and rebuilt/published by final integration. The shipping source is
  correct and builds cleanly.
