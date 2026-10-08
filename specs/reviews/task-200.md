# Review — task-200 (Message bus — per-kind payload schema validation, reject invalid payloads with 400)

Spec: `specs/system/message-bus.md` §Payload Schemas (lines 190–229): "Each `MessageKind` has a defined payload schema. The server validates payloads on receipt — invalid payloads are rejected with 400", plus the per-kind required-field table.
Commits under review (scoping per `commits:` frontmatter): `9764477` (required-field table + `validate_payload` + gyre-common unit tests), `4184658` (REST/MCP wiring + REST integration test), `c0cbeac` (null-payload normalization + MCP test), `e71d72f` (contract docs on both send paths), `eecd06d`/`5408311` (controller sandbox-preservation pair that briefly gutted then restored the function body — see F1 history note; final state verified enforcing).
Verdict: **complete**.

## Round 1

Test runs (this checkout, branch `devloop/task-200/attempt-9` at `4d8d4a9`):

- `cargo test -p gyre-common --lib message` → **29 passed, 0 failed** (incl. `validate_payload_enforces_required_fields`, `validate_payload_treats_explicit_null_as_absent`).
- `cargo test -p gyre-server --lib api::messages` → **14 passed, 0 failed** (incl. `send_message_rejects_payload_missing_required_field`, `send_message_agent_not_found_returns_404`).
- `cargo test -p gyre-server --lib mcp_message_send` → **8 passed, 0 failed** (incl. `mcp_message_send_rejects_payload_missing_required_field`).
- `cargo test -p gyre-server --lib "message"` → **36 passed, 0 failed** (broad sweep: signing, ack, telemetry, ws, mcp).
- `cargo test -p gyre-server --test api_integration agent_messages_send_and_receive` → **FAILED — pre-existing environment flake, not this task**: same failure reproduces at the pre-task-200 baseline `9764477~1` and with task-200 changes stashed (reqwest `Canceled`/`IncompleteMessage` on `POST /api/v1/agents` at api_integration.rs:82, i.e. during agent *creation*, before any message is sent; no causal path through payload validation). The test itself sends a *valid* payload (`task_assignment` with `task_id`), so it exercises acceptance, not rejection.
- Mutation probes (restore verified by `git status --short` clean after each):
  - REST: replace the `kind.validate_payload(req.payload.as_ref())` call in `api/messages.rs:259` with a constant `Ok` → `send_message_rejects_payload_missing_required_field` **FAILS**.
  - MCP: replace the `kind.validate_payload(args.get("payload"))` call in `mcp.rs:1948` with a constant `Ok` → `mcp_message_send_rejects_payload_missing_required_field` **FAILS**.
  Both tests are genuinely anchored to the enforcement, not to incidental behavior.
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-mcp-write-tools.sh` → OK. `bash scripts/check-dead-message-kinds.sh` → OK. `bash scripts/check-inert-enforcement.sh` → OK. `bash scripts/check-task-commit-attribution.sh` → OK (all six product-surface commits listed in frontmatter).

Verified working (no findings):

- **Required-field table transcribes the spec exactly.** `MessageKind::required_payload_fields` (`crates/gyre-common/src/message.rs:292-344`) checked row-by-row against message-bus.md lines 196–229: all 34 spec-listed kinds match, field names and cardinality identical (e.g. `AgentContainerSpawned` → agent_id/container_id/image/runtime; `StaleSpecWarning` → mr_id/repo_id/spec_path/spec_sha/current_sha; `PushAccepted` → repo_id/branch/agent_id only, matching the spec's note that commit_count/task_id/ralph_step are optional; `StatusUpdate` → status+summary). The six non-spec variants (`SpecApproved`, `ConstraintViolation`, `AtomicGroupFailed`, `MrReverted`, `MergeQueuePaused`, `MergeQueueResumed`) are listed in explicit match arms with empty slices — no silent wildcard; adding a spec row for one is a visible edit. `Custom(_)` → empty slice.
- **`validate_payload` enforces exactly the specced semantics** (`message.rs:354-392`): absent-or-null payload with ≥1 required field → `Err` naming kind + first missing field; present non-object payload → `Err` "must be a JSON object"; each required key must be present **and non-null** (`Value::Null` counts as missing — matches the plan's null-is-missing rule and is covered by both unit tests); kinds with no required fields accept any/absent payload; `Custom` accepts absent or any object, rejects non-object. Types beyond object-ness deliberately unchecked, per plan. Return type is plain `Result<(), String>` — hexagonal boundary respected (`gyre-common` takes no `ApiError`).
- **REST path wired and 400-mapped.** `api/messages.rs:256-261`: validation runs after tier/destination/scoping guards and *before* the `Message` is built, signed, or stored — a rejected payload is never persisted or broadcast. `ApiError::BadRequest` → `StatusCode::BAD_REQUEST` confirmed at `api/error.rs:56`. The 400 body carries the reason verbatim (`json["error"]` asserted to contain `"summary"` in the test).
- **MCP path wired.** `mcp.rs:1946-1950`: same placement discipline (after `server_only`/tier/destination/workspace guards at 1862-1944, before `Message` construction at 1962 and signing/store at 1976-1988). `Err` → `tool_error(reason)`, so the client sees `isError: true` with the reason naming the missing field.
- **Null-payload parity between the two receipt paths is real, not asserted.** REST body types `payload: Option<Value>` (serde maps `"payload": null` → `None`); MCP hands over `Some(Value::Null)` for the same wire form. `validate_payload` normalizes via `payload.filter(|v| !v.is_null())` (`message.rs:361`), so the identical wire payload gets the same verdict on both paths — the `validate_payload_treats_explicit_null_as_absent` unit test pins this.
- **Both paths share one table** — `grep validate_payload crates/gyre-server/src` shows exactly two production call sites (api/messages.rs:259, mcp.rs:1948) both calling the `gyre-common` function; the table exists once and cannot drift between paths.
- **Tests are hard, not self-confirming.** REST test asserts HTTP 400 twice (missing `summary`; absent payload for a required kind) + reason-content + 201 for the complete payload — status-level assertions, no logic mirroring. MCP test additionally asserts the *persistence* contract: `list_unacked == 0` after the rejection and `== 1` after the valid send, which kills any "reject but still store" regression. Both mutation probes confirm the tests fail when the calls are removed.
- **Coverage scope respected — no out-of-scope edits.** `git diff 9764477~1..HEAD --stat` touches only `message.rs`, `api/messages.rs`, `mcp.rs`, `docs/api-reference.md`, and the task/spec bookkeeping files. Server-internal emit paths (`emit_event`, `build_agent_completed_payload`, `gyre_record_activity`) untouched, per plan item 4; the `gyre_record_activity` payload-shape mismatch remains tracked in task-001 F6, not papered over here. No gates weakened, no exemptions added, no tests deleted.
- **Contract surfaced where hit**: MCP `gyre_message_send` `payload` description and `docs/api-reference.md` now name the required-field behavior and the 400 (commit `e71d72f`).
- **Legacy pre-bus endpoint excluded correctly**: `api/agent_messages.rs::send_message` (`POST /api/v1/agents/{id}/messages`) carries no `MessageKind` at all (its request is `{from, content, message_type?}`) and the module's own comment marks it superseded by message-bus Phase 3; the spec's per-kind table is inapplicable to it. Only the two `Message`-bus receipt paths exist (`grep "payload: req.payload|payload: args"` finds exactly the two wired sites).

Findings:

- (none)

History note (not a finding against the final state): the controller's sandbox-preservation commits `eecd06d` → `5408311` briefly left `validate_payload` as `let _ = payload; Ok(())` (both gyre-common validation tests failed at `eecd06d`). `5408311` restores the enforcing body byte-identical to the `c0cbeac` state, and the final tree was re-verified end-to-end here (unit + integration + both mutation probes). The restored commit is recorded in the `commits:` frontmatter, so review scoping covers it.

## Shipped

- `MessageKind::validate_payload` + `required_payload_fields` in `gyre-common` encode message-bus.md §Payload Schemas once (all 34 spec kinds, exact required-field sets; non-spec variants in explicit empty arms; `Custom` object-only), returning a caller-surfaced reason string.
- Both message receipt paths enforce it before sign/store: REST `POST /api/v1/workspaces/:id/messages` rejects a missing/null required field, absent payload for a required kind, or non-object payload with **400** naming the field; MCP `gyre_message_send` returns a tool error for the same, with nothing persisted.
- Explicit JSON `null` payload is normalized to "absent" inside the validator, so the REST (`None`) and MCP (`Some(Null)`) paths return identical verdicts for the same wire payload; a null required-field value still fails.
- Hard tests anchor the enforcement on both paths (HTTP status + reason content + MCP persistence counts), each verified to fail when its validation call is removed; MCP tool description and `docs/api-reference.md` document the contract.

— Verifier, 2026-10-08
