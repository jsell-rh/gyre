---
title: "MCP HSI Parity — Resources and Tools"
spec_ref: "human-system-interface.md §11 (CLI/MCP Parity Constraint)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-010.md
coverage_sections: []
commits: ["67985e8d9d51b99cf89a0f41ab7707b84ec18bce", "275692a808a228d725ba27365a5461d778a26602"]
---

## Spec Excerpt

From `human-system-interface.md` §11:

> **Every data surface in the UI must be consumable outside the browser.**
>
> | UI Surface | CLI Equivalent | MCP Equivalent |
> |---|---|---|
> | Briefing | `gyre briefing ...` | Resource: `briefing://` |
> | Inbox | `gyre inbox ...` | Resource: `notifications://` |
> | Explorer (concept search) | `gyre explore <concept>` | Tool: `graph.concept` |
> | System Trace | `gyre trace <mr-id>` | Resource: `trace://<mr-id>` |
> | Spec editing assist | `gyre spec assist <path> ...` | Tool: `spec.assist` |
> | Divergence alerts | `gyre divergence ...` | (via notifications) |

## Current State

The MCP server (`crates/gyre-server/src/mcp.rs`) currently exposes these resources:
- `spec://` — read spec markdown files
- `agents://` — list active agents
- `queue://` — merge queue entries
- `conversation://context` — interrogation agent context

And these tools:
- `gyre_create_task`, `gyre_list_tasks`, `gyre_update_task`
- `gyre_create_mr`, `gyre_list_mrs`
- `gyre_record_activity`
- `gyre_agent_heartbeat`, `gyre_agent_complete`
- `gyre_analytics_query`, `gyre_search`
- `conversation.upload`
- `gyre_message_send`, `gyre_message_poll`, `gyre_message_ack` (from TASK-001)

**Missing:** The HSI §11 parity table defines MCP resources and tools that do not exist:
1. `briefing://` resource — workspace briefing narrative
2. `notifications://` resource — inbox notifications
3. `trace://<mr-id>` resource — SDLC system trace for an MR
4. `graph.concept` tool — concept search over the knowledge graph
5. `spec.assist` tool — LLM-assisted spec editing

## Implementation Plan

1. **`briefing://` resource** — Add to `resource_definitions()` and `handle_resources_read()`:
   - URI template: `briefing://{workspace_id}` (optional `?since=<epoch>`)
   - Wraps `GET /api/v1/workspaces/:workspace_id/briefing`
   - Returns the briefing JSON (sections: completed, in_progress, cross_workspace, exceptions, metrics)

2. **`notifications://` resource** — Add to `resource_definitions()` and `handle_resources_read()`:
   - URI template: `notifications://{workspace_id}` (optional `?min_priority=&max_priority=`)
   - Wraps `GET /api/v1/users/me/notifications?workspace_id=`
   - Returns notification list JSON

3. **`trace://` resource** — Add to `resource_definitions()` and `handle_resources_read()`:
   - URI template: `trace://{mr_id}`
   - Wraps `GET /api/v1/merge-requests/:id/trace`
   - Returns GateTrace JSON (spans, root_spans, service_graph, commit_sha, gate_run_id)

4. **`graph.concept` tool** — Add tool definition:
   - Parameters: `concept` (string), `repo_id` (optional), `workspace_id` (optional), `depth` (optional, default 2)
   - When `repo_id` provided: wraps `GET /api/v1/repos/:id/graph/concept/<name>`
   - When `workspace_id` provided: wraps `GET /api/v1/workspaces/:id/graph/concept/:name`
   - Returns matching graph nodes with type, name, qualified_name, spec linkage

5. **`spec.assist` tool** — Add tool definition:
   - Parameters: `repo_id` (required), `spec_path` (required), `instruction` (required), `draft_content` (optional)
   - Wraps `POST /api/v1/repos/:repo_id/specs/assist`
   - Returns diff suggestions (op, path, content) and explanation

6. Add tests for all new resources and tools.

## Acceptance Criteria

