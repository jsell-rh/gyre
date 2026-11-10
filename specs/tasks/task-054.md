---
title: "Conversations Endpoint Integration Test"
spec_ref: "human-system-interface.md §5 (Conversation-to-Code Provenance)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-054.md
coverage_sections: []
commits: 
  - 6e752e6d
---

## Spec Excerpt

From `human-system-interface.md` §5:

> Each agent's conversation with LLM hashed, stored as provenance artifact.

The spec defines `GET /api/v1/conversations/:sha` for retrieving conversation blobs.

## Current State

- Handler: `crates/gyre-server/src/api/conversations.rs` — complete (67 lines)
- Module declaration: `pub mod conversations;` in `api/mod.rs` — present
- Route registration: registered in `crates/gyre-server/src/lib.rs` (line 661) — **DONE**
- `ConversationRepository` port: wired into `AppState`
- Upload path (`conversation.upload` MCP tool): working
- Database tables (`conversations`, `turn_commit_links`): migrated
- Integration test: **MISSING**

The endpoint is fully functional (handler + route registration). The only gap is an integration test verifying the round-trip behavior.

## Implementation Plan

1. **Add integration test** verifying round-trip:
   - Store a conversation blob via the `ConversationRepository` port
   - `GET /api/v1/conversations/:sha` returns 200 with the decompressed blob
   - `GET /api/v1/conversations/nonexistent` returns 404
   - ABAC: unauthenticated request returns 401/403

2. **Verify** `cargo test --all` passes.

## Acceptance Criteria

- [x] `GET /api/v1/conversations/:sha` returns the decompressed conversation blob (route registered in lib.rs:661)
- [x] Integration test covers success path (200 with blob)
- [x] Integration test covers not-found path (404)
- [x] Integration test covers ABAC enforcement
- [x] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/conversations.rs` — the handler is complete
3. Confirm the route is registered in `crates/gyre-server/src/lib.rs` (line ~661)
4. Write an integration test in `crates/gyre-server/tests/` or extend an existing test file
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `6e752e6d` test(conversations): add integration tests for GET /api/v1/conversations/:sha (TASK-054)

## Shipped

`GET /api/v1/conversations/:sha` (HSI §5, conversation-to-code provenance) is covered by
`crates/gyre-server/tests/conversation_integration.rs` — three integration tests exercising the
real route (registered in `crates/gyre-server/src/lib.rs`) over a live TCP listener, with blobs
seeded through the `ConversationRepository` port:

- **Success path** — stores a zstd-compressed blob via `ConversationRepository::store`, then GETs
  the endpoint with global-token auth and asserts 200, `Content-Type: application/octet-stream`,
  the `X-Gyre-Conversation-Sha` echo header, and body equality with the original uncompressed
  JSON (the round-trip decompresses on `get`).
- **Not-found path** — requests a nonexistent SHA, asserts 404.
- **Auth enforcement** — seeds a conversation first (so 401 cannot mask 404), requests without
  `Authorization`, asserts 401 from the `AuthenticatedAgent` extractor at the system boundary.

No production code was changed by this assignment — handler, route registration, and port wiring
were already complete at the assignment base. Within this repository's lineage the suite was
delivered by `d7940e857` and adapted to the fallible `seed_builtin_policies` API by checkpoint
`ed36c1b68`; the contract's `6e752e6d` does not resolve as an object in this repository's
history.

### Test evidence

Focused probe re-run on this branch head (includes the assignment base merge `1901941b`):

- `cargo test -p gyre-server --test conversation_integration` → **3 passed; 0 failed**
  (log retained under `/tmp/stage/review-evidence/task-054-conversation-integration.log`).
- Prior rounds mutation-tested the suite: forcing the handler's metadata-miss branch to 200 made
  the 404 test fail, and appending a byte to the response body made the blob test fail — both
  assertions bite.
