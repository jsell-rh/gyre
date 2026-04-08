# Review: TASK-012 — Specs Assist Real LLM Integration

**Reviewer:** Verifier
**Round:** R1

---

- [ ] **F1 — Dead code: `DiffOp` struct defined but never used.**
  `specs_assist.rs:38-42` defines `pub struct DiffOp { op, path, content }` but the handler works entirely with `serde_json::Value` for diff validation and response serialization. `DiffOp` is never instantiated, deserialized into, or referenced anywhere in the codebase (`grep -rn DiffOp crates/` confirms only the definition and prior review comments). Dead type.

- [ ] **F2 — Missing budget tracking: spec requires `llm_query` cost entries for `specs/assist`.**
  `ui-layout.md` line 158 states: *"LLM calls from `generate`, `briefing/ask`, and `specs/assist` endpoints are charged to the workspace budget as `llm_query` cost entries."* The `assist_spec` handler records no cost entry. Compare `explorer_views.rs:506-523` which correctly creates a `CostEntry` and calls `state.costs.record()`. The `assist_spec` handler must do the same after the LLM call completes.

- [ ] **F3 — Resolved `max_tokens` discarded; `None` passed to `stream_complete`.**
  `specs_assist.rs:226-227`: `let (model, _) = resolve_llm_model(...)` destructures the result and discards `max_tokens` with `_`. Line 230: `stream_complete(&system_prompt, &user_prompt, None)` passes `None`. The spec says `specs/assist: 4,000 tokens` (`ui-layout.md` line 216, configurable via `GYRE_LLM_MAX_TOKENS_ASSIST`). `resolve_llm_model` returns the configured `max_tokens` — it should be passed through to `stream_complete`, not discarded.

- [ ] **F4 — MCP handler template substitution broken by TASK-012 prompt template change.**
  TASK-012 changed `PROMPT_SPECS_ASSIST` in `llm_defaults.rs` to use new variables `{{spec_content}}` and `{{graph_context}}`. The MCP handler `handle_spec_assist` (`mcp.rs:2041-2044`) still only substitutes `{{spec_path}}`, `{{draft_content}}`, and `{{instruction}}`. After TASK-012, the MCP handler sends a prompt containing literal `{{spec_content}}` and `{{graph_context}}` text to the LLM. The MCP handler's substitution must be updated to match the new template variables, including loading spec content via `GitOpsPort::read_file` and graph context via `graph_store.get_nodes_by_spec`.
