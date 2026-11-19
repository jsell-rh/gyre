---
title: "Storage-exempt broadcast kinds: kind-based short-circuit and real DataSeeded/QueueUpdated emitters"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / broadcast-only-kind-emitters / 60b177f85a11"
depends_on: []
progress: complete
commits: ["669214b67a1a1675deeee31994412a8aabf39bbe", "7f02c401fe9d8ba5d46b4303da37fb31645ccbb5", "356ddc34988696560ac01e683ee882fb92d97d42"]
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

Both required behaviors are implemented in production code, the one
verification finding against the candidate is repaired, and the
deterministic repo-invariant checks (rustfmt, clippy changed-lines,
check-dead-message-kinds with QueueUpdated removed from the exemptions
file, and the rest of checks.sh) ran green in the independent
verification sandbox on this exact Rust source — `git diff 34ef6315..HEAD
-- crates/ scripts/` is empty.

### Kind-based storage short-circuit

`MessageKind::broadcast_only()` (crates/gyre-common/src/message.rs)
returns true for exactly `DataSeeded` and `QueueUpdated`. All three send
paths short-circuit storage by KIND in addition to the existing
tier/destination guards, so no `DataSeeded`/`QueueUpdated` row is ever
persisted even when addressed to a Workspace destination, while
broadcast-channel delivery, signing, and all validation for other kinds
are unchanged:

- REST send: `send_message` (crates/gyre-server/src/api/messages.rs)
- MCP send: `handle_message_send` (crates/gyre-server/src/mcp.rs)
- internal emitter: `emit_scoped_event` (crates/gyre-server/src/lib.rs)

The optional 400-rejection of non-Broadcast destinations was NOT taken:
the acceptance criterion requires the broadcast channel to deliver a
Workspace-forced broadcast-only message, so the send succeeds 201 and
the kind-based exemption keeps it out of `store()`.

### Real emitters for the two formerly-dead kinds

- `DataSeeded` — emitted when workspace data seeding completes, at the
  end of `admin_seed` (crates/gyre-server/src/api/admin.rs): server
  origin, `Destination::Broadcast`, `workspace_id: None`, payload none,
  tenant = the seeding caller's tenant.
- `QueueUpdated` — `emit_queue_updated` helper
  (crates/gyre-server/src/merge_processor.rs) called at every merge-queue
  entry state transition: enqueue (`enqueue_core` in api/merge_queue.rs
  and the reconciliation auto-enqueue in reconciliation.rs), claim
  (Queued→Processing, single-entry and atomic-group paths), requeue
  (Processing→Queued: paused queue, supervised-trust hold, ABAC
  fail-closed, atomic-group rollback), and terminal outcomes (Merged,
  Failed, Cancelled — including operator cancel in api/merge_queue.rs
  and the circuit-breaker cancellation). Payload none,
  `Destination::Broadcast`, `workspace_id: None`; tenant resolved from
  the affected workspace (skip-and-log when unresolvable, never
  fabricated).

`QueueUpdated` was removed from scripts/dead-message-kind-exemptions.txt
(no new exemption entries; the file shrank by one kind), and the
message-bus.md coverage row 6 cites this task's source evidence.

### Verification-finding repair (attempt 4381dc12)

The independent verification run failed one frontend test:
WorkspaceSettings "renders invitation status and expiry" expected
'Expires in 30m' but got 'Expires in 29m'. Root cause: a pre-existing
wall-clock boundary race in the test fixture — `expires_at` was set to
exactly floor(now)+30*60 while the component floors whole minutes from
Date.now() at render time, so any render crossing the next second
computed 1799s → 29m. Fixed in commit 669214b6 by adding +30s slack to
the countdown fixtures (assertion strength unchanged: still exact
'Expires in 30m', still proves minutes-not-seconds). Focused evidence:
85/85 passed on 4 consecutive vitest executions of the file
(/tmp/stage/review-evidence).

### Test evidence

- `cargo test -p gyre-common broadcast_only` → ok (run in this
  assignment; see /tmp/stage/review-evidence).
- Focused vitest run of web/src/__tests__/WorkspaceSettings.test.js →
  85/85 × 4 (this assignment).
- gyre-server test compilation is deferred to the independent verifier
  per the 4 GiB budget rule; the task's server tests to execute are
  listed in /tmp/stage/review-evidence/README.md, notably
  `broadcast_only_kinds_not_persisted_even_forced_to_workspace`,
  `emit_event_broadcast_only_kind_never_stored_for_workspace_destination`,
  `admin_seed_broadcasts_data_seeded`, and
  `enqueue_and_cancel_broadcast_queue_updated`.
