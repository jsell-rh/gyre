---
title: "LLM Explorer Agent — Configuration, System Prompt & Self-Check Loop"
spec_ref: "explorer-implementation.md §7–8, §10–11"
depends_on:
  - task-068
  - task-069
progress: ready-for-review
coverage_sections:
  - "explorer-implementation.md §7 LLM Agent (Claude Agent SDK)"
  - "explorer-implementation.md §8 Agent Configuration"
  - "explorer-implementation.md §10 Agent System Prompt"
  - "explorer-implementation.md §11 Self-Check Loop"
commits: ["a58467b4ca6bc60ff14a4559cf95c630f6b33bf6", "a885c918cb40730343494ff750ea4b68fa6dd455", "7ea40e77363c78bd4e1085dd43be8a2ba1ad4d3d"]
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

## Shipped

Audit + gap-fill against spec §7–8, §10–11. Both agent paths were
verified against every acceptance criterion; one real defect was found
and fixed.

### Claude Agent SDK path (§7–8)

`scripts/explorer-agent.mjs` imports `query()` from
`@anthropic-ai/claude-agent-sdk` and runs as a headless one-shot
subprocess (JSON in on stdin, JSON-lines out on stdout). Options match
§8: model resolved as request model → `GYRE_LLM_MODEL` →
`'claude-sonnet-4-6'`; `mcpServers.gyre` is `type: 'http'` at
`${server_url}/mcp` with `Authorization: Bearer ${token}`; built-in
tools disabled (`tools: []`) so the agent is read-only analysis only;
`allowedTools` covers all five §8 MCP tools (`mcp__gyre__graph_summary`,
`graph_query_dryrun`, `graph_nodes`, `graph_edges`, `search`) plus the
read-only `node_provenance` tool documented in the system prompt. On
the server side, model resolution goes workspace/tenant LLM config →
`GYRE_LLM_MODEL` → `EXPLORER_SDK_DEFAULT_MODEL = "claude-sonnet-4-6"`
(via `resolve_llm_model_with_default`, new in llm_helpers.rs — the
native Vertex fallback keeps the platform gemini default since claude
model names are not valid there). The SDK path is admin-gated (it uses
the server master token) and opted out with `GYRE_EXPLORER_SDK=0`.

### System prompt (§10)

`build_system_prompt_body()` carries the full §10 workflow (summary →
reason → generate → dry-run → refine → `<view_query>` output →
text explanation), the View Query Grammar, grounding rules, and output
format. `__WORKFLOW_STEP_1__` is substituted per path: the SDK variant
(`build_system_prompt_for_sdk`) instructs "Call graph_summary to
understand the codebase structure" (spec §10 step 1 verbatim — nothing
is pre-loaded); the native variant notes repo context is pre-loaded.

### Self-check loop (§11)

Both paths share the same machinery: `parse_view_query_from_text()` /
the script's regex extract the LAST `<view_query>` block and parse it
as JSON; `self_check_dry_run()` deserializes → validates → dry-runs
against the real graph; `actionable_dry_run_warnings()` classifies
which warnings a refinement can fix (info/data-source-inherent
warnings never trigger refinement). On actionable warnings with budget
remaining, `status: "refining"` is sent and the dry-run result is fed
back — as a synthetic tool_use/tool_result pair in the native LlmPort
loop, as the next refinement-round question in the SDK loop.
`MAX_REFINEMENT_TURNS = 3` caps both; at the cap the best version is
sent to the frontend anyway (logged). Structurally unparseable queries
are never forwarded — the native path rejects with a visible warning,
the SDK path spends refinement budget asking for a corrected block
before rejecting. Conversation history is maintained per WebSocket
session in both paths (native: full tool-use blocks; SDK: serialized
into a `<conversation_history>` transcript prefix because `query()`
has no multi-turn `messages` parameter).

### Defect fixed (round 2)

`run_sdk_subprocess_once` re-resolved `GYRE_EXPLORER_SDK_PATH` with a
relative default (`"scripts/explorer-agent.mjs"`) that the child node
process resolves against the server's CWD — diverging from the path
the dispatch site validated (exe-dir-relative in installed layouts).
The dispatch site's canonicalized, validation-passing path is now
threaded through `run_explorer_agent_sdk` into the spawn, so execution
cannot diverge from validation.

### Verification repair (round 4)

Branch lineage: the task work (`a58467b4`, `a885c918`) was carried
through two base merges (round 3 onto `ba5ebe10`, then `4e304e5d` —
merge `29915552`); all Rust/web-source merges were clean, `web/dist`
was taken wholesale from the incoming base's committed build, and a
fresh `npm ci && npm run build` of the merged sources reproduced the
committed `web/dist` byte-for-byte.

The deterministic gates flagged the round-2 changes: rustfmt on the
new SDK self-check code (explorer_ws.rs, llm_helpers.rs), plus four
clippy lints — fixed in `7ea40e77`. The two `Message::Text(
serialize_msg(...).unwrap_or_default().into())` view-query forwards
added by this task dropped the useless `String`→`String` `.into()`
(axum 0.7 `Message::Text` takes `String`); `run_sdk_subprocess_once`
now takes `&[ConversationMessage]` instead of `&mut Vec<_>` (it only
serializes history — pushes stay in the caller); both SDK runner
functions carry `#[allow(clippy::too_many_arguments)]` per the
repo-wide convention; both files are rustfmt-clean. No behavior
change: the same 58 lib + 9 integration explorer tests pass at
`7ea40e77`.

### Test evidence

- `cargo test -p gyre-server --lib explorer` — 58 passed at `7ea40e77`
  (view-query parsing incl. malformed/unclosed/multiple-blocks/last-wins,
  system prompt content, status protocol values, turn budgets, default
  views).
- `cargo test -p gyre-server --test explorer_ws_integration` — 9 passed
  at `7ea40e77` (session lifecycle, view CRUD + validation, rate/session
  limits, status progression asserting on-wire statuses are exactly
  `thinking|refining|ready` and that a view_query arrives).
- `scripts/check-relative-path-defaults.sh` — OK after the round-2
  path fix.
- Diff-scoped clippy replication (check-clippy-diff.py logic against
  base `4e304e5d`): 0 findings on changed lines at `7ea40e77`;
  `rustfmt --check` clean on both touched files.
- Probe outputs retained under `/tmp/stage/review-evidence/`.
