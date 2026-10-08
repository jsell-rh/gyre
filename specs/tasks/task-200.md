---
title: "Message bus — per-kind payload schema validation (reject invalid payloads with 400)"
spec_ref: "message-bus.md §Payload Schemas"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "message-bus.md §Payload Schemas"
commits: ["15fa2b969a5c5fd26cb53c134e6edad3478a06c3", "e44f11354629cf2dab7fd7846c8a0a0a1d2d9591", "61ab1826", "9e80fa76d0c502a4afbc9ce747e8eb6e1f2500d4", "a5bb0560", "7fb1fa12", "af448f7b", "66aefab8", "a680a062", "2d4f9cb0", "fe1b8dba", "6e8a33fa", "9897458d"]
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
   - The spec table (message-bus.md lines 194–229) declares a wire type per field (`Id`/`String`, `u64`, `u32`, `f64`, `Vec<String>`, nested `decisions` objects), not just requiredness. Encode the full field schema — name, wire type, requiredness — and validate every schema-known field that is present against its declared type (required or optional). Unknown extension fields pass through; absent optional fields stay valid; no type coercion. (The earlier "presence-only" reading here was wrong — see the independent spec review that rejected attempt 10; the spec is the contract.)

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

- **Shared table** — `MessageKind::payload_schema` + `validate_payload` in `crates/gyre-common/src/message.rs` encode message-bus.md §Payload Schemas once — per-kind field name, wire type (Id/String, u64, u32, f64, Vec<String>, nested `decisions` objects), and requiredness — so both receipt paths call the same table and cannot drift.
- **Receipt paths wired** — `api/messages.rs::send_message` maps `Err` → `ApiError::BadRequest` (→ 400, `api/error.rs:56`), `mcp.rs::handle_message_send` maps `Err` → `tool_error`. Both run after the tier/destination/scoping guards and before signing/persistence, so a rejected payload is never stored.
- **Explicit `null` payload ≡ absent payload** — normalized inside `validate_payload`. The REST body types the field `Option<Value>` (serde maps `"payload": null` → `None`) while the MCP argument map hands over `Some(Value::Null)`; without the normalization the identical wire payload was 400 on one path and accepted on the other. A required field is still unsatisfied by null, whether the payload itself is null or the required key holds null.
- **Non-spec variants** (`SpecApproved`, `ConstraintViolation`, `AtomicGroupFailed`, `MrReverted`, `MergeQueuePaused`, `MergeQueueResumed`) are outside the spec table and server-emitted only, so they impose no required fields — they are listed explicitly in the `match` so adding a spec row for one is a visible edit, not a silent default arm.
- **Server-internal emit paths untouched** per plan (`emit_event`, `build_agent_completed_payload`, `gyre_record_activity`): the spec scopes validation to receipt. `gyre_record_activity`'s payload-shape mismatch against §Payload Schemas is tracked separately in `specs/reviews/task-001.md` F6 — it is a distinct defect, not papered over here.
- **Tests** — `message::tests::validate_payload_enforces_required_fields`, `validate_payload_enforces_field_types` (u64/u32/f64/Vec<String>/nested decisions, required and optional fields), and `validate_payload_treats_explicit_null_as_absent` (gyre-common); `api::messages::tests::send_message_rejects_payload_missing_required_field` asserts 400 + reason naming `summary`, 400 for absent payload, 400 + reason naming `task_id` for `{"task_id": false}`, 201 when complete; `mcp::tests::mcp_message_send_rejects_payload_missing_required_field` asserts `isError` + reason naming `task_id` for missing **and** wrongly-typed fields, that nothing was persisted (`list_unacked` == 0), then that the valid payload persists (== 1). All fail if the validation call is removed; the type tests fail if the type match is disabled (mutation-probed).
- **Contract surfaced where hit** — MCP `gyre_message_send` payload description and `docs/api-reference.md` now name the per-kind required fields and the 400 behavior.

## Shipped

- `MessageKind::payload_schema` + `validate_payload` in `gyre-common` encode message-bus.md §Payload Schemas once — all 34 spec kinds with exact field names, wire types (Id/String, u64, u32, f64, Vec<String>, nested `decisions`), and requiredness; non-spec variants in explicit empty match arms; `Custom` object-only — returning a reason string surfaced verbatim to the caller.
- Both receipt paths enforce the full schema before sign/store: REST `POST /api/v1/workspaces/:id/messages` rejects missing/null required fields, absent payloads for required kinds, non-object payloads, and wrongly-typed schema-known fields (required or optional) with **400** naming the field (`api/messages.rs:259`); MCP `gyre_message_send` returns a tool error for the same with nothing persisted (`mcp.rs:1948`). Unknown extension fields pass through; absent optional fields stay valid; no coercion.
- Explicit JSON `null` payload is normalized to "absent" inside the validator, so the REST (`None`) and MCP (`Some(Null)`) paths give identical verdicts for the same wire payload; a null value for a required key still fails.
- Tests anchor the enforcement on both paths (HTTP status + reason content + MCP persistence counts) and were mutation-probed: each fails when its validation call is removed, and the type tests fail when the type match is disabled. Verified in review `specs/reviews/task-200.md`.

## Repair (rejected integration 848395b, re-land attempt 10)

Two defects, both in the re-land's bookkeeping — `git diff 848395b..HEAD -- crates/` is empty, so the product surface is byte-identical to the reviewed candidate:

Those six candidate-lineage SHAs (`9764477`…`5408311`) are kept out of the `commits:` frontmatter on purpose: they are dangling objects reachable in no ref, so a fresh clone cannot resolve them and any automated scoping over the list would break. The re-land lineage (`62c0730`…`b414c50`, six commits, same subjects, `crates/`-identical trees modulo dev-loop machinery) is what the frontmatter records.

Verification: `git diff --check f315b6f..HEAD` clean; `check-rustfmt-diff.py f315b6f` and `check-clippy-diff.py f315b6f` clean; `bash scripts/check-task-commit-attribution.sh` exits 0 on the repaired tree; all 20 static gate scripts OK on HEAD; targeted suites green (`gyre-common message` 29 passed, `gyre-server api::messages` 14 passed, `gyre-server mcp_message_send` 8 passed). No gate weakened, no exemption entry added (exemption file untouched at 3), no test deleted.

- **Latent rustfmt violations** (masked by the whitespace failure — the gate stops at `git diff --check`): against the task base `f315b6f`, `check-rustfmt-diff.py` flagged changed lines this task added in `message.rs`, `api/messages.rs`, and `mcp.rs`. Formatted exactly those lines (commit `4c175765`); no logic changed — assertions preserved, targeted suites re-run green after the edit.