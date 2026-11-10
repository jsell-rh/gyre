---
title: "MCP HSI Parity — Resources and Tools"
spec_ref: "human-system-interface.md §11 (CLI/MCP Parity Constraint)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-010.md
coverage_sections: []
commits: []
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

Authored and reviewed on the `feat/explorer-canvas` branch (R1–R4 in
[specs/reviews/task-010.md](../reviews/task-010.md)):

- `03109ab2a6d0ecc703b488c0c51cfca2adc3a68c` — feat(mcp): add HSI §11 parity resources and tools (TASK-010)
- `0f4e3c1f3c00565bd13175d3ee9f72c2e0f1e479` — fix(mcp): address R1 findings — parity, dead param, edge filter (TASK-010)
- `34efb85149c4d34d8e44cc117a0895aef0376ae6` — fix(mcp): extract shared assemble_concept_results, delegate from MCP handler (TASK-010)
- `40c36207e2a9e2c6f5bd57836598b67a79f9438d` — fix(mcp): briefing since-resolution parity and stale doc comment (TASK-010)

The surface landed on main via the squash merge `d7940e857` (PR #627,
"feat(explorer): explorer canvas, loop tooling, spec coverage tracking").
This round's spec amendment and task bookkeeping are committed on the
assignment branch (see `git log` for this branch's tip).

## Shipped

All five HSI §11 parity surfaces are real production code at the assignment
base — this assignment verified them against the acceptance criteria and
closed the one remaining documented deviation (a spec-table naming drift,
amended via spec lifecycle per the 2026-09-29 §11 coverage audit's
prescription).

### Behavior at base (verified this round)

- `briefing://{workspace_id}` resource (mcp.rs `handle_resource_read`):
  three-step `since` resolution (explicit `?since=` →
  `user_workspace_state.get_last_seen` → 24h fallback), delegates to the
  same `assemble_briefing` the REST handler calls.
- `notifications://{workspace_id}` resource: resolves the user from agent
  JWT auth, honors `?min_priority=`/`?max_priority=`, serializes through
  `NotificationResponse` with the REST envelope (`notifications`, `limit`,
  `offset`).
- `trace://{mr_id}` resource: verifies the MR exists, delegates to
  `assemble_gate_trace` (spans, root_spans, service_graph, commit_sha,
  gate_run_id).
- `graph_concept` tool: requires `concept` + a scope (`repo_id` or
  `workspace_id`), delegates to the shared `assemble_concept_results` used
  by both REST handlers — no hand-built JSON, no predicate divergence.
- `spec_assist` tool: requires `repo_id`/`spec_path`/`instruction`, optional
  `draft_content`; replicates the REST pipeline (per-user/workspace rate
  limit → LLM config check → spec content resolution → graph context →
  prompt template → LLM call → diff validation) and returns the same
  diff/explanation shape.

### This round's change: spec amendment

`specs/system/human-system-interface.md` §11 parity table previously named
the tools `graph.concept`/`spec.assist` (dotted editorial shorthand) while
the shipped MCP surface uses the repo-wide snake_case convention. Renaming
the tools would break the established 25-tool naming convention and
existing consumers, so per task rule 4 and the coverage audit's
prescription, the table was amended to name the shipped tools
(`graph_concept`, `spec_assist`) with a tool-naming note; coverage row 50
records the closure.

### Test evidence

- `cargo test -p gyre-server --lib -- mcp_briefing_resource
  mcp_notifications_resource mcp_trace_resource mcp_graph_concept
  mcp_spec_assist mcp_tools_list` — 20 passed, 0 failed.
- `cargo test -p gyre-server --lib mcp::` — 88 passed, 0 failed.
- Evidence retained in /tmp/stage/review-evidence/task-010-focused-tests.txt
  and task-010-mcp-module-tests.txt.
- `scripts/check-task-commit-attribution.sh` — OK at this branch's tip.
