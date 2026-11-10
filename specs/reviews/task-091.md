# Review — task-091 (HSI §4 Scoped Inline Chat + Hard Interrupt)

Spec: `human-system-interface.md` §4 "Scoped Inline Chat" + "Hard Interrupt".
Candidate under review: `efeabaf22` (base `8b93f242`). Evidence:
`/tmp/stage/review-evidence/task-091-round4/evidence.md`.

## Round 4 — role-based recipient repair (gap d382faa3) + post-remerge record restore

The original task-091 implementation (commits `58e4568`, `a8d036f`) is
already merged into the assignment base; this round's diff is the repair of
requirements gap d382faa3 (workspace-overview priority chat selected its
recipient by name-substring, so a repo orchestrator or a name-matching
worker could be chosen over the real workspace orchestrator, and a
custom-named workspace orchestrator matched nothing) plus the shipped-record
restore after the base remerge. Verified working (no findings):

- **Role-based selection is real and wired end to end.**
  `gyre-domain` `OrchestratorType` (snake_case serde + matching `Display`)
  is exposed by `AgentResponse.orchestrator_type` on every agent list/get
  endpoint; `WorkspaceHome.svelte` filters on
  `orchestrator_type === 'workspace_orchestrator'` (preferring active,
  hidden when none). The outer `OrchestratorAgentResponse` copy was dropped
  so the flattened `AgentResponse` carries the key exactly once — covered by
  the existing strict-parse duplicate-key wire regressions
  (`repo_orchestrator_response_has_no_duplicate_repo_id`,
  `repo_orchestrator_response_serializes_no_key_twice`); the CLI parse shape
  is unaffected.
- **Mutation-verified regression tests.** Reverting the production filter to
  the old name-convention makes BOTH new tests fail ("does NOT select a repo
  orchestrator…", "targets the workspace orchestrator by role…"); restored
  after, byte-identical. The tests kill the bug they claim to kill.
- **Frontend suites green** (`npm ci` first): the four task-091 suites
  (WorkspaceHomeSections, InlineChat, DetailPanelChat, Briefing) — 97
  passed / 41 pre-existing skips; plus all nine other WorkspaceHome-rendering
  suites — 91 passed, no collateral breakage.
- **Server compiles** (`cargo check -p gyre-server` and `--tests`,
  SKIP_WEB_BUILD=1, clean). Server test *execution* deferred to the verifier
  per the 4 GiB stage budget; must-run list recorded in the evidence file
  (`api::agents`, `api::orchestrator`, `api::spawn`, `mcp::`).
- **Byte-identity claim holds**: `git diff 015340baf..efeabaf22 -- crates
  web/src` is empty.
- **Attribution gate passes at the candidate** (`scripts/check-task-commit-attribution.sh`
  OK in a worktree at `efeabaf22`; all three frontmatter commits —
  `3e22e490`, `a8d036f`, `58e4568` — reachable). Note: the checkout's
  working tree carries the stage-injected assigned task body (single-commit
  list), which fails the gate — the candidate's committed record is the
  authoritative one; publication must ship the committed record, not the
  remerge-materialized working-tree body.
- **HSI §4 contract surface confirmed at the candidate**: `InlineChat`
  renders `Message to {recipient} ▸` and exports `focus()`; agent panel chat
  tab + Hard Interrupt row (Pause = `status_update` Directed message with
  `{status: "pause_requested", summary: …}` and a "Pause requested" toggle;
  Stop = confirmation dialog then `POST /admin/agents/:id/kill`; Message =
  open/focus chat); MR chat tab with "Agent completed" + Ask Why fallback;
  Briefing Q&A via `POST /workspaces/:id/briefing/ask`; server
  `send_message` maps Custom + `tier: "directed"` (and `status_update`
  natively) to Directed.

Verifier/integration requirements (not candidate defects):

- Run the deferred server suites listed above.
- `web/dist` at the candidate is byte-identical to the base bundle (no
  `workspace_orchestrator` string) — per the dispositioned bfe34dfa finding
  and stage protocol, dist is intentionally not rebuilt in this assignment;
  **final integration must rebuild `web/dist` from `web/src` before
  publishing/serving**, or the served UI retains name-convention recipient
  selection and gap d382faa3 resurfaces in the shipped bundle.

Verdict: approved (round 4).
