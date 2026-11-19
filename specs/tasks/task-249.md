---
title: "Storage-exempt broadcast kinds: kind-based short-circuit and real DataSeeded/QueueUpdated emitters"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / broadcast-only-kind-emitters / 60b177f85a11"
depends_on: []
progress: ready-for-review
commits: ["356ddc34988696560ac01e683ee882fb92d97d42", "7f02c401fe9d8ba5d46b4303da37fb31645ccbb5"]
---

## Required behavior

Per message-bus.md Message Kinds / Broadcast-only kinds: 'DataSeeded and QueueUpdated ... are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting MessageRepository::store(). The send path must short-circuit before attempting storage for these kinds.' Two requirements: (1) Kind-based storage short-circuit: the REST send handler (api/messages.rs), the MCP send path, and the internal emitter path currently short-circuit storage on Destination::Broadcast (and Telemetry tier) only, so a server-originated DataSeeded or QueueUpdated addressed to a Workspace destination would be persisted. Add a kind-based short-circuit so these kinds never reach store() regardless of destination — broadcast-channel delivery only (optionally reject a non-Broadcast destination for these broadcast-only kinds with 400, but the mandatory behavior is that no DataSeeded/QueueUpdated row is ever stored). (2) Dead spec delivery links: DataSeeded and QueueUpdated have no production emitter (repo-wide they appear only in the enum, tests, and one consumer match arm; scripts/dead-message-kind-exemptions.txt:20 records QueueUpdated as spec'd but never emitted). Add real server-originated emitters at the actual production events: DataSeeded (payload: none, Destination::Broadcast, workspace_id: None) when workspace data seeding completes (the admin workspace-seed flow in crates/gyre-server/src/api/admin.rs), and QueueUpdated (payload: none, Destination::Broadcast) when merge-queue state changes (enqueue/dequeue/merge-completion transitions in the merge-queue/merge-processor paths). Remove QueueUpdated from scripts/dead-message-kind-exemptions.txt once it has a production emitter (the check-dead-message-kinds.sh invariant requires every MessageKind to have an emitter; add no new exemption entries).

## Acceptance Criteria

- [ ] Storage exemption: a test sends a DataSeeded (and QueueUpdated) message through the send path — including forced to a Workspace destination — and asserts via list_by_workspace/find_by_id that no row was stored, while the broadcast channel delivered the message to subscribers.
- [ ] DataSeeded emitter: running the workspace data-seed flow results in a DataSeeded broadcast message observable by a connected/subscribed client (real event, server origin, workspace_id: None).
- [ ] QueueUpdated emitter: a merge-queue state transition (e.g. an MR entering or leaving the queue) results in a QueueUpdated broadcast message observable the same way.
- [ ] scripts/check-dead-message-kinds.sh passes with QueueUpdated removed from the exemptions file (no new exemptions added).
- [ ] Existing broadcast/destination validation behavior is unchanged for all other kinds.

## Verification intent

Persistence probe for the short-circuit: emit a DataSeeded message addressed to a workspace and query state.messages.list_by_workspace for that workspace; with the current destination-only guard the kind is persisted (a row exists), violating 'never hitting MessageRepository::store()' — the test must fail unless the kind-based short-circuit is in place. Emitter probe: run the seed flow with a broadcast-channel subscriber attached; today no DataSeeded message ever appears (the kind has zero production constructors), so an assertion that a DataSeeded broadcast arrives fails against the current code and only passes once the real emitter exists.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "a8f73059653f7d09adbd36b2",
    "decision": "gap",
    "requirement_quote": "They are server-only AND storage-exempt — they flow through the in-memory broadcast channel only, never hitting `MessageRepository::store()`. The send path must short-circuit before attempting storage for these kinds.",
    "detail": "The enum, tiers, and constraints are implemented: every specced variant exists (plus seven later server-only Event additions such as MrReverted and TaskBlocked), tier() classifies Custom as Event with Directed opt-in via the request 'tier' field, server_only() returns true for all built-in Event kinds plus the GateResult Directed exception exactly as specced, custom Deserialize maps unknown strings to Custom(s), and the send handler rejects Directed+Workspace/Broadcast and Telemetry+Agent/Broadcast with 400 and server_only kinds from non-Server origins with 403. Two specific requirements are unmet: (1) the kind-based storage short-circuit for DataSeeded/QueueUpdated is NOT implemented — the send path and internal emitter short-circuit on Destination::Broadcast only, so a server-originated DataSeeded or QueueUpdated sent to a Workspace destination would be persisted, contradicting 'never hitting MessageRepository::store()' for these kinds; (2) DataSeeded and QueueUpdated have no production emitter at all (repo-wide grep finds them only in enum definition/tests and one consumer match arm), so the broadcast-only delivery path this section specifies for them is dead — the exemptions file itself records QueueUpdated as 'Spec'd (message-bus QueueUpdated) but never emitted', a dead spec delivery link owned by this specification.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/messages.rs",
        "quote": "if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {",
        "detail": "The REST send path's storage guard is tier- and destination-based only; there is no kind-based short-circuit for DataSeeded/QueueUpdated, so those kinds reach store() whenever their destination is not Broadcast. The internal emitter (lib.rs emit_scoped_event) mirrors this with the same destination-only guard."
      },
      {
        "file": "scripts/dead-message-kind-exemptions.txt",
        "quote": "# Spec'd (platform-model hot-file warnings; message-bus QueueUpdated)\n# but never emitted — pre-existing gaps, tracked here so the check is\n# green for NEW kinds.",
        "detail": "The broadcast-only kind QueueUpdated (and likewise DataSeeded, which has no emitter and no exemption entry) is never constructed in production code — the spec's broadcast-only kinds never flow through the in-memory broadcast channel because nothing emits them."
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "| MessageKind::GateResult => MessageTier::Directed,",
        "detail": "Tier classification matches the spec including the GateResult Directed exception; BudgetWarning/BudgetExhausted/AgentError sit in the Event arm and the telemetry kinds in the Telemetry arm, per the 'Changes from prior draft' list."
      },
      {
        "file": "crates/gyre-server/src/api/messages.rs",
        "quote": "\"Directed tier requires Agent destination, not Workspace\".to_string(),",
        "detail": "Tier+destination constraints are enforced with 400 in the send handler (and mirrored on the MCP path): Directed+Workspace, Directed+Broadcast, Telemetry+Agent, Telemetry+Broadcast are all rejected."
      },
      {
        "file": "crates/gyre-server/src/api/messages.rs",
        "quote": "if kind.server_only() && !matches!(from, MessageOrigin::Server) {",
        "detail": "The server_only() 403 check is enforced on the send path, covering the GateResult forge-authority exception and all built-in Event kinds, with Custom exempt as specced."
      }
    ]
  }
]

