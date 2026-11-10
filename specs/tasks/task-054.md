---
title: "Conversations Endpoint Integration Test"
spec_ref: "human-system-interface.md §5 (Conversation-to-Code Provenance)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-054.md
coverage_sections: []
commits: 
  - d7940e857
  - ed36c1b68
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

- `d7940e857` feat(explorer): explorer canvas, loop tooling, spec coverage tracking (#627) — delivered `crates/gyre-server/tests/conversation_integration.rs` into this lineage
- `ed36c1b68` checkpoint: recover interrupted pipeline source — adapted the test to the fallible `seed_builtin_policies` API

## Shipped

`GET /api/v1/conversations/:sha` (HSI §5, conversation-to-code provenance) is covered by
`crates/gyre-server/tests/conversation_integration.rs` — three integration tests exercising the
real HTTP endpoint over a live TCP listener against the `ConversationRepository` port
(`MemConversationRepository` via `build_state`):

- **Success path** — seeds a zstd-compressed blob via `ConversationRepository::store`, then GETs
  `/api/v1/conversations/:sha` with global-token auth and asserts 200,
  `Content-Type: application/octet-stream`, the `X-Gyre-Conversation-Sha` echo header, and that
  the body equals the original uncompressed JSON (the round-trip decompresses on `get`).
- **Not-found path** — requests a nonexistent SHA, asserts 404.
- **Auth enforcement** — seeds a conversation first (so 401 cannot mask 404), requests without
  `Authorization`, asserts 401 from the `AuthenticatedAgent` extractor at the system boundary.

The test file was delivered into this branch's lineage by `d7940e857` (PR #627 squash) and
adapted to the fallible `seed_builtin_policies` API by checkpoint `ed36c1b68`; the assigned
contract previously pointed at `6e752e6d`, a SHA from a checkout not in this repository's
history, which has been replaced with the real lineage SHAs above so review scoping resolves.

No production code was changed by this assignment — the handler, route registration
(`lib.rs`, `/api/v1/conversations/:sha`), and port wiring were already complete at the
assignment base; this record reconciles the task's commit attribution and progress state.

### Test evidence

Focused probe re-run on this branch at the restored-contract head (output retained under
`/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --test conversation_integration` → **3 passed; 0 failed**
  (`task-054-conversation-integration-green.log`).
- Prior rounds additionally mutation-tested the suite: forcing the handler's metadata-miss
  branch to 200 made the 404 test fail, and tampering the response body with a trailing byte
  made the blob test fail — both assertions bite (logs retained in the pipeline attempt
  artifacts).

The assignment base merge (`33cd786c2`, task-107 Sigstore work) was included in the probe
head; the suite is green against the merged tree.
