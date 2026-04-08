# TASK-010 Review: MCP HSI Parity — Resources and Tools

## R1 — 2026-04-08

Reviewer: Verifier
Commit under review: `18c53745`

### Findings

- [-] [process-revision-complete] **F1: `depth` parameter is dead code in `handle_graph_concept`** — `mcp.rs:1962-1966`: the `depth` parameter is parsed from args and stored in `_depth` (underscore prefix = compiler-acknowledged unused variable), capped at 5. The tool schema advertises "Neighbor traversal depth (default 2, max 5)" but depth has zero effect on query results — users who pass `depth=0` or `depth=5` get identical output. The underlying REST endpoint `GET /api/v1/repos/:id/graph/concept/:concept_name` (graph.rs:579-623) does not accept a depth parameter either, so this is an invented parameter that was never wired up. Either implement depth-based neighbor traversal or remove the parameter from the tool schema and handler.

- [-] [process-revision-complete] **F2: Briefing MCP resource omits `completed_agents` and `summary` data that the REST endpoint provides** — `mcp.rs:2345-2346`: the MCP handler returns `"completed_agents": []` and `"summary": ""` (empty stubs). The REST handler `get_workspace_briefing` at `graph.rs:857-911` populates `completed_agents` from message bus `agent_completed` Event-tier messages, and at `graph.rs:913-937` generates a computed summary string (e.g., "3 MRs merged, 2 tasks in progress since 14 hours ago"). HSI §11 states "Every data surface in the UI must be consumable outside the browser" — the MCP resource returns strictly less data than the REST endpoint for an identical workspace and since window. An MCP consumer (e.g., an LLM editor) that reads `briefing://ws-1` gets no agent completion data and no summary, while a REST consumer gets both. Fix: either delegate to the same assembly logic the REST handler uses, or replicate the message bus query and summary generation.

- [-] [process-revision-complete] **F3: Briefing MCP items missing `spec_path` field** — `mcp.rs:2304-2310` (completed items) and `mcp.rs:2318-2323` (in_progress items): neither JSON object includes `spec_path`. The REST handler populates `spec_path` from `mr.spec_ref` for completed MRs (`graph.rs:816-819`) and from `t.spec_path` for in-progress tasks (`graph.rs:836`). `BriefingItem` struct defines `spec_path: Option<String>` (`graph.rs:265`). This is a silent response field drop — MCP consumers cannot see which spec each briefing item relates to, breaking the parity constraint.

- [-] [process-revision-complete] **F4: Edge filter semantic divergence in `handle_graph_concept` vs REST endpoint** — `mcp.rs:2013-2015`: the MCP handler collects edges where EITHER source OR target is in the matched node set (`||`). The REST handler at `graph.rs:610-614` collects edges where BOTH source AND target are in the matched node set (`&&`). The same concept query produces different edge sets: the MCP tool returns edges connecting matched nodes to non-matched nodes (broader), while REST only returns edges internal to the matched set (narrower). Since HSI §11 says MCP is a consumer of the same REST API, the MCP tool should produce results consistent with the REST endpoint. Fix: change `||` to `&&` in the edge filter at mcp.rs:2014.

## R2 — 2026-04-08

Reviewer: Verifier
Commit under review: `fe7fc664`

R1 findings F1–F4 were addressed: `depth` parameter removed, briefing delegates to `assemble_briefing`, trace delegates to `assemble_gate_trace`, edge filter operator corrected to `&&`. However, `handle_graph_concept` was only partially fixed — the R1 fix addressed the edge filter operator and dead parameter but did not address the broader reimplementation problem. The tool still hand-builds JSON instead of delegating to or serializing through the REST handler's response types.

### Findings

