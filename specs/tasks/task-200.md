---
title: "Message bus — per-kind payload schema validation (reject invalid payloads with 400)"
spec_ref: "message-bus.md §Payload Schemas"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Payload Schemas"
commits: ["eecd06dc7285c14d87c70a6baf3bea15ee14a1ea", "e71d72fd103f5c0bfa657f5a4133a85e0ff89bd0", "c0cbeac214b48bb791bc3a64b0ede2d81a044083", "41846588861b3ba22bc04077ce768bd98cf8cf8c", "9764477d2fcb5226bc866f6ff8c143c39bd715ca"]
---

## Spec Excerpt

From `specs/system/message-bus.md` §Payload Schemas:

> Each `MessageKind` has a defined payload schema. The server validates payloads on receipt — invalid payloads are rejected with 400.

The spec then defines a table of payload fields and which are **required** per kind. Key rows (see spec §Payload Schemas, lines 194–229, for the complete table):

| Kind | Required fields |
|---|---|
| `TaskAssignment` | `task_id` |
| `ReviewRequest` | `mr_id` |
| `StatusUpdate` | `status`, `summary` |
| `Escalation` | `reason` |
| `AgentCreated` | `agent_id` |
| `AgentStatusChanged` | `agent_id`, `status` |
| `AgentContainerSpawned` | `agent_id`, `container_id`, `image`, `runtime` |
| `TaskCreated` | `task_id` |
| `TaskTransitioned` | `task_id`, `status` |
| `MrCreated` | `mr_id` |
| `MrStatusChanged` | `mr_id`, `status` |
| `MrMerged` | `mr_id` |
| `PushRejected` | `repo_id`, `branch`, `agent_id`, `reason` |
| `PushAccepted` | `repo_id`, `branch`, `agent_id` |
| `SpecChanged` | `repo_id`, `spec_path`, `change_kind` |
| `AgentCompleted` | `agent_id`, `task_id` |
| `ReconciliationCompleted` | `workspace_id`, `persona_id` |
| `GateFailure` | `mr_id`, `gate_name` |
| `StaleSpecWarning` | `mr_id`, `repo_id`, `spec_path`, `spec_sha`, `current_sha` |
| `SpeculativeConflict` | `repo_id`, `branch`, `conflicting_files` |
| `SpeculativeMergeClean` | `repo_id`, `branch` |
| `HotFilesChanged` | `repo_id` |
| `BudgetWarning` | `agent_id`, `workspace_id`, `usage_pct` |
| `BudgetExhausted` | `agent_id`, `workspace_id`, `grace_secs` |
| `AgentError` | `agent_id`, `error` |
| `ToolCallStart` | `agent_id`, `tool_name` |
| `ToolCallEnd` | `agent_id`, `tool_name`, `duration_ms` |
| `RunStarted` | `agent_id` |
| `TextMessageContent` | `agent_id`, `content` |
| `StateChanged` | `agent_id`, `new_state` |
| `RunFinished` | `agent_id` |
| `QueueUpdated` | (none) |
| `DataSeeded` | (none) |
| `Custom(name)` | Any valid JSON object; no required fields |

## Current Gap (why this is hollow)

`gyre-common/src/message.rs` defines `MessageKind` with `tier()`, `server_only()`, `as_str()`, and serde — but **no payload validation** exists anywhere in the workspace. Both send paths accept and store `req.payload` unchecked:

- `gyre-server/src/api/messages.rs::send_message` (builds `Message` at ~line 265 with `payload: req.payload` — no validation).
- `gyre-server/src/mcp.rs::handle_message_send` (~line 1633, `payload: args.get("payload").cloned()` — no validation).

There is no `validate_payload` / schema module. Missing-required-field enforcement is absent, so a `TaskAssignment` with no `task_id`, or a `StatusUpdate` missing `summary`, is accepted with 201.

## Implementation Plan

1. **Add `MessageKind::validate_payload` to `gyre-common/src/message.rs`.**
   - Signature: `pub fn validate_payload(&self, payload: Option<&serde_json::Value>) -> Result<(), String>`.
   - Behavior:
     - Compute the required-field list for `self` from the spec table above. Encode it as a `&'static [&'static str]` per built-in kind via a `match`.
     - Kinds with no required fields (`QueueUpdated`, `DataSeeded`, and the extra non-spec variants `SpecApproved`, `ConstraintViolation`, `AtomicGroupFailed`) return `Ok(())` for any payload (including `None`).
     - `Custom(_)`: if a payload is present it MUST be a JSON object (reject non-object with `Err`); no required fields. `None` payload is allowed for `Custom`.
     - For kinds with ≥1 required field: `None` payload → `Err`. Payload present but not a JSON object → `Err`. For each required field name: the key MUST be present and non-null in the object (`Value::Null` counts as missing) → else `Err` naming the missing field and kind.
   - The returned `String` is a human-readable reason (e.g. `"payload for kind 'task_assignment' missing required field 'task_id'"`), surfaced verbatim in the 400 body.
   - Do NOT validate field *types* beyond object-ness and presence — the spec table specifies presence/required-ness; type coercion is out of scope. Keep it minimal and exact.

2. **Wire into the REST send path** (`gyre-server/src/api/messages.rs::send_message`).
   - After parsing `kind` (line ~92) and before building the `Message` (line ~265) — a natural spot is right after the tier/destination/scoping checks — call `kind.validate_payload(req.payload.as_ref())` and map `Err(reason)` → `return Err(ApiError::BadRequest(reason))`.
   - `ApiError::BadRequest` already maps to HTTP 400 (confirm in `api/error.rs`).

