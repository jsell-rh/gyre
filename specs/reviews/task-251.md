# Review — task-251 (Directed inbox lifecycle: acknowledge_all on agent completion and on stale-agent Dead transition)

Spec: `specs/system/message-bus.md` §Delivery Guarantees ("Agent completion cleanup") and §Edge Cases (orphaned agents).
Candidate under review: `34d13815` (feat `d2983bed`, base `fb1424fb` = task-247 wiring).
Verdict: **approved** — all three assigned production call sites are real and correct; test execution deferred to the independent verifier per the review-stage build budget (recorded below).

## What was verified

- **REST completion** (`api/spawn.rs::complete_agent`): calls the new shared `acknowledge_agent_inbox(state, &agent.id, "agent_completed")` on the fresh-completion (201) path immediately after the Idle transition, and on the duplicate-complete (202) path so Directed messages arriving after a first completion are reclaimable (matches §Edge Cases "Send to dead/completed agent"). Helper is best-effort by design: storage failure logs a warning and leaves the inbox unacked/redeliverable — the safe side of at-least-once.
- **MCP parity** (`mcp.rs::handle_agent_complete`): same helper with `"agent_completed"` inside the `agents.update` Ok arm, before MR creation. `gyre_agent_complete` was already in the `needs_write` RBAC gate; no new tools.
- **Orphan cleanup** (`stale_agents.rs::run_once`, Abort arm): `acknowledge_agent_inbox(state, &agent.id, "agent_orphaned")` right after the Dead transition — exactly the specced detector path.
- **Adapter contract consistent** (unchanged by this task): mem and Diesel sqlite/postgres `acknowledge_all` record the passed reason; `expire_acked_inboxes` deletes only agent-targeted rows acked `agent_completed`/`agent_orphaned` past the cutoff, so the tests' deletion-count assertions also prove the recorded reason.
- **Tests are hard, not self-confirming**: each drives a real surface (axum router `POST /agents/:id/complete`, `/mcp` `tools/call gyre_agent_complete`, `stale_agents::run_once`) and asserts `count_unacked == 0` + audit retention + exact reclamation counts. On the base commit no production path calls `acknowledge_all`, so each test's `count_unacked` assertion fails there (verification intent satisfied). Negative controls: a live agent's unacked message and an explicit-ack message inside the retention window survive expiry.
- **Scope respected**: diff touches only the three call-site files + their tests + task/coverage bookkeeping (row 16 of `specs/coverage/system/message-bus.md`, accurately worded). No routes, MCP tools, MessageKinds, migrations, or durable state added.

## Probes (evidence: `/tmp/stage/review-evidence/task-251-review.md`)

- `cargo check -p gyre-server --tests` → **exit 0** (clean, no warnings; 9m58s cold). All three new tests type-check.
- `scripts/check-arch.sh`, `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh` → all exit 0.
- `cargo fmt -p gyre-server -- --check` → clean.
- `scripts/check-task-commit-attribution.sh` → **exit 0 on the candidate commit** (verified in a clean `git worktree` at `34d13815`; the failure against the review working tree was caused by the controller's unstaged revert of `specs/tasks/task-251.md` to `commits: []` — the committed candidate lists `d2983bed`; working-tree artifact, not a candidate defect).

## Deferred to the independent verifier (gyre-server test execution is wrapper-blocked in review stage)

    cargo test -p gyre-server --lib complete_agent_bulk_acks_directed_inbox
    cargo test -p gyre-server --lib agent_complete_bulk_acks_directed_inbox
    cargo test -p gyre-server --lib stale_abort_bulk_acks_directed_inbox_with_agent_orphaned
    cargo test -p gyre-server --lib api::spawn
    cargo test -p gyre-server --lib api::messages
    cargo test -p gyre-server --lib message_dispatcher
    cargo test -p gyre-server --lib stale_agents
    cargo test -p gyre-server --lib mcp
    cargo test -p gyre-server --test message_persistence   (task-247 suite, AC5 restart persistence)

## Findings

- (none blocking)

## Non-blocking observations (forward notes, not defects of this candidate)

1. **Admin-kill path inbox cleanup**: `kill_agent` (api/admin.rs) and compose.rs also transition agents to Dead without bulk-acking, while message-bus.md line 318 says an inbox is freed when the agent is "completed/killed". Pre-existing on base and outside this task's assigned scope (the task names exactly the completion surfaces + the stale detector, and §Edge Cases attributes orphan cleanup to the detector) — recommend a follow-up task for kill-path `acknowledge_all("agent_orphaned")`.
2. Coverage row 16 cites evidence at an ephemeral `/tmp/stage/review-evidence/task-2....md` path (truncated-looking name) inside a committed doc — cosmetic; the review-stage evidence file is `/tmp/stage/review-evidence/task-251-review.md`.
3. The review working tree carries the controller's unstaged revert of `specs/tasks/task-251.md` (progress/commits reset); the committed candidate is intact and is what this verdict applies to.
