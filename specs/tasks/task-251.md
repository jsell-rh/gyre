---
title: "Directed inbox lifecycle: acknowledge_all on agent completion and on stale-agent Dead transition"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / directed-inbox-lifecycle / 60b177f85a11"
depends_on: [task-247]
progress: not-started
commits: []
---

## Required behavior

Per message-bus.md Delivery Guarantees: Directed tier is 'At-least-once (persisted until ack, redelivered on poll)' with persistence 'DB — no TTL, expires only on ack or agent completion', and 'Agent completion cleanup: When an agent completes (POST /api/v1/agents/:id/complete), all unacked Directed messages in its inbox are marked with acknowledged: true and ack_reason: "agent_completed". They remain in the DB for audit but are no longer redelivered.' The DB-persistence half of this guarantee is delivered by the wiring task (dependency); this task delivers the cleanup lifecycle that is entirely missing — acknowledge_all has no production caller (its only server-crate call site is a test fixture in message_dispatcher.rs): (1) Agent completion: the REST complete handler (crates/gyre-server/src/api/spawn.rs complete_agent, ~line 2632) and the MCP agent-complete path must call state.messages.acknowledge_all(&agent_id, "agent_completed") when the agent completes, after which the unacked count is zero, poll with ?acknowledged=false returns nothing, and the messages remain stored (audit) with ack_reason recorded. (2) Orphaned agents (spec Edge Cases: 'For orphaned agents (killed without complete), the stale agent detector marks them Dead and calls acknowledge_all with ack_reason: "agent_orphaned", enabling the same cleanup'): stale_agents.rs currently transitions agents to Dead but never bulk-acks — add the acknowledge_all(&agent_id, "agent_orphaned") call when the detector marks an agent Dead, so expire_acked_inboxes (dead-inbox retention GYRE_DEAD_INBOX_TTL_SECS) can actually reclaim dead agents' inboxes instead of unacked Directed messages accumulating indefinitely. Existing expiry semantics are unchanged: expire_acked_inboxes only deletes agent-targeted messages acked with agent_completed/agent_orphaned after the retention window.

## Acceptance Criteria

- [ ] Completion cleanup: after POST /api/v1/agents/:id/complete, count_unacked for that agent is 0, GET /api/v1/agents/:id/messages?acknowledged=false returns no messages, and the previously unacked messages are still retrievable (audit retention) with ack_reason 'agent_completed'.
- [ ] MCP parity: the MCP agent-complete tool path performs the same bulk-ack.
- [ ] Orphan cleanup: when the stale detector marks a heartbeated-out agent Dead, its unacked Directed messages are bulk-acked with ack_reason 'agent_orphaned' (count_unacked 0, rows retained).
- [ ] Reclamation end-to-end: after either cleanup path, running the message-expiry job with the dead-inbox TTL elapsed deletes those acked inbox rows via expire_acked_inboxes (returns the deleted count), while unacked inboxes of live agents and explicit-ack messages inside the retention window are untouched.
- [ ] At-least-once across the full lifecycle (with the wiring dependency): an unacked Directed message to a live agent survives a server restart and is redelivered on poll; the same message is never redelivered after its agent completes or is marked Dead.

## Verification intent

Completion probe: send an unacked Directed message to an agent, call POST /api/v1/agents/:id/complete, then run the message-expiry job with the dead-inbox TTL elapsed; today acknowledge_all is never called by any production path (spawn.rs complete_agent and the MCP complete tool never invoke it, and stale_agents.rs transitions to Dead without bulk-acking), so count_unacked stays > 0 and expire_acked_inboxes deletes zero rows for that inbox — the test asserts both the zero unacked count after completion/orphaning and the eventual reclamation, and therefore fails against the current code, passing only when the completion and stale-detector acknowledge_all calls exist on the production paths.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "ce88ec9cd0bc4a86136ae33b",
    "decision": "gap",
    "requirement_quote": "**Directed** | At-least-once (persisted until ack, redelivered on poll) | DB — no TTL, expires only on ack or agent completion",
    "detail": "Partially implemented. Implemented: the Event TTL expiry job (GYRE_EVENT_TTL_SECS default 7 days, hourly background job calling expire_events, plus the dead-inbox retention GYRE_DEAD_INBOX_TTL_SECS), the telemetry ring buffer (in-memory, per-workspace 10,000 default via GYRE_TELEMETRY_BUFFER_SIZE, largest-first workspace eviction at 100 workspaces via GYRE_TELEMETRY_MAX_WORKSPACES), the Directed queue depth limit (GYRE_AGENT_INBOX_MAX default 1000, sends rejected with 429 before persisting, no silent drops), and poll-based redelivery. NOT implemented: (1) the Directed persistence medium — 'DB — no TTL, expires only on ack' is not on the wired path; production state.messages is the in-memory MemMessageRepository in every deployment, so restart orphans every unacked Directed message and the at-least-once guarantee fails exactly when it matters (after a crash); (2) agent completion cleanup — 'When an agent completes (POST /api/v1/agents/:id/complete), all unacked Directed messages in its inbox are marked acknowledged with ack_reason agent_completed' is never performed: acknowledge_all has no production caller (the only call site in the server crate is a test fixture; neither the REST complete handler in api/spawn.rs nor the MCP agent-complete path calls it); (3) consequently the orphaned-agent path ('the stale agent detector marks them Dead and calls acknowledge_all with ack_reason agent_orphaned') is also unimplemented — stale_agents.rs transitions agents to Dead but never bulk-acks, so expire_acked_inboxes can never match a dead inbox in production and unacked Directed messages to dead agents accumulate indefinitely.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
        "detail": "The Directed/Event persistence medium in production is the in-memory adapter (unconditionally wired), so 'DB — no TTL, expires only on ack' does not describe the deployed system: a restart drops every unacked Directed message."
      },
      {
        "file": "crates/gyre-server/src/message_dispatcher.rs",
        "quote": "let events_deleted = state\n        .messages\n        .expire_events(now.saturating_sub(event_ttl_ms))\n        .await?;",
        "detail": "The Event-tier TTL expiry job IS implemented (GYRE_EVENT_TTL_SECS default 604_800s, run_message_expiry spawned hourly) — though it too runs against the in-memory store in production."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "agent_inbox_max: std::env::var(\"GYRE_AGENT_INBOX_MAX\")\n            .ok()\n            .and_then(|v| v.parse().ok())\n            .unwrap_or(1000),",
        "detail": "The Directed queue depth limit is configurable with the specced default of 1000; the send path rejects with 429 (TooManyRequests) before persisting, preserving the no-silent-drop requirement."
      },
      {
        "file": "crates/gyre-server/src/message_dispatcher.rs",
        "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
        "detail": "The ONLY acknowledge_all call site in the server crate is this test fixture in message_dispatcher.rs's tests; the production completion paths (POST /api/v1/agents/:id/complete in api/spawn.rs, MCP agent-complete in mcp.rs) and stale_agents.rs were inspected and never call acknowledge_all, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded in production and the dead-inbox cleanup never fires."
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "quote": "let _ = agent.transition_status(AgentStatus::Dead);",
        "detail": "The stale-agent detector marks orphaned agents Dead but performs no acknowledge_all with ack_reason 'agent_orphaned' — the spec's orphan cleanup path is absent."
      }
    ]
  }
]