- [-] [process-revision-complete] **F5: `graph_concept` MCP tool hand-builds JSON, silently dropping 11 node fields and 4 edge fields vs REST `GraphNodeResponse`/`GraphEdgeResponse`** — `mcp.rs:2016-2044`: the MCP handler constructs node objects with 9 fields (`id`, `repo_id`, `node_type`, `name`, `qualified_name`, `file_path`, `line_start`, `line_end`, `spec_path`) while the REST endpoint returns `GraphNodeResponse` (graph.rs:45-64) with 20 fields. Missing from MCP: `visibility`, `doc_comment`, `spec_confidence`, `last_modified_sha`, `last_modified_by`, `last_modified_at`, `created_sha`, `created_at`, `complexity`, `churn_count_30d`, `test_coverage`. Similarly, edge objects have 5 fields (`id`, `repo_id`, `source_id`, `target_id`, `edge_type`) while `GraphEdgeResponse` (graph.rs:116-129) has 9 fields. Missing: `metadata`, `first_seen_at`, `last_seen_at`, `deleted_at`. HSI §11 requires MCP to be a consumer of the same REST API — an MCP consumer gets a fraction of the data a REST consumer gets. Fix: serialize through `GraphNodeResponse`/`GraphEdgeResponse` (via `Into::into` conversion and `serde_json::to_value`) the same way the R1 fix did for briefing and trace resources.

- [-] [process-revision-complete] **F6: `graph_concept` MCP tool adds `deleted_at.is_none()` predicate on nodes and edges that REST does not apply** — `mcp.rs:1988` filters `n.deleted_at.is_none()` on nodes and `mcp.rs:2007` filters `edge.deleted_at.is_none()` on edges. The REST handler at `graph.rs:591-597` (repo-level) and `graph.rs:1129-1134` (workspace-level) does NOT filter by `deleted_at` on either nodes or edges. The REST `GraphEdgeResponse` explicitly includes `deleted_at: Option<u64>` as a response field. This means the MCP tool returns fewer nodes and edges than the REST endpoint for the same concept query — a predicate divergence that violates HSI §11 parity. Fix: remove the `deleted_at.is_none()` guards, or (better) delegate to a shared function that both REST and MCP call.

- [-] [process-revision-complete] **F7: `graph_concept` MCP `node_type` and `edge_type` serialization produces wrong values for multi-word enum variants** — `mcp.rs:2022` uses `format!("{:?}", n.node_type).to_lowercase()` (Debug format, then lowercased) while the REST endpoint serializes `NodeType` via serde with `#[serde(rename_all = "snake_case")]` (graph.rs:8). For single-word variants (e.g., `Module` → `"module"`) these coincidentally match. For multi-word variants they diverge: `EnumVariant` → MCP: `"enumvariant"`, REST: `"enum_variant"`. Same for `edge_type` (graph.rs:36): `DependsOn` → MCP: `"dependson"`, REST: `"depends_on"`; `FieldOf` → MCP: `"fieldof"`, REST: `"field_of"`; `RoutesTo` → MCP: `"routesto"`, REST: `"routes_to"`; `PersistsTo` → MCP: `"persiststo"`, REST: `"persists_to"`; `GovernedBy` → MCP: `"governedby"`, REST: `"governed_by"`; `ProducedBy` → MCP: `"producedby"`, REST: `"produced_by"`. Consumers that discriminate on node_type or edge_type strings will silently fail to match MCP values against REST values. Fix: serialize through the response structs (resolves F5, F6, and F7 together).

Root cause for F5–F7: `handle_graph_concept` reimplements query logic and response construction with hand-built `json!({})` instead of delegating to a shared function like the R1 fixes did for `assemble_briefing` and `assemble_gate_trace`. The fix pattern is established: extract the concept-search logic into a shared `assemble_concept_results(state, repo_ids, pattern)` function that returns `KnowledgeGraphResponse`, then call it from both the REST handler and MCP tool. This resolves all three findings at once.

## R3 — 2026-04-08

Reviewer: Verifier
Commit under review: `773e87d6`

R2 findings F5–F7 were addressed: `assemble_concept_results` extracted as shared function in `graph.rs`, both REST handlers (`get_graph_concept`, `get_workspace_graph_concept`) and MCP tool (`handle_graph_concept`) now delegate to it. Hand-built JSON, Debug-format enums, and `deleted_at` predicate divergence are all resolved by the shared function. Clean build, all tests pass.

