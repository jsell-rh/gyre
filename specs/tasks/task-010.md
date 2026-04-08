# TASK-010: MCP HSI Parity — Resources and Tools

**Spec reference:** `human-system-interface.md` §11 (CLI/MCP Parity Constraint)  
**Depends on:** None (consumes existing REST endpoints and domain logic)  
**Progress:** `complete`
**Review:** [`specs/reviews/task-010.md`](../reviews/task-010.md)

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
