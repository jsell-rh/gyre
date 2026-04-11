# TASK-054: Conversations Endpoint Integration Test

**Spec reference:** `human-system-interface.md` §5 (Conversation-to-Code Provenance)  
**Depends on:** None (handler and route already implemented)  
**Progress:** `ready-for-review`

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
