# Review — task-119 (Implement budget enforcement cascade)

Spec: `specs/system/agent-runtime.md` §4 (Budget Enforcement: Cascade, Enforcement
Levels, What's Tracked, Budget Reset) and §5 (Agent Prompt Structure).
Candidate: `c42e9a2d` (branch `pipeline/task-119/d3607aace6bc498eb1c77a59a7a64a7e-1`,
base `7cf2f81c`; product commits 4bfdf85b, 7bdc6060, bfb2dc3d, 29d1a961).
Verdict: **needs-revision** — two material findings (F1, F2); the rest of the task
contract is genuinely implemented and verified.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters --lib` → 447 passed, 0 failed; focused budget subset
  (list_since window, compute_minutes accumulation, repo budget override persistence) → 6 passed.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (4m09s).
- `cargo test -p gyre-server --lib budget` → refused by the executor's build wrapper
  (gyre-server test compilation exceeds the 4 GiB sandbox budget). Infrastructure
  restriction, not a code defect. Verifier must run:
  `cargo test -p gyre-server --lib budget`, `... lifetime`, `... repo_budget`,
  `... mcp_prompts`, `... prompt`, then `cargo test --all`.
- Gate scripts all pass: arch, ABAC registry, MCP write tools, dead message kinds,
  migration versions + SQL portability, inert enforcement, mem port contracts,
  unbounded HTTP, in-memory stores, scope literals, forged scope fields. `cargo fmt --check` clean.
- `check-task-commit-attribution.sh` fails ONLY because the controller reset the
  working-tree copy of `specs/tasks/task-119.md` to `commits: []`; the committed
  candidate records all four task-labeled product commits (verified with the
  script's own parsing against `git show c42e9a2d:specs/tasks/task-119.md`).

## Verified working (no findings)

- **Cascade validation is real.** PUT /workspaces/:id/budget validates all four
  BudgetConfig fields ≤ tenant ceiling (400); PUT /repos/:id validates every set
  field of the repo override against the EFFECTIVE workspace limit (min of
  workspace config and tenant ceiling — dimensions the workspace leaves unset
  inherit the tenant); `{}` clears → inherit; violations are not persisted.
  repo max_agents ≤ workspace max_agents_per_repo (task-118, present).
- **Repo roll-up is real.** Agent usage reports charge `repo:{id}` entries
  (repo resolved from agent.repo_id or the current task) and roll up to
  workspace + tenant; spawn checks run on both REST spawn and the Phase-4
  scheduler, with repo-level token/cost/concurrent caps enforced live.
- **Lifetime enforcement is real and correctly bounded.** Effective limit =
  min(repo override, effective workspace limit) recomputed per pass, so an
  override can tighten but never loosen the cascade and later reductions bind
  immediately; budget-kill semantics (worktree preserved, task InProgress,
  token revoked, AgentKilled audit) hold; runs after every report and hourly.
- **80% / 100% / beyond-limit machinery exists** (notifications with per-period
  dedup, BudgetExhausted to agent inboxes via Destination::Agent, one grace
  timer per workspace with a re-check before killing that fails closed,
  immediate kill on budget PUT below usage — tested through the real router).
- **Compute minutes** charged on every terminal transition (complete REST+MCP,
  fail, stop, admin kill, stale-detector death, budget kill) in whole minutes,
  surviving alongside token/cost counters (adapter test proves no clobbering).
- **Rolling 24h reset** recomputes counters from per-call records ≥ now−24h
  (hourly job; `reset_daily_counters` port method removed; all three adapters
  implement `list_since`).
- **§5 prompt structure + MCP prompts**: GYRE_PROTOCOL_PROMPT injected at spawn;
  prompt-set record pins the persona section only; `system://persona` serves the
  pinned assembly verbatim (end-to-end test asserts it contains the seeded
  persona and NOT the protocol text); `system://protocol`, `system://context`,
  `system://constraints` implemented and agent-scoped; prompts capability
  advertised. The "priority 7" → "High, rank 4" spec edit is a faithful
  reconciliation with user-management.md's canonical notification taxonomy.
- Tests are substantive (real stores/router, state-transition assertions,
  worktree-preservation and task-InProgress assertions); no self-confirming or
  assertionless tests found in the task diff.

## Findings

- [-] **F1 (major, agent-runtime.md §4 Enforcement Levels): overshooting normal
  exhaustion bypasses the 60s graceful stop.** `enforce_workspace_budget`
  (api/budget.rs:767) takes the beyond-limit immediate-hard-kill branch for ANY
  `pct > 1.0`. The spec is explicit that beyond-limit means the budget was
  *reduced below current usage* ("The 100% threshold handles normal exhaustion;
  beyond-limit handles external budget changes"). When a usage report crosses the
  limit with overshoot — the normal case, e.g. 950 → 1150 against a 1000-token
  limit — agents are hard-killed with `grace_secs: 0` and never receive the
  specced 60s commit window; the grace branch (pct >= 1.0) is only reachable when
  usage lands exactly on the limit, and any LLM call made during a started grace
  period re-enters enforcement above 1.0 and kills immediately, making "agents
  completing within the grace period are marked Idle" effectively unreachable.
  No test covers the overshoot crossing (the exhaustion test seeds usage at
  exactly 100%). Repair: reserve the immediate-kill branch for budget-reduction
  triggers (the PUT path / config-change detection) and route report-driven
  crossings ≥ 100% — including overshoot — into the graceful-stop branch.
- [-] **F2 (agent-runtime.md §4 What's Tracked): user-initiated specs/assist
  queries are charged to the repo budget.** `record_llm_call_usage`
  (api/budget.rs:603) charges the repo whenever `repo_id` is set, regardless of
  `usage_type`; the REST specs/assist (api/specs_assist.rs:468) and MCP
  spec_assist (mcp.rs:2846) handlers pass `repo_id: Some(repo.id)` with
  `usage_type: "llm_query"`. The spec says llm_query is "Charged to workspace
  budget directly (no repo-level tracking — user queries have no repo context)".
  User queries can now exhaust a repo budget and block agent spawns
  ("repo budget exceeded"). The coverage note (coverage/system/agent-runtime.md
  row 30) claims these paths charge "the workspace directly — no repo entry",
  which misstates the code. Repair: charge the repo only for agent usage
  (usage_type "agent_run") or stop passing repo_id on the llm_query paths, and
  correct the coverage note.

Minor (non-blocking, recorded for completeness):

- jobs.rs `spawn_budget_reset` description string contains a long literal run of
  spaces from a bad string-continuation reflow ("re-syncs token/cost______
  counters").
- `system://constraints` reports the RAW workspace budget config, not the
  effective (tenant-min) limits enforcement actually uses — informational only.
- `prompts/get` for an unknown name returns `{"error": ...}` inside the JSON-RPC
  result rather than a JSON-RPC error object; harmless for current clients.

## Test-inflation check

None found. The new tests drive real stores and the real router, assert agent
status transitions, inbox delivery destinations, worktree preservation, task
status, and persisted repo budget state; they fail if the corresponding
production behavior is disabled. The one untested behavior is the F1 overshoot
crossing — recorded as part of F1, not as inflation.

## Verifier-required probes

```
cargo test -p gyre-server --lib budget
cargo test -p gyre-server --lib lifetime
cargo test -p gyre-server --lib repo_budget
cargo test -p gyre-server --lib mcp_prompts
cargo test -p gyre-server --lib prompt
cargo test --all
```
