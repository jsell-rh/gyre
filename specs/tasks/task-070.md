---
title: "LLM Explorer Agent — Configuration, System Prompt & Self-Check Loop"
spec_ref: "explorer-implementation.md §7–8, §10–11"
depends_on:
  - task-068
  - task-069
progress: not-started
coverage_sections:
  - "explorer-implementation.md §7 LLM Agent (Claude Agent SDK)"
  - "explorer-implementation.md §8 Agent Configuration"
  - "explorer-implementation.md §10 Agent System Prompt"
  - "explorer-implementation.md §11 Self-Check Loop"
commits: []
---

## Spec Excerpt

**§7 LLM Agent (Claude Agent SDK):** The explorer agent runs via `query()` from `@anthropic-ai/claude-agent-sdk`. It is NOT the same agent that writes code — it's a read-only analysis agent with access to the knowledge graph.

**§8 Agent Configuration:**
```javascript
const options = {
  model: process.env.GYRE_LLM_MODEL || 'claude-sonnet-4-6',
  mcpServers: {
    gyre: {
      type: 'http',
      url: `${serverUrl}/mcp`,
      headers: { Authorization: `Bearer ${token}` },
    },
  },
  allowedTools: [
    'mcp__gyre__graph_summary',
    'mcp__gyre__graph_query_dryrun',
    'mcp__gyre__graph_nodes',
    'mcp__gyre__graph_edges',
    'mcp__gyre__search',
  ],
};
```

**§10 Agent System Prompt:** The agent helps users understand their codebase by generating view queries. Workflow:
1. Call graph_summary to understand the codebase
2. Reason about relevant nodes/edges
3. Generate a view query JSON
4. Call graph_query_dryrun to check the query
5. If warnings, refine (max 3 refinements)
6. Output final view query in `<view_query>` block
7. Provide text explanation

**§11 Self-Check Loop:** The server extracts `<view_query>` blocks from agent output, runs dry-run, feeds result back. Agent can refine up to 3 turns. If not resolved by turn 3, send the best version anyway.

## Implementation Plan

### Existing Code

- `crates/gyre-server/src/explorer_ws.rs` — already implements the agent loop with Claude Agent SDK subprocess and native LLM port fallback. Contains tool-call handling for graph_summary, graph_query_dryrun, etc.
- Check for `scripts/explorer-agent.mjs` — the Claude Agent SDK subprocess script.

### Work Required

1. **Audit Claude Agent SDK integration**: Verify the `scripts/explorer-agent.mjs` script (or equivalent) uses `query()` from `@anthropic-ai/claude-agent-sdk` with the correct configuration from §8.

2. **Verify model configuration**: Default model should be `claude-sonnet-4-6` (overridable via `GYRE_LLM_MODEL` env var).

3. **Verify MCP server configuration**: The agent should connect to the Gyre server's MCP endpoint (`/mcp`) with Bearer token auth. Verify `allowedTools` matches the 5 tools from §8.

4. **Audit system prompt**: Verify the agent's system prompt matches §10 — it should instruct the agent to call graph_summary first, generate view queries, use `<view_query>` blocks, and self-check via dry-run.

5. **Self-check loop**: Verify the server:
   - Extracts `<view_query>` JSON from agent output
   - Runs dry-run on extracted query
   - If warnings exist and refinement count < 3, feeds dry-run result back to agent
   - If refinement count >= 3, sends best version to frontend anyway
   - Sends `status: "refining"` during refinement turns

6. **Fallback path**: Verify the native LLM port fallback (when Claude Agent SDK is unavailable) implements the same self-check loop with the same tool definitions.

7. **Conversation history**: Verify the agent maintains conversation context across messages in the same WebSocket session (previous Q&A pairs are included in subsequent prompts).

## Acceptance Criteria

- [ ] Claude Agent SDK subprocess script exists and uses `query()` with correct options
- [ ] Default model: `claude-sonnet-4-6`, overridable via `GYRE_LLM_MODEL`
- [ ] Agent connects to MCP at `{serverUrl}/mcp` with Bearer auth
- [ ] `allowedTools` includes all 5 MCP tools from §8
- [ ] System prompt matches §10 workflow (graph_summary → reason → generate → dry-run → refine → output)
- [ ] `<view_query>` blocks are extracted from agent output and parsed as JSON
- [ ] Self-check loop runs dry-run on extracted queries
- [ ] Refinement capped at 3 turns; best version sent if cap reached
- [ ] `status: "refining"` sent during each refinement turn
- [ ] Conversation history maintained within WebSocket session
- [ ] Native LLM port fallback implements same loop
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/explorer-implementation.md` §7–8, §10–11. Then audit:
- `crates/gyre-server/src/explorer_ws.rs` — the agent loop implementation. Look for:
  - Claude Agent SDK subprocess spawning
  - System prompt construction
  - `<view_query>` extraction regex/parser
  - Dry-run invocation during self-check
  - Refinement counter and cap
  - Conversation history management
- `scripts/explorer-agent.mjs` — if it exists, audit against §8 configuration
- Environment variables: `GYRE_LLM_MODEL`, `GYRE_EXPLORER_SDK`

The handler is 4271 lines and already implements most of this. Focus on verifying the self-check loop logic (extract → dry-run → refine → cap at 3) and that the system prompt matches the spec.
