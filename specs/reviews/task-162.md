# Review — task-162 (Message bus — wire MessageConsumer dispatcher and event TTL expiry job)

Spec: `specs/system/message-bus.md` §"Relationship to Notifications", §"Implementation Notes".
Commits under review (candidate `c409c3bb7d`, base `3af80bd21a`):
`51221dc02d235b9737d645d541650e771d3e89e5`, `5192ff750f58b79d2553a317e9d9b802292ad3e1`,
`6d20863c02e07c0a51873fdbb735c61bef80af68` (+ process/merge commits).
Verdict: **needs-revision** — one material finding (F1, duplicate GateFailure
notifications); everything else verified genuine. Evidence:
`/tmp/stage/review-evidence/review-round1.md` (+ per-probe logs).

## Round 1

Probes (candidate HEAD, this sandbox): `cargo check -p gyre-server` → clean
(8m14s); all 18 mechanical invariant scripts (arch, mem-port-contracts,
dead-message-kinds, in-memory-state-stores, inert-enforcement, byte-slice,
relative-path, fail-open-ref-resolution, fabricated/scope-literal defaults,
lossy-secret, forwarded-header, forged-scope, unbounded-http, migration
versions/portability, abac-route-registry, mcp-write-tools) → OK. gyre-server
test compilation deferred to the verifier (4 GiB budget): verifier must run
`cargo test -p gyre-server --lib message_dispatcher`, `--lib gate_executor`,
`--lib notifications`, `--lib reconciliation`, and `cargo test --all`.

Verified working (no findings):

- **Dispatcher is real and single-owner.** `spawn_message_consumer()`
  (message_dispatcher.rs) takes the 256-capacity mpsc receiver via
  `AppState::take_message_dispatch_rx()` (second call = logged no-op) and
  spawns the drain task; `MessageDispatcher` holds
  `Vec<Arc<dyn MessageConsumer>>`; wired in main.rs after build_state.
  `emit_event` uses non-blocking `try_send` with a warn log on a full channel,
  exactly per spec.
- **NotificationBridge is a genuine consumer.** Registered first; derives
  rank-3 GateFailure (MR → author agent → spawner chain; tenant from the
  workspace record, skip-and-log when unresolvable — no fabricated "default"),
  rank-4 AgentBudgetWarning (Admin/Developer/Owner; Viewer excluded), rank-2
  AgentEscalation (spawner), rank-6 MetaSpecDrift (Admin/Developer/Owner), all
  through the shared `notify_rich` creation path with structured source and
  deep links.
- **TTL job is real.** `spawn_message_expiry()` hourly (3600s,
  MissedTickBehavior::Delay, immediate first tick), registry-tracked as
  `message_expiry` with on-demand `POST /admin/jobs/message_expiry/run`;
  calls `expire_events` and `expire_acked_inboxes` with
  `GYRE_EVENT_TTL_SECS`/`GYRE_DEAD_INBOX_TTL_SECS` (defaults 604800,
  warn-on-invalid fallback, read per cycle). No double scheduling
  (register-only in the registry; spawn loop is the sole executor).
- **mem adapter contract alignment.** `acknowledge`/`acknowledge_all`/
  `expire_acked_inboxes` now mirror the SQLite ack_reason contract (only
  `agent_completed`/`agent_orphaned` bulk-acks are cleaned; explicit acks and
  unacked Directed retained) — check-mem-port-contracts passes.
- **Old sync paths cleanly removed.** `notify_gate_failure` fully deleted (no
  dangling references); `emit_reconciliation_completed` inline fan-out removed;
  consumers updated to poll the async bridge (reconciliation wave test drains
  the bus through the real dispatcher).
- **Spec amendment is faithful.** HSI §8 p3 now routes GateFailure via
  MessageConsumer, consistent with message-bus.md §Relationship to
  Notifications; the HSI↔message-bus amendment table records it.
- **Tests are real, not inflation.** The end-to-end test drives run_gate →
  emit_event → dispatcher → bridge and asserts exactly one rank-3
  notification; TTL tests pin retention boundaries (expired Event deleted;
  fresh Event / unacked Directed / explicit-ack retained) plus env override;
  env-mutating tests serialize on a mutex. Each fails if the specced behavior
  breaks.

Findings:

- [-] **F1 (major): duplicate rank-3 GateFailure notifications per failed
  gate.** `NotificationBridge::on_message` matches `MessageKind::GateFailure`
  with no destination/tier filter. Two production emitters publish
  GateFailure messages for the same failed gate: `gate_executor.rs` run_gate
  (~line 127, Workspace-targeted event, pre-existing) and `ralph_loop.rs`
  `on_gates_failed` (~line 308, Agent-targeted copy emitted for the agent's
  next session, same `mr_id`/`gate_name` payload and workspace). The standard
  autonomous flow reaches both (enqueue triggers gates synchronously; the
  ralph watcher then reacts to the failure), so the MR author's spawning user
  now receives TWO identical rank-3 notifications per failed gate — before
  this candidate they received exactly one (the removed synchronous
  `notify_gate_failure` path). This contradicts the candidate's own HSI §8 p3
  amendment ("single creation path"). The new end-to-end test only exercises
  run_gate directly, so it cannot catch the duplicate. Repro test and full
  analysis in `/tmp/stage/review-evidence/review-round1.md` (fails with
  `notifs.len() == 2` on the candidate). Repair: consume only
  Workspace/Broadcast-targeted GateFailure events in the bridge (the
  Agent-directed copy is the agent's inbox record, not a human-notification
  trigger), or dedupe per (mr_id, gate_name) recipient; add the repro as a
  regression test.
- [-] **F2 (minor): silent consumer drop on two of three send paths.**
  message-bus.md requires full-channel drops to be logged. `emit_event`
  logs (warn), but the REST send handler (`api/messages.rs:313`) and MCP
  `message.send` (`mcp.rs:2157`) still use bare `let _ = try_send(...)`
  (pre-existing lines). The shipped notes claim all three send paths log the
  drop — only one does. One-line fixes mirroring the emit_event match.

Test-inflation check: none found — all new tests kill real bugs in the
specced behavior (bridge recipient resolution, single-creation-path, TTL
retention boundaries, env override).

Scope check: no out-of-scope production edits; spec changes are the two
sections the task's coverage_sections name plus the directly entangled HSI p3
amendment. Generated `web/dist/` untouched (verifier-owned).