- [ ] `briefing://` resource returns workspace briefing
- [ ] `notifications://` resource returns inbox notifications
- [ ] `trace://` resource returns MR gate trace (GateTrace: spans, root_spans, service_graph)
- [ ] `graph.concept` tool searches the knowledge graph by concept
- [ ] `spec.assist` tool returns LLM-suggested spec edits
- [ ] All resources/tools respect agent JWT auth and workspace scoping
- [ ] Tests for each new resource and tool

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/mcp.rs` for the existing resource and tool patterns
3. Read `crates/gyre-server/src/api/graph.rs` for the briefing and graph handlers
4. Read `crates/gyre-server/src/api/specs_assist.rs` for the assist handler
5. Implement following the existing patterns in `mcp.rs`
6. Add tests following the existing test pattern
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(mcp): add HSI §11 parity resources and tools (TASK-010)`

## Shipped

All five HSI §11 parity surfaces are real production code, and the tools
are now reachable under BOTH spellings the spec has carried.

### Behavior

- `briefing://{workspace_id}` resource: three-step `since` resolution
  (explicit `?since=` → `user_workspace_state.get_last_seen` → 24h
  fallback), delegates to the same `assemble_briefing` the REST handler
  calls.
- `notifications://{workspace_id}` resource: resolves the user from
  agent JWT auth, honors `?min_priority=`/`?max_priority=`, serializes
  through `NotificationResponse` with the REST envelope.
- `trace://{mr_id}` resource: verifies the MR exists, delegates to
  `assemble_gate_trace` (spans, root_spans, service_graph, commit_sha,
  gate_run_id).
- `graph_concept` tool: requires `concept` + a scope (`repo_id` or
  `workspace_id`), delegates to the shared `assemble_concept_results`
  used by both REST handlers.
- `spec_assist` tool: requires `repo_id`/`spec_path`/`instruction`;
  replicates the REST pipeline (rate limit → LLM config → spec content →
  graph context → prompt → LLM call → diff validation).

### Dotted-name parity repair (finding fd72a1bf)

The assigned requirement inputs quote the §11 parity table with dotted
tool names (`graph.concept`, `spec.assist`); the shipped surface only
accepted snake_case, so `tools/call` with a dotted name returned
"Unknown tool". Commit 275692a80 closes that contract contradiction in
the server itself:

- `tools/call` normalizes the dotted §11 spellings to the canonical
  snake_case names before the `needs_write` RBAC gate and dispatch
  (both tools are read-only; gate membership unchanged).
- `tools/list` advertises the dotted spellings as alias entries DERIVED
  from the canonical definitions (same inputSchema and description), so
  the two spellings cannot drift.
- `docs/api-reference.md` documents the alias surface; the §11 coverage
  note records it.

The branch also carries the prior-epoch spec amendment (5b06eded9)
renaming the in-repo §11 table cells to the shipped snake_case surface;
protected spec publication stays with the controller. With the aliases,
the server satisfies either spec text.

### Verification repair (finding b550b5ccf)

Changed-line rustfmt failures at mcp.rs:606, 6886-6888, 6967
(tuple-array literal wrapping, chained `create_node` call). Commit
67985e8d9 applies `cargo fmt`; whitespace-only, no behavior change.
`cargo fmt -p gyre-server -- --check` now passes clean.

### Test evidence

Focused run (`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- dotted
mcp_graph_concept mcp_spec_assist mcp_tools_list mcp_briefing
mcp_notifications mcp_trace`): 23 passed, 0 failed — the 3 new alias
tests plus all pre-existing TASK-010 resource/tool tests.

- `mcp_tools_list_includes_dotted_parity_aliases` — both spellings
  listed with identical inputSchemas.
- `mcp_graph_concept_dotted_alias_dispatches_projection` — dotted name
  returns the real manifest-based concept projection, byte-identical to
  the canonical-name result.
- `mcp_spec_assist_dotted_alias_dispatches` — dotted name reaches repo
  resolution ("repo not found"), not "Unknown tool".

Mutation probes (evidence: /tmp/stage/review-evidence/): removing the
normalization fails both dispatch tests with "Unknown tool: …"; removing
the advertising fails the tools/list test. Mechanical gates:
check-mcp-write-tools OK (8 tools, all gated); check-mcp-wrapper-parity
violation set identical to the assignment base (line-number shifts
only).
