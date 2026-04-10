# TASK-054: Register Conversations Endpoint Route

**Spec reference:** `human-system-interface.md` §5 (Conversation-to-Code Provenance)  
**Depends on:** None (handler already implemented)  
**Progress:** `not-started`

## Spec Excerpt

From `human-system-interface.md` §5:

> Each agent's conversation with LLM hashed, stored as provenance artifact.

The spec defines `GET /api/v1/conversations/:sha` for retrieving conversation blobs. The handler is fully implemented in `crates/gyre-server/src/api/conversations.rs`, including per-handler ABAC (resolves `workspace_id` from conversation metadata before evaluating access). The module is declared in `api/mod.rs` (`pub mod conversations;`), but the route is **not registered** in the `api_router()` function.

## Current State

- Handler: `crates/gyre-server/src/api/conversations.rs` — complete (67 lines)
- Module declaration: `pub mod conversations;` in `api/mod.rs` — present
- Route registration in `api_router()` — **MISSING**
- `ConversationRepository` port: wired into `AppState`
- Upload path (`conversation.upload` MCP tool): working
- Database tables (`conversations`, `turn_commit_links`): migrated

The upload side works (agents can store conversations via MCP), but the retrieval side (humans/UI reading conversations via REST) is unreachable because the route is not registered.

## Implementation Plan

1. **Add route registration** in `crates/gyre-server/src/api/mod.rs`:
   ```rust
   .route("/api/v1/conversations/:sha", get(conversations::get_conversation))
   ```
   Add the import at the top of `api_router()` or use the fully qualified path.

2. **Add integration test** verifying round-trip:
   - Store a conversation blob via the `ConversationRepository` port
   - `GET /api/v1/conversations/:sha` returns 200 with the blob
   - `GET /api/v1/conversations/nonexistent` returns 404

3. **Verify** `cargo test --all` passes.

## Acceptance Criteria

- [ ] `GET /api/v1/conversations/:sha` returns the decompressed conversation blob
- [ ] 404 returned for unknown SHA
- [ ] ABAC enforced (workspace read access required)
- [ ] Integration test covers success and not-found paths
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/conversations.rs` — the handler is complete
3. Read `crates/gyre-server/src/api/mod.rs` — add the route registration
4. Add the `conversations::get_conversation` import
5. Write an integration test in `crates/gyre-server/tests/` or extend an existing test file
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

(none yet)