### Findings

- [-] [process-revision-complete] **F8: `briefing://` MCP resource skips `last_seen_at` default, diverging from REST handler's `since` resolution** — `mcp.rs:2257-2258`: when no `?since=` query param is provided, the MCP handler falls back directly to `now - 24h`. The REST handler `get_workspace_briefing` at `graph.rs:974-988` uses a three-step resolution: (1) explicit `?since=` param, (2) `user_workspace_state.get_last_seen(uid, workspace_id)`, (3) 24h fallback. The MCP handler skips step 2. An MCP consumer who visited the workspace 8 hours ago and reads `briefing://ws-1` (no explicit `since`) receives 24 hours of items, while a REST consumer receives only 8 hours. This violates HSI §11's parity constraint — the same implicit request produces different results depending on access method. The `auth` parameter is available in `handle_resource_read`, and `state.user_workspace_state` is accessible. Fix: replicate the REST handler's `since` resolution logic — check `auth.user_id`, look up `last_seen_at`, fall back to 24h.

- [-] [process-revision-complete] **F9: `assemble_concept_results` doc comment contains stale route annotation `GET /api/v1/repos/{id}/graph/concept/{name}`** — `graph.rs:574`: the first line of the doc comment on `assemble_concept_results` is `/// GET /api/v1/repos/{id}/graph/concept/{name}` — a route annotation that described the REST handler before the function was extracted. `assemble_concept_results` is not an HTTP handler; it's a shared assembly function called by both REST and MCP. The stale route annotation is misleading — a reader scanning for endpoint definitions will find this function and assume it's a handler. The actual handler `get_graph_concept` at `graph.rs:649` lost its route annotation in the refactoring. Fix: remove the `GET /api/v1/repos/...` line from `assemble_concept_results`'s doc comment, and add `/// GET /api/v1/repos/{id}/graph/concept/{name}` to `get_graph_concept`.

## R4 — 2026-04-08

Reviewer: Verifier
Commit under review: `64513d92`

R3 findings F8–F9 resolved. Verification:

- **F8:** MCP `briefing://` handler now implements the three-step `since` resolution: explicit `?since=` param → `user_workspace_state.get_last_seen()` → 24h fallback. Logic is byte-for-byte equivalent to the REST handler at `graph.rs:974-988`. New test `mcp_briefing_resource_uses_last_seen_at_default` verifies the `last_seen_at` lookup produces the correct `since` value.

- **F9:** Route annotation `/// GET /api/v1/repos/{id}/graph/concept/{name}` removed from `assemble_concept_results` (shared assembly function) and restored on `get_graph_concept` (the actual handler) at `graph.rs:644`.

Sweep for new findings:
- All five new MCP handlers (`briefing://`, `notifications://`, `trace://`, `graph_concept`, `spec_assist`) delegate to shared assembly functions or serialize through response structs — no hand-built JSON for data responses.
- Enum serialization uses serde (via `assemble_concept_results` → `KnowledgeGraphResponse` → `GraphNodeResponse`/`GraphEdgeResponse` with `#[serde(rename_all = "snake_case")]`) — no Debug-format divergence in TASK-010 code.
- `notifications://` response envelope matches REST handler at `users.rs:313` (field-for-field: `notifications`, `limit`, `offset`).
- `trace://` delegates to `assemble_gate_trace` — same function the REST handler calls.
- `spec_assist` replicates REST handler's LLM call pipeline (rate limit → prompt template → model resolution → stream collect) with the same parameters.
- All tests assert on observable response structure, not just "no panic".
- `scripts/check-mcp-wrapper-parity.sh` — no violations in TASK-010 code (flagged items are pre-existing handlers).
- `scripts/check-cli-spec-parity.sh` — passes.
- `scripts/check-placeholder-stubs.sh` — passes.
- All 1,843 tests pass (0 failures, 14 ignored).

No new findings. **TASK-010 verified complete.**
