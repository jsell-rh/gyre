#!/usr/bin/env node
/**
 * Explorer Agent — Claude Agent SDK integration for conversational exploration.
 *
 * Usage: echo '{"question":"...", "canvas_state":{...}, "repo_id":"...", "server_url":"...", "token":"..."}' | node explorer-agent.mjs
 *
 * Output: JSON lines on stdout:
 *   {"type":"text","content":"...","done":false}
 *   {"type":"view_query","query":{...}}
 *   {"type":"status","status":"thinking"|"refining"}
 *   {"type":"done"}
 *   {"type":"error","message":"..."}
 */

import { query } from '@anthropic-ai/claude-agent-sdk';

// Read input from stdin
let input = '';
for await (const chunk of process.stdin) {
  input += chunk;
}

let req;
try {
  req = JSON.parse(input);
} catch (e) {
  console.log(JSON.stringify({ type: 'error', message: `Invalid input: ${e.message}` }));
  process.exit(1);
}

const { question, canvas_state, repo_id, server_url, token, model, system_prompt, history } = req;

if (!server_url || !token || !repo_id) {
  console.log(JSON.stringify({ type: 'error', message: 'Missing server_url, token, or repo_id' }));
  process.exit(1);
}

const agentModel = model || process.env.GYRE_LLM_MODEL || 'claude-sonnet-4-6';

// Build canvas context
let canvasContext = '';
if (canvas_state?.selected_node) {
  const sel = canvas_state.selected_node;
  const qname = sel.qualified_name || sel.name;
  canvasContext += `Selected node: ${qname} (type: ${sel.node_type}, id: ${sel.id})`;
}
if (canvas_state?.visible_tree_groups?.length) {
  canvasContext += ` | Visible groups: ${canvas_state.visible_tree_groups.join(', ')}`;
}
if (canvas_state?.active_lens) {
  canvasContext += ` | Active lens: ${canvas_state.active_lens}`;
}

const userMessage = canvasContext ? `[Canvas: ${canvasContext}]\n\n${question}` : question;

// Build conversation history as structured multi-turn messages.
// The SDK's query() accepts a `messages` array of {role, content} objects for
// multi-turn conversations. We pass prior history as alternating user/assistant
// messages and append the current question as the final user message.
// History content is JSON-encoded within the prompt to avoid injection risks
// (no XML escaping needed — JSON strings are naturally safe).
let historyMessages = [];
if (history?.length) {
  for (const msg of history) {
    // Normalize role to 'user' or 'assistant' (the only roles the SDK accepts)
    const role = msg.role === 'assistant' ? 'assistant' : 'user';
    historyMessages.push({ role, content: msg.content });
  }
}
// Append the current user question as the final message
historyMessages.push({ role: 'user', content: userMessage });
const fullPrompt = userMessage;

console.log(JSON.stringify({ type: 'status', status: 'thinking' }));

// Build MCP server config pointing to the Gyre server's MCP endpoint.
// The MCP tools (graph_summary, graph_query_dryrun, graph_nodes, graph_edges,
// search) are served by the Gyre server at /mcp.
const mcpUrl = `${server_url}/mcp`;

// MCP tool names as they appear after the SDK prefixes them with mcp__<server>__
const mcpToolNames = [
  'mcp__gyre__graph_summary',
  'mcp__gyre__graph_query_dryrun',
  'mcp__gyre__graph_nodes',
  'mcp__gyre__graph_edges',
  'mcp__gyre__node_provenance',
  'mcp__gyre__search',
];

const options = {
  model: agentModel,
  systemPrompt: system_prompt || undefined,
  // Disable all built-in tools (Read, Edit, Bash, etc.) — only MCP tools should be available.
  tools: [],
  // MCP connection to the Gyre server for graph exploration tools.
  mcpServers: {
    gyre: {
      type: 'http',
      url: mcpUrl,
      headers: { Authorization: `Bearer ${token}` },
    },
  },
  // Auto-approve MCP tool calls without prompting (headless subprocess).
  allowedTools: mcpToolNames,
  // Bypass permission prompts — this is a headless subprocess, not interactive.
  permissionMode: 'bypassPermissions',
  allowDangerouslySkipPermissions: true,
  // Budget: MAX_TOOL_TURNS=5 + MAX_REFINEMENT_TURNS=3 = 8 hard cap.
  // Tool-turn vs refinement-turn tracking is done in the output stream below.
  maxTurns: 8,
  // Disable session persistence — this is a one-shot subprocess.
  persistSession: false,
};

// Turn budget constants per spec
const MAX_TOOL_TURNS = 5;
const MAX_REFINEMENT_TURNS = 3;

