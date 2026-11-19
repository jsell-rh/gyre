# Review — task-249: Storage-exempt broadcast kinds

- **Candidate:** `34ef6315410fdea0c31731125f58080f39d7d585` (base `321615eced877c4839ccb98b78fae10778559933`)
- **Reviewer:** independent review stage (model: gateway/rits/zai-org/glm-5-3)
- **Verdict:** approved

## What was reviewed

Full diff base..candidate across `gyre-common/src/message.rs`,
`gyre-server/src/api/{messages,admin,merge_queue}.rs`, `gyre-server/src/lib.rs`,
`gyre-server/src/mcp.rs`, `gyre-server/src/merge_processor.rs`,
`gyre-server/src/reconciliation.rs`, `scripts/dead-message-kind-exemptions.txt`,
`specs/coverage/system/message-bus.md`, `specs/tasks/task-249.md`.

## Requirement 1 — kind-based storage short-circuit

`MessageKind::broadcast_only()` (gyre-common/src/message.rs) returns true for
exactly `DataSeeded` and `QueueUpdated`. All three production send paths add
`&& !msg.kind.broadcast_only()` to their storage guard, so no row of these
kinds is persisted regardless of destination:

- REST `send_message` (api/messages.rs:303-310)
- MCP `handle_message_send` (mcp.rs:3023-3030)
- internal `emit_scoped_event` (lib.rs:750-753)

Repo-wide grep confirms these are the only production callers of
`MessageRepository::store()` (all other `.store(&msg)` hits are inside test
modules). Broadcast-channel delivery is unchanged: both the REST path and
`emit_scoped_event` send to `message_broadcast_tx` after the storage guard
unconditionally. Validation logic (tier/destination/origin checks) is
untouched — the diff only adds a conjunct to the storage conditions, so
existing behavior for all other kinds is unchanged.

The probe test `broadcast_only_kinds_not_persisted_even_forced_to_workspace`
(api/messages.rs) sends both kinds through the real REST handler to a
Workspace destination, asserts 201, then asserts `find_by_id` → None AND
`list_by_workspace` → empty AND delivery on a subscribed broadcast receiver.
Against the pre-task destination-only guard the row WAS stored, so the test
is a genuine kill probe. The internal-emitter probe
(`emit_event_broadcast_only_kind_never_stored_for_workspace_destination`,
lib.rs) covers the same contract on `emit_scoped_event`.

## Requirement 2 — real emitters

- **DataSeeded**: emitted at the end of the real seeding work in
  `admin_seed` (api/admin.rs) via `emit_tenant_event(caller_tenant,
  Destination::Broadcast, DataSeeded, None)` — server origin by construction
  (`emit_scoped_event` sets `MessageOrigin::Server`), `workspace_id: None`,
  payload none, tenant derived from the authenticated caller (not fabricated).
  The idempotent early-return path does not emit (no seeding occurred) —
  correct reading of "when workspace data seeding completes".
  Test `admin_seed_broadcasts_data_seeded` runs the real HTTP flow with a
  broadcast subscriber and asserts the full message shape plus storage
  exemption. Pre-task the kind had zero production constructors, so this
  assertion could not pass — genuine probe.
- **QueueUpdated**: `emit_queue_updated` (merge_processor.rs) resolves the
  tenant from the workspace record via `message_tenant_id` (skip-and-log when
  unresolvable — no fabricated "default" scope, consistent with the
  scope-fabrication invariant) and emits Broadcast/`workspace_id: None`/
  payload none. Called at: enqueue (`enqueue_core` + reconciliation
  auto-enqueue after successful enqueue), claim (Queued→Processing, single
  and atomic-group paths), requeue (paused queue, trust hold, ABAC
  fail-closed, atomic-group rollback), and terminal outcomes (Merged, Failed,
  Cancelled incl. circuit-breaker and `fail_queue_entry`). Covers the
  "enqueue/dequeue/merge-completion transitions" requirement.
  Test `enqueue_and_cancel_broadcast_queue_updated` (api/merge_queue.rs)
  observes both transitions on a subscribed receiver with shape + storage
  assertions.

## Invariant script

`QueueUpdated` removed from `scripts/dead-message-kind-exemptions.txt`;
comment updated; no new entries. `bash scripts/check-dead-message-kinds.sh`
passes (run by reviewer, exit 0).

## Probes run by the reviewer (evidence in /tmp/stage/review-evidence)

| Probe | Result |
|---|---|
| `bash scripts/check-dead-message-kinds.sh` | exit 0, "OK: every MessageKind variant has an emitter" |
| `/tmp/stage/bin/cargo test -p gyre-common --quiet broadcast_only` | 1 passed (`broadcast_only_returns_true_for_dataseeded_queueupdated_only`, incl. negative cases) |
| `/tmp/stage/bin/cargo check -p gyre-server --tests` | exit 0 — type-checks the candidate's production changes and all new server test code |
| `/tmp/stage/bin/cargo fmt -p gyre-server -p gyre-common -- --check` | clean |

Per the review-stage budget constraint, gyre-server test *execution* was
deferred to the independent verifier. The verifier must run:

- `cargo test -p gyre-server --lib broadcast_only_kinds_not_persisted_even_forced_to_workspace`
- `cargo test -p gyre-server --lib emit_event_broadcast_only_kind_never_stored_for_workspace_destination`
- `cargo test -p gyre-server --lib admin_seed_broadcasts_data_seeded`
- `cargo test -p gyre-server --lib enqueue_and_cancel_broadcast_queue_updated`
- `cargo test -p gyre-server --lib merge_processor::tests` (includes the repaired `atomic_group_failure_notifies_all_distinct_authors` and the pass-path tests that now tolerate exactly the QueueUpdated broadcasts)
- `cargo test --all --quiet`

## Test-repair commit (7f02c401)

The atomic-group rollback now broadcasts QueueUpdated before
AtomicGroupFailed; three tests that assumed AtomicGroupFailed was the first
channel message were changed to scan for the kind. Assertion strength is
preserved (the event must still be emitted). The "no events expected on
pass" tests now assert every drained message is QueueUpdated — they still
fail on any recovery event. Verified not to weaken the tested behavior.

## Notes (non-blocking)

- WS delivery of global broadcasts (`workspace_id: None`) goes to all
  authenticated WS clients regardless of tenant (pre-existing filter in
  ws.rs:467-476). The new emitters follow the spec-mandated shape
  (Broadcast, `workspace_id: None`, message carries tenant_id); the filter's
  tenant handling for global broadcasts is a pre-existing platform-wide
  design outside this task's scope.
- Commit attribution is correct: both product-surface commits
  (356ddc34, 7f02c401) are listed in the task-249 `commits:` frontmatter;
  the remaining candidate commits are docs/task-file only.

## Conclusion

Both required behaviors are implemented in production code with genuine
kill-probe tests; the exemptions invariant passes; validation for other
kinds is unchanged. No blocking findings.
