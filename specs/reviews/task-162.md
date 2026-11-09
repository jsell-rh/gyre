# Review — task-162: Message bus MessageConsumer dispatcher and event TTL expiry job

**Candidate:** 7eed81d4be1b7369ae88d1ca872a7af303090079 (base 7373979e8)
**Reviewer round:** 2026-11-09 (fifth review round; prior findings 73a69995, e7aa777ea, c8626b1b3, 3d2b75ed)
**Verdict:** Approved (verdict.json, empty findings)

## Acceptance criteria — independently verified

All six criteria verified against production code at the candidate, with
mutation probes proving the tests kill real regressions (evidence under
/tmp/stage/review-evidence at review time):

1. **spawn_message_consumer()** (message_dispatcher.rs) takes single-owner
   possession of the 256-capacity mpsc receiver via
   `AppState::take_message_dispatch_rx()` and spawns the drain task; wired in
   main.rs. Second call is a logged no-op.
2. **NotificationBridge** (first registered MessageConsumer) derives Inbox
   notifications from GateFailure (p3, MR author agent's spawning user,
   tenant resolved from the workspace record), BudgetWarning (p7,
   Admin/Developer/Owner), AgentError (p5 AgentEscalation, spawning user),
   ReconciliationCompleted (p6 MetaSpecDrift, Admin/Developer/Owner). All
   three send paths (AppState::emit_event, api/messages.rs, mcp.rs
   message.send) clone into the channel after storing; try_send failures log
   and never block.
3. **Event TTL expiry**: spawn_message_expiry hourly job (3600s,
   MissedTickBehavior::Delay) calls expire_events(now − GYRE_EVENT_TTL_SECS·1000);
   registered as `message_expiry` in the admin job registry.
4. **Dead inbox cleanup**: expire_acked_inboxes(now −
   GYRE_DEAD_INBOX_TTL_SECS·1000); MemMessageRepository now tracks ack
   reasons so its contract matches the SQLite adapter (only
   agent_completed/agent_orphaned bulk-acks cleaned; explicit acks and
   unacked Directed messages retained — asserted by test).
5. **Env vars**: message_expiry_env_ttl_respected proves a 1s TTL deletes
   only >1s-old events; both default to 604800 with warn-on-invalid fallback.
6. **Tests**: full `cargo test -p gyre-server --lib` at candidate —
   **1529 passed, 0 failed**; `cargo check -p gyre-server --bins --tests`
   clean. (gyre-adapters/ports/common untouched by the diff.)

## Mutation probes (all restored; working tree clean at verdict)

- **A — bridge disabled** (`on_message` no-op): 5 message_dispatcher tests +
  gate_executor bridge test + reconciliation wave test all FAIL — the
  end-to-end chains (run_gate → emit → dispatcher → bridge → p3; wave → p6)
  are real.
- **B — zero consumers registered**: spawn_message_consumer_end_to_end FAILS
  (wiring is load-bearing; direct-bridge unit tests rightly still pass).
- **C — expiry job neutered**: both TTL tests FAIL, including the env-var
  override test.

## Focused suites

message_dispatcher 9/9, gate_executor 25/25, reconciliation wave-completion
1/1. Mechanical checks clean: arch, dead-message-kinds,
task-commit-attribution, mem-port-contracts, abac-route-registry.
check-notification-test-coverage.sh exits 2 on this host (mawk 1.3.4 lacks
gawk 3-arg match()) — identical at pristine base; host tooling, unrelated.

## Process note — frontmatter regression, fifth recurrence (non-blocking)

The assigned head 7eed81d4b is again an executor checkpoint ("process:
record task-162 branch commits") that rewrote `commits:` back to the two
task-labeled commits only, dropping the five merge commits that f26e4c434
had listed. Unlike prior rounds, the shipped durable note (task-162.md,
"fourth recurrence" section) pre-declares exactly this state ("that
frontmatter state is wrong, not this record") and makes no false claim at
head — the prior findings' defect class (inaccurate claims at head) is
cured. Mitigations independently verified this round: the full
base..candidate diff (which contains every merge resolution) was reviewed;
main.rs at the candidate retains `bootstrap_default_tenant`,
`seed_builtin_personas`, and `message_dispatcher::spawn_message_consumer`/
`spawn_message_expiry` wiring together; the task-owned main.rs wiring is in
listed commit 51221dc02; check-task-commit-attribution.sh passes (merges are
not task-labeled). The HSI §8 p3 amendment is recorded in the spec's change
table (spec-lifecycle form), keeping message-bus.md and HSI consistent.
