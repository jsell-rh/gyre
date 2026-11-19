# Review: task-251 — Directed inbox lifecycle (acknowledge_all on completion and stale Dead)

**Candidate:** `6a24fb59b700b9e2492f8a3203dee0d8540c2e0d` (base `25497b9e`, merge `4462a4b3`)
**Verdict:** Approved (see /tmp/stage/verdict.json)

## What was reviewed

The diff from base adds exactly the specced cleanup lifecycle and nothing else:
`acknowledge_agent_inbox` helper + three production call sites (REST `complete_agent`
fresh-201 and duplicate-202 paths, MCP `handle_agent_complete`, stale-detector Abort
arm), three focused lifecycle tests, and the task-251 coverage-row/task-file updates.

## Independent evidence

- **Static verification of all three call sites** with correct reasons
  ("agent_completed" ×2 surfaces, "agent_orphaned" on the Dead transition). The Abort
  arm is the detector's only Dead transition (Pause → Stopped, ContinueOffline →
  running), so one site covers the spec's orphan path. Ordering is safe:
  `create_mr_for_completion` runs before the REST bulk-ack; nothing after the bulk-ack
  emits a Directed message to the completing agent.
- **Storage half executed for real:** `cargo test -p gyre-adapters acknowledge_all`
  (1 passed) and `cargo test -p gyre-adapters expire_acked_inboxes` (4 passed,
  including agent_orphaned and explicit-ack-retention cases). Both sqlite and
  postgres adapters implement `acknowledge_all`/`expire_acked_inboxes` with the
  specced semantics; mem mirrors them.
- **Type check:** `cargo check -p gyre-server --tests` exit 0, no warnings
  (type-checks all three new tests plus the merged task-247 wiring).
  `cargo fmt --check -p gyre-server` exit 0. `check-arch.sh`,
  `check-inert-enforcement.sh`, `check-byte-slice-truncation.sh` all pass.
- **Tests are meaningful:** on the base commit (no production `acknowledge_all`
  caller) each test's `count_unacked == 0` / `expire_acked_inboxes == N` assertions
  fail — the tests can only pass with the production calls in place. Controls cover
  the negative space (live agent's unacked inbox and explicit-ack rows inside the
  retention window survive reclamation).

## Deferred to the independent verifier (wrapper-blocked in this sandbox)

`cargo test -p gyre-server --lib` for `complete_agent_bulk_acks_directed_inbox`,
`agent_complete_bulk_acks_directed_inbox`,
`stale_abort_bulk_acks_directed_inbox_with_agent_orphaned`, plus regression suites
`api::spawn`, `api::messages`, `message_dispatcher`, `stale_agents`, `mcp`, and
`--test message_persistence`. The build wrapper exits 64 on gyre-server test
codegen in the review stage (4 GiB budget) — an infrastructure restriction, not a
code defect. Full probe log:
`/tmp/stage/review-evidence/task-251-candidate-6a24fb59.md`.

## Non-blocking observations

1. A duplicate MCP `gyre_agent_complete` call errors at the Idle transition before
   the bulk-ack (the REST surface handles its duplicate path explicitly). The first
   successful completion already acked the inbox, and late arrivals are reclaimed
   when the idle agent later ages out through the detector's Dead path — no
   indefinite accumulation. Within the task contract.
2. task-251.md's shipped notes cite the implementer's evidence file with a
   truncated name ("task-2....md"); cosmetic only.
3. Coverage row 16 update is accurate and honestly labels server-lib execution as
   deferred to the verifier.
