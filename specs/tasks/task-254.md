---
title: "Message edge cases: 64KB send limit with 413 and JWKS historical-key serving for signature rotation"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / message-size-and-key-rotation / 60b177f85a11"
depends_on: [task-247, task-251]
progress: not-started
commits: []
---

## Required behavior

The remaining unimplemented rows of the message-bus.md Edge Cases table (the server-restart row is delivered by the wiring dependency; the send-to-dead/completed-agent row's cleanup lifecycle is delivered by the inbox-lifecycle dependency; the already-implemented rows — send-to-self, 100-agent fan-out, 429 queue depth, atomic concurrent sends, idempotent concurrent ack — must remain passing): (1) Message too large: 'Total POST request body limited to 64KB (configurable via GYRE_MAX_MESSAGE_SIZE). This covers the full JSON envelope including to, kind, and payload. Server rejects with 413.' Today only the global GYRE_MAX_BODY_SIZE (default 10MB) applies, so a message payload can be ~10MB. Apply a message-specific body limit on the send endpoint (POST /api/v1/workspaces/:workspace_id/messages) — e.g. an axum DefaultBodyLimit route layer — reading GYRE_MAX_MESSAGE_SIZE with the specced 64KB default, returning 413 when the request body exceeds it; the limit covers the entire request body, not just the payload field. (2) Signature key rotation: 'Messages signed with old key remain verifiable — JWKS endpoint serves historical keys. New messages use new key.' Today the JWKS document (auth.rs:379) is built from the single current key pair, so after any key change old messages' key_id values no longer resolve. Implement key-history retention: when the signing key changes, the previous public key(s) remain resolvable — persist historical public keys (kid + public key material) in durable storage alongside the current key, and serve them in the JWKS keys array at GET /.well-known/jwks.json; verification that fetches by key_id must still resolve old kids and successfully verify messages signed before the rotation, while new messages are signed with the new key.

## Acceptance Criteria

- [ ] Size limit: POST /api/v1/workspaces/:id/messages with a total request body of 65KB (payload padding) is rejected with 413 and nothing is stored or broadcast; a body at/under the 64KB limit is accepted; GYRE_MAX_MESSAGE_SIZE overrides the default in tests.
- [ ] Key rotation verifiability: sign a Directed/Event message under key K1, rotate the server signing key to K2, then (a) GET /.well-known/jwks.json returns a keys array containing BOTH K1's and K2's kids, and (b) reconstructing the sign input and verifying the K1-signed message's Ed25519 signature against K1's served public key succeeds (real crypto verify, not a structural check).
- [ ] New messages after rotation are signed with K2 and verify against K2's entry.
- [ ] Server-restart row (via the wiring dependency): persisted Directed and Event messages survive a restart; the telemetry buffer is lost (ephemeral, acceptable); WS clients reconnect and re-subscribe with last_seen.
- [ ] Dead/completed-agent row (via the inbox-lifecycle dependency): messages to completed agents are stored and reclaimed by the dead-inbox expiry after the acknowledge_all lifecycle runs.
- [ ] All previously-implemented edge-case rows keep passing: send-to-self allowed, workspace fan-out one store + one broadcast, 429 at GYRE_AGENT_INBOX_MAX with no silent drops, atomic concurrent sends, idempotent concurrent ack (200).

## Verification intent

Size probe: POST a message whose total JSON body is 65KB; today the request passes the global 10MB limit and returns 201 with a ~65KB persisted envelope — the test asserts 413 and no stored row, failing against the current code and passing only when the GYRE_MAX_MESSAGE_SIZE limit (64KB default) guards the send endpoint. Rotation probe: sign a message with the current key, rotate the key, then fetch /.well-known/jwks.json and attempt verification of the old message via its key_id; today the JWKS document contains only the single current key (auth.rs:379-393), so the old kid is absent and verification cannot succeed — the test asserts the old kid is present and the Ed25519 verify() call returns Ok, failing unless historical keys are retained and served.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "e5d4cc99cba7ab16b2b40bb5",
    "decision": "gap",
    "requirement_quote": "| Server restart | Persisted messages survive. Telemetry buffer is lost (acceptable — it's ephemeral). WebSocket connections drop; clients reconnect and re-subscribe with `last_seen`. |",
    "detail": "Implemented rows: send-to-self is allowed (no from==to check exists); workspace fan-out to 100 agents is one store plus one broadcast send; queue depth rejects with 429 at GYRE_AGENT_INBOX_MAX (default 1000) with no silent drops; concurrent sends are atomic (single INSERT in the DB adapters, mutex-guarded insert in mem); concurrent ack is idempotent (200). NOT implemented rows: (1) Server restart — persisted messages do NOT survive in any deployment because AppState.messages is unconditionally the in-memory MemMessageRepository (the SQLite/PG adapters and messages table are unwired); only the telemetry-buffer-lost and WS-reconnect halves of the row hold. (2) Message too large — there is no 64KB GYRE_MAX_MESSAGE_SIZE limit and no message-specific 413; only the global GYRE_MAX_BODY_SIZE (default 10MB) body limit applies, so a message payload can be ~10MB, far beyond the specced 64KB envelope cap. (3) Send to dead/completed agent — the message is stored (no status guard), but the row's cleanup lifecycle is broken: neither 'acknowledge_all was called at completion time' (no production caller on POST /api/v1/agents/:id/complete) nor the orphaned-agent 'agent_orphaned' bulk-ack (absent from stale_agents.rs) ever happens, so expire_dead_inboxes/expire_acked_inboxes can never reclaim those inboxes (counted under Delivery Guarantees ce88ec9). (4) Signature key rotation — the JWKS endpoint serves only the single current key (jwks_json is built from one key pair); there is no key-history storage, so 'messages signed with old key remain verifiable — JWKS endpoint serves historical keys' is unimplemented.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "messages: Arc::new(mem::MemMessageRepository::default()),",
        "detail": "The 'Server restart | Persisted messages survive' row is not met: the production message store is a process-local HashMap in every deployment (DB adapters unwired), so a restart loses all Directed and Event messages."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "// Body size limit: configurable via GYRE_MAX_BODY_SIZE (bytes), default 10MB.\n    let max_body_bytes: usize = std::env::var(\"GYRE_MAX_BODY_SIZE\")",
        "detail": "The 'Message too large | 64KB GYRE_MAX_MESSAGE_SIZE, reject 413' row is not met: GYRE_MAX_MESSAGE_SIZE does not exist anywhere in the codebase (inspected via repo-wide grep); only this global 10MB router body limit applies, with no per-message 64KB cap."
      },
      {
        "file": "crates/gyre-server/src/message_dispatcher.rs",
        "quote": ".acknowledge_all(&Id::new(\"agent-2\".to_string()), \"agent_completed\")",
        "detail": "The dead-agent inbox lifecycle rows are not met in production: this test fixture is the only acknowledge_all call site in the server crate — the completion handler and the stale-agent detector never bulk-ack, so neither 'agent_completed' nor 'agent_orphaned' is ever recorded and the 7-day dead-inbox cleanup never fires."
      },
      {
        "file": "crates/gyre-server/src/oidc.rs",
        "quote": "let jwks: serde_json::Value = serde_json::from_str(&state.agent_signing_key.jwks_json)",
        "detail": "The 'Signature key rotation' row is not met: the JWKS endpoint serves only the single current key's JWK Set (jwks_json is built from one key pair); there is no historical-key storage, so old-key signatures stop being verifiable after rotation."
      },
      {
        "file": "crates/gyre-server/src/api/messages.rs",
        "quote": "return Err(ApiError::TooManyRequests(format!(\n                    \"agent {} inbox is full ({} unacked messages)\",",
        "detail": "The queue-depth row IS met: sends to a full inbox are rejected with 429 before persisting — no silent drops — at GYRE_AGENT_INBOX_MAX (default 1000)."
      }
    ]
  }
]