## Shipped

Both requirements are implemented in production code, and the one
verification finding against the candidate is repaired.

### Kind-based storage short-circuit

`MessageKind::broadcast_only()` (crates/gyre-common/src/message.rs) returns
true for exactly `DataSeeded` and `QueueUpdated`. All three send paths
short-circuit storage by KIND in addition to the existing tier/destination
guards, so no `DataSeeded`/`QueueUpdated` row is ever persisted even when
addressed to a Workspace destination, while broadcast-channel delivery,
signing, and all validation for other kinds are unchanged:

- REST send: `send_message` (crates/gyre-server/src/api/messages.rs)
- MCP send: `handle_message_send` (crates/gyre-server/src/mcp.rs)
- internal emitter: `emit_scoped_event` (crates/gyre-server/src/lib.rs)

The optional 400-rejection of non-Broadcast destinations was NOT taken: the
acceptance criterion requires the broadcast channel to deliver a
Workspace-forced broadcast-only message, so the send succeeds 201 and the
kind-based exemption keeps it out of `store()`.

### Real emitters for the two formerly-dead kinds

- `DataSeeded` — emitted when workspace data seeding completes, at the end
  of `admin_seed` (crates/gyre-server/src/api/admin.rs): server origin,
  `Destination::Broadcast`, `workspace_id: None`, payload none, tenant =
  the seeding caller's tenant.
- `QueueUpdated` — `emit_queue_updated` helper
  (crates/gyre-server/src/merge_processor.rs) called at every merge-queue
  entry state transition: enqueue (`enqueue_core` in api/merge_queue.rs and
  the reconciliation auto-enqueue in reconciliation.rs), claim
  (Queued→Processing, single-entry and atomic-group paths), requeue
  (Processing→Queued: paused queue, supervised-trust hold, ABAC fail-closed,
  atomic-group rollback), and terminal outcomes (Merged, Failed, Cancelled).
  Payload none, `Destination::Broadcast`, `workspace_id: None`; tenant
  resolved from the affected workspace (skip-and-log when unresolvable).

`QueueUpdated` was removed from scripts/dead-message-kind-exemptions.txt
(no new entries); `bash scripts/check-dead-message-kinds.sh` passes.

### Tests

- `broadcast_only_kinds_not_persisted_even_forced_to_workspace`
  (api/messages.rs): sends `data_seeded` and `queue_updated` through the
  REST send path to a Workspace destination — 201, `find_by_id` → None,
  `list_by_workspace` empty, broadcast channel delivered both. Fails
  against the pre-task destination-only guard (the row WAS stored).
- `emit_event_broadcast_only_kind_never_stored_for_workspace_destination`
  (lib.rs): same probe through the internal emitter for both kinds.
- `admin_seed_broadcasts_data_seeded` (api/admin.rs): runs the real
  workspace-seed flow with a broadcast subscriber; asserts a Server-origin
  DataSeeded broadcast (Broadcast destination, `workspace_id: None`,
  payload none) and no stored row. Pre-task the kind had zero production
  constructors.
- Merge-queue state-transition tests in merge_processor.rs observe
  QueueUpdated broadcasts on claim/merge/requeue/pause/fail transitions.

### Verification finding repaired (commit 7f02c401)

`cargo test --all` on the candidate failed
`merge_processor::tests::atomic_group_failure_notifies_all_distinct_authors`:
the atomic-group rollback requeue now broadcasts `QueueUpdated` (specced
task-249 behavior) before the `AtomicGroupFailed` event, and that one test
still assumed `AtomicGroupFailed` was the first channel message. Fixed by
applying the same scan-for-kind pattern its sibling rollback test already
uses; the event must still be emitted, so assertion strength is preserved.

### Evidence

- `cargo check -p gyre-server` → exit 0 (this sandbox; gyre-server test
  compilation deferred to the independent verifier per the 4 GiB budget)
- `cargo test -p gyre-common --quiet` → 129 passed, including
  `broadcast_only_returns_true_for_dataseeded_queueupdated_only`
- `bash scripts/check-dead-message-kinds.sh` → OK
- `cargo fmt -p gyre-server -- --check` → clean

Verifier must run: `cargo test -p gyre-server --lib merge_processor::tests`
(includes the repaired `atomic_group_failure_notifies_all_distinct_authors`),
`broadcast_only_kinds_not_persisted_even_forced_to_workspace`,
`emit_event_broadcast_only_kind_never_stored_for_workspace_destination`,
`admin_seed_broadcasts_data_seeded`, and the full `cargo test --all --quiet`
(the finding's command). Details in /tmp/stage/review-evidence/task-249-repair.md.
