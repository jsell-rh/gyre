# Review: TASK-012 — Specs Assist Real LLM Integration

**Reviewer:** Verifier
**Round:** R3

---

- [x] [process-revision-complete] **F1 — Dead code: `DiffOp` struct defined but never used.**
  `specs_assist.rs:38-42` defines `pub struct DiffOp { op, path, content }` but the handler works entirely with `serde_json::Value` for diff validation and response serialization. `DiffOp` is never instantiated, deserialized into, or referenced anywhere in the codebase (`grep -rn DiffOp crates/` confirms only the definition and prior review comments). Dead type.

- [x] [process-revision-complete] **F2 — Missing budget tracking: spec requires `llm_query` cost entries for `specs/assist`.**
  `ui-layout.md` line 158 states: *"LLM calls from `generate`, `briefing/ask`, and `specs/assist` endpoints are charged to the workspace budget as `llm_query` cost entries."* The `assist_spec` handler records no cost entry. Compare `explorer_views.rs:506-523` which correctly creates a `CostEntry` and calls `state.costs.record()`. The `assist_spec` handler must do the same after the LLM call completes.

- [x] [process-revision-complete] **F3 — Resolved `max_tokens` discarded; `None` passed to `stream_complete`.**
  `specs_assist.rs:226-227`: `let (model, _) = resolve_llm_model(...)` destructures the result and discards `max_tokens` with `_`. Line 230: `stream_complete(&system_prompt, &user_prompt, None)` passes `None`. The spec says `specs/assist: 4,000 tokens` (`ui-layout.md` line 216, configurable via `GYRE_LLM_MAX_TOKENS_ASSIST`). `resolve_llm_model` returns the configured `max_tokens` — it should be passed through to `stream_complete`, not discarded.

- [x] [process-revision-complete] **F4 — MCP handler template substitution broken by TASK-012 prompt template change.**
  TASK-012 changed `PROMPT_SPECS_ASSIST` in `llm_defaults.rs` to use new variables `{{spec_content}}` and `{{graph_context}}`. The MCP handler `handle_spec_assist` (`mcp.rs:2041-2044`) still only substitutes `{{spec_path}}`, `{{draft_content}}`, and `{{instruction}}`. After TASK-012, the MCP handler sends a prompt containing literal `{{spec_content}}` and `{{graph_context}}` text to the LLM. The MCP handler's substitution must be updated to match the new template variables, including loading spec content via `GitOpsPort::read_file` and graph context via `graph_store.get_nodes_by_spec`.

---

## R2

- [-] [process-revision-complete] **F5 — MCP handler returns raw LLM text without response validation (logic divergence).**
  `mcp.rs:2138` returns `tool_result(full_text)` — the raw, unparsed LLM output. The REST handler `assist_spec` at `specs_assist.rs:252-311` performs three validation steps the MCP handler skips entirely: (1) parses the raw text as JSON, (2) validates that `diff` and `explanation` fields exist, (3) validates each diff op has a valid `op` value (`add`/`remove`/`replace`) and a `path` field. On validation failure, the REST handler returns a structured error. The MCP consumer receives raw, unvalidated LLM text — if the LLM returns invalid JSON or malformed diff ops, the consumer gets no error indication and no structured data. This is a logic divergence per HSI §11 MCP parity: the MCP tool must return equivalent data to the REST endpoint, which means parsed `{diff, explanation}` JSON on success, or a structured error on failure.

- [-] [process-revision-complete] **F6 — CLI `spec_assist` display code accesses `op["text"]` which no longer exists after TASK-012 changed the server response format (response field name mismatch).**
  TASK-012 changed the server's `event: complete` data from `{text: full_text}` to `{diff: [...], explanation: "..."}`. The CLI display code at `main.rs:854` still accesses `op["text"].as_str()`, which returns `None` on the new `{diff, explanation}` response. The user runs `gyre spec assist` and sees "Spec assist response:" followed by empty output — all diff ops and explanation are silently dropped. Additionally, the client doc comment at `client.rs:444` says "collected text payloads" (stale — should reference `{diff, explanation}` responses), and `docs/api-reference.md:78` describes the endpoint as "SSE stream of `DiffOp` events" with ops `insert`/`delete`/`replace` — the `DiffOp` type was removed (R1 F1), and the op names should be `add`/`remove`/`replace` per spec. Fix: update `main.rs` display to render `diff` ops and `explanation` from the response; update client doc comment; update api-reference description.

---

## R3

- [-] [process-revision-complete] **F7 — CLI client does not handle `event: error` SSE events — users see misleading "No suggestions returned." when the LLM returns invalid output.**
  TASK-012 introduced `event: error` on the server side (for invalid LLM JSON, missing fields, or invalid diff ops — `specs_assist.rs:296-315`). The CLI client `spec_assist` parser (`client.rs:471-496`) only captures `complete` and `partial` events. When the server sends `event: error`, the client ignores it, returns an empty `ops` vec, and the display code at `main.rs:847-848` prints "No suggestions returned." — a misleading message that implies the LLM had no suggestions rather than reporting the actual error. The spec (`ui-layout.md` §3 line 167) defines `event: error` as part of the SSE contract. The R2 fix (F6) already updated this client code for the new `complete` event format but did not add `error` event handling. Fix: in the client's SSE parser (`client.rs:483-494`), add an `else if current_event == "error"` branch that captures the error data. In the display code (`main.rs:847`), check for error events and print the error message (e.g., `eprintln!("Error: {}", error_data["error"].as_str())`) before or instead of the "No suggestions returned." fallback.