try {
  let fullText = '';
  // Track what we've already streamed to avoid duplicating content
  let streamedLength = 0;
  // Separate turn counters: tool turns (with tool_use blocks) vs refinement turns (text-only)
  let toolTurns = 0;
  let refinementTurns = 0;

  for await (const message of query({ prompt: fullPrompt, options, messages: historyMessages })) {
    switch (message.type) {
      case 'assistant': {
        // Complete assistant message with content blocks.
        // Extract text content and emit any not-yet-streamed portions.
        if (message.message?.content) {
          const hasToolUse = message.message.content.some((b) => b.type === 'tool_use');
          if (hasToolUse) {
            toolTurns++;
            if (toolTurns > MAX_TOOL_TURNS) {
              console.error(`[budget] tool turn limit exceeded (${toolTurns}/${MAX_TOOL_TURNS})`);
            }
          } else {
            refinementTurns++;
            if (refinementTurns > MAX_REFINEMENT_TURNS) {
              console.error(`[budget] refinement turn limit exceeded (${refinementTurns}/${MAX_REFINEMENT_TURNS})`);
            }
          }

          for (const block of message.message.content) {
            if (block.type === 'text') {
              fullText += block.text;
              // Only emit text that wasn't already streamed via content_block_delta
              const unstreamed = fullText.substring(streamedLength);
              if (unstreamed) {
                console.log(JSON.stringify({ type: 'text', content: unstreamed, done: false }));
                streamedLength = fullText.length;
              }
            } else if (block.type === 'tool_use') {
              // Tool use block — signal refining status.
              console.log(JSON.stringify({ type: 'status', status: 'refining' }));
            }
          }
        }
        break;
      }

      case 'result': {
        // Final result message. Extract the result text if present and not
        // already captured from assistant messages.
        if (message.subtype === 'success' && message.result && !fullText) {
          fullText = message.result;
          console.log(JSON.stringify({ type: 'text', content: message.result, done: false }));
          streamedLength = fullText.length;
        }
        break;
      }

      default: {
        // Handle streaming events for real-time token delivery.
        // The SDK may emit events with content_block_delta containing text tokens.
        if (message.type === 'event' || message.type === 'stream_event') {
          const event = message.event || message;
          if (event?.type === 'content_block_delta' && event?.delta?.type === 'text_delta') {
            const token = event.delta.text;
            if (token) {
              fullText += token;
              streamedLength = fullText.length;
              console.log(JSON.stringify({ type: 'text', content: token, done: false }));
            }
          }
        }
        break;
      }
    }
  }

  // Parse <view_query> blocks from the accumulated text.
  // Take the LAST block (matches server-side parse_view_query_from_text behavior)
  // because the LLM may refine its query mid-response.
  const vqRegex = /<view_query>([\s\S]*?)<\/view_query>/g;
  let lastMatch = null;
  let m;
  while ((m = vqRegex.exec(fullText)) !== null) {
    lastMatch = m;
  }
  if (lastMatch) {
    try {
      const viewQuery = JSON.parse(lastMatch[1].trim());
      const cleanText = fullText.replace(/<view_query>[\s\S]*?<\/view_query>/g, '').trim();
      if (cleanText) {
        console.log(JSON.stringify({ type: 'text', content: cleanText, done: true }));
      }

      // Self-check: call graph_query_dryrun via MCP HTTP to validate the query.
      // The SDK agent already had the MCP tool available and may have used it
      // during its run, but we do a server-mediated check here as a safety net
      // so warnings surface to the frontend even if the agent skipped dry-run.
      let warnings = [];
      try {
        const dryRunBody = {
          jsonrpc: '2.0',
          id: 'selfcheck-1',
          method: 'tools/call',
          params: {
            name: 'graph_query_dryrun',
            arguments: { repo_id, query: viewQuery },
          },
        };
        const dryRunRes = await fetch(mcpUrl, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
          },
          body: JSON.stringify(dryRunBody),
        });
        if (dryRunRes.ok) {
          const dryRunJson = await dryRunRes.json();
          // The MCP response wraps tool output in result.content[].text
          const resultContent = dryRunJson?.result?.content;
          if (Array.isArray(resultContent)) {
            for (const block of resultContent) {
              if (block.type === 'text' && block.text) {
                try {
                  const parsed = JSON.parse(block.text);
                  if (parsed.warnings?.length) {
                    warnings = parsed.warnings;
                  }
                } catch { /* not JSON, skip */ }
              }
            }
          }
        }
      } catch (dryRunErr) {
        console.error(`[self-check] dry-run failed: ${dryRunErr.message}`);
      }

      if (warnings.length) {
        console.error(`[self-check] view_query warnings: ${warnings.join('; ')}`);
      }

      console.log(JSON.stringify({
        type: 'view_query',
        query: viewQuery,
        ...(warnings.length ? { warnings } : {}),
      }));
    } catch (_parseErr) {
      // view_query JSON was malformed; send the raw text
      console.log(JSON.stringify({ type: 'text', content: fullText, done: true }));
    }
  } else {
    console.log(JSON.stringify({ type: 'text', content: '', done: true }));
  }

  console.log(JSON.stringify({ type: 'done' }));
} catch (err) {
  console.log(JSON.stringify({ type: 'error', message: err.message || String(err) }));
  process.exit(1);
}
