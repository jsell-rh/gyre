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

// Build conversation history as a transcript prefix.
// The SDK's query() accepts a single `prompt` (string or AsyncIterable of user
// messages) — it has NO multi-turn `messages` parameter, so prior turns are
// embedded as a transcript block in the prompt. (Passing a `messages` key is
// silently ignored by the SDK, which dropped history entirely.)
let historyBlock = '';
if (history?.length) {
  const lines = [];
  for (const msg of history) {
    // Normalize role label (the transcript only has user/assistant turns)
    const role = msg.role === 'assistant' ? 'Assistant' : 'User';
    lines.push(`${role}: ${msg.content}`);
  }
  historyBlock = `<conversation_history>\n${lines.join('\n\n')}\n</conversation_history>\n\n`;
}
const fullPrompt = `${historyBlock}${userMessage}`;

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
  // Per-invocation turn budget. The server-side self-check loop (spec §11)
  // re-invokes this script for refinement rounds and caps those separately
  // at MAX_REFINEMENT_TURNS (3); this cap bounds the agent's internal
  // tool-use loop within a single invocation.
  maxTurns: 8,
  // Disable session persistence — this is a one-shot subprocess.
  persistSession: false,
};

try {
  let fullText = '';
  // Track what we've already streamed to avoid duplicating content
  let streamedLength = 0;

  for await (const message of query({ prompt: fullPrompt, options })) {
    switch (message.type) {
      case 'assistant': {
        // Complete assistant message with content blocks.
        // Extract text content and emit any not-yet-streamed portions.
        if (message.message?.content) {
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
      // Emit the extracted view query. The SERVER runs the authoritative
      // dry-run self-check (spec §11) on this query — extracting, dry-running,
      // and re-invoking this script with refinement feedback (max 3 rounds) —
      // so no dry-run is performed here.
      console.log(JSON.stringify({ type: 'view_query', query: viewQuery }));
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
