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

// Build the full prompt including history context
let promptParts = [];
if (history?.length) {
  for (const msg of history) {
    promptParts.push(`[${msg.role}]: ${msg.content}`);
  }
}
promptParts.push(userMessage);
const fullPrompt = promptParts.join('\n\n');

console.log(JSON.stringify({ type: 'status', status: 'thinking' }));

// Build MCP server config for the specific repo's tools
const mcpUrl = `${server_url}/mcp`;

const options = {
  model: agentModel,
  systemPrompt: system_prompt,
  mcpServers: {
    gyre: {
      type: 'http',
      url: mcpUrl,
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
  maxTurns: 9,
};

try {
  let fullText = '';

  for await (const message of query({ prompt: fullPrompt, options })) {
    if (message.type === 'text') {
      console.log(JSON.stringify({ type: 'text', content: message.content, done: false }));
      fullText += message.content;
    } else if (message.type === 'tool_use') {
      console.log(JSON.stringify({ type: 'status', status: 'refining' }));
    }
    // tool_result messages are internal — no need to forward
  }

  // Parse <view_query> blocks from the accumulated text
  const vqMatch = fullText.match(/<view_query>([\s\S]*?)<\/view_query>/);
  if (vqMatch) {
    try {
      const viewQuery = JSON.parse(vqMatch[1].trim());
      const cleanText = fullText.replace(/<view_query>[\s\S]*?<\/view_query>/g, '').trim();
      if (cleanText) {
        console.log(JSON.stringify({ type: 'text', content: cleanText, done: true }));
      }
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