3. **Wire into the MCP send path** (`gyre-server/src/mcp.rs::handle_message_send`, ~line 1509).
   - After parsing `kind` (~line 1527) call `kind.validate_payload(args.get("payload"))`; map `Err(reason)` → `return tool_error(reason)`. Place it alongside the existing `server_only` / tier guards so it runs before signing/store.

4. **Do NOT touch server-internal emit paths** (`AppState::emit_event`, `build_agent_completed_payload`, `gyre_record_activity`). Those construct payloads server-side and are trusted; the spec scopes validation to *receipt* on the send API. Adding validation there is out of scope and risks breaking existing verified behavior. (If a server-built payload would fail validation, that is a separate bug — do not paper over it here.)

## Acceptance Criteria

- `MessageKind::validate_payload` exists in `gyre-common/src/message.rs` and encodes the required-field sets exactly matching the spec table (every built-in kind covered; `Custom` object-only; no-required kinds permissive).
- `POST /api/v1/workspaces/:id/messages` returns **400** with the reason string when a required field is missing/null or payload is absent for a kind that requires fields; returns 201 when required fields are present.
- The MCP `gyre_message_send` tool returns a `tool_error` (not success) for the same invalid payloads.
- Valid payloads (all required fields present) still succeed on both paths — existing message-bus tests continue to pass (`cargo test -p gyre-server messages`, `cargo test -p gyre-common message`).
- New tests prove enforcement, each of which FAILS if validation is removed:
  - A unit test in `gyre-common` asserting `validate_payload` rejects a `TaskAssignment` with `None` and with `{}` (missing `task_id`), accepts `{"task_id": "..."}`, rejects a `StatusUpdate` missing `summary`, and accepts a `Custom` with any object / rejects a `Custom` with a non-object payload.
  - An integration test on `send_message` asserting a `StatusUpdate` missing `summary` yields `ApiError::BadRequest` (400) and a complete one yields 201.
- No test is self-confirming or mirrors the validation logic; assertions check HTTP status / `Result` variant and the specific missing-field behavior.

## Agent Instructions

- Read `specs/system/message-bus.md` §Payload Schemas (lines 190–229) for the authoritative required-field table — transcribe it exactly; the excerpt above is a convenience copy.
- `MessageKind` lives in `crates/gyre-common/src/message.rs`; `serde_json` is already a dependency there.
- Put the validation logic in `gyre-common` (shared) so both the REST handler and the MCP handler call the same function — do not duplicate the table in two places.
- Follow hexagonal boundaries: `gyre-common` has no infra deps; `validate_payload` returns a plain `Result<(), String>` (no `ApiError` in common). The server layer maps the string to `ApiError::BadRequest` / `tool_error`.
- Confirm `ApiError::BadRequest` → 400 in `crates/gyre-server/src/api/error.rs` before relying on it.
- Skip formatters/linters and project-wide suites; run only the targeted tests above. Conventional commit, author `Project Manager` is NOT you — commit under your worker identity per repo convention.

## Implementation Notes

- **Shared table** — `MessageKind::required_payload_fields()` + `validate_payload()` in `crates/gyre-common/src/message.rs` encode message-bus.md §Payload Schemas once; both receipt paths call it, so the two cannot drift.
- **Receipt paths wired** — `api/messages.rs::send_message` maps `Err` → `ApiError::BadRequest` (→ 400, `api/error.rs:56`), `mcp.rs::handle_message_send` maps `Err` → `tool_error`. Both run after the tier/destination/scoping guards and before signing/persistence, so a rejected payload is never stored.
- **Explicit `null` payload ≡ absent payload** — normalized inside `validate_payload`. The REST body types the field `Option<Value>` (serde maps `"payload": null` → `None`) while the MCP argument map hands over `Some(Value::Null)`; without the normalization the identical wire payload was 400 on one path and accepted on the other. A required field is still unsatisfied by null, whether the payload itself is null or the required key holds null.
- **Non-spec variants** (`SpecApproved`, `ConstraintViolation`, `AtomicGroupFailed`, `MrReverted`, `MergeQueuePaused`, `MergeQueueResumed`) are outside the spec table and server-emitted only, so they impose no required fields — they are listed explicitly in the `match` so adding a spec row for one is a visible edit, not a silent default arm.
- **Server-internal emit paths untouched** per plan (`emit_event`, `build_agent_completed_payload`, `gyre_record_activity`): the spec scopes validation to receipt. `gyre_record_activity`'s payload-shape mismatch against §Payload Schemas is tracked separately in `specs/reviews/task-001.md` F6 — it is a distinct defect, not papered over here.
- **Tests** — `message::tests::validate_payload_enforces_required_fields` and `validate_payload_treats_explicit_null_as_absent` (gyre-common); `api::messages::tests::send_message_rejects_payload_missing_required_field` asserts 400 + reason naming `summary`, 400 for absent payload, 201 when complete; `mcp::tests::mcp_message_send_rejects_payload_missing_required_field` asserts `isError` + reason naming `task_id` **and** that nothing was persisted (`list_unacked` == 0), then that the valid payload persists (== 1). All fail if the validation call is removed.
- **Contract surfaced where hit** — MCP `gyre_message_send` payload description and `docs/api-reference.md` now name the per-kind required fields and the 400 behavior.
