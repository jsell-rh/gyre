#!/usr/bin/env node
/**
 * Gyre Agent Runner — M25 / M27 / Conversation Provenance (HSI §5)
 *
 * Runs a Claude agent inside the gyre-agent container using the Claude Agent
 * SDK.  Connects back to the Gyre server via MCP so the agent can manage
 * tasks, send heartbeats, and call gyre_agent_complete when done.
 *
 * Conversation provenance:
 *   - Collects all SDK messages into an array, incrementing a turn counter
 *     on each assistant message.
 *   - Configures a PreToolUse hook so that before any `git push`, the git
 *     http.extraHeader is updated with X-Gyre-Conversation-Turn: <turn>.
 *   - After the SDK query completes, serializes the conversation as JSON,
 *     compresses with gzip, computes SHA-256, and uploads via the
 *     conversation.upload MCP tool.
 *   - Passes conversation_sha in the gyre_agent_complete summary.
 *
 * M27: When GYRE_CRED_PROXY is set, Anthropic API calls are routed through
 * the credential proxy via ANTHROPIC_BASE_URL (set by entrypoint.sh).
 * The GYRE_AUTH_TOKEN is still used directly for Gyre API calls as an
 * interim measure (see spec M27.4 for full opacity plan).
 *
 * Vertex AI: When CLAUDE_CODE_USE_VERTEX=1, the SDK uses Vertex AI.
 * For Docker: cred-proxy handles GCE metadata emulation.
 * For local e2e: Vertex credentials come from gcloud ADC (no cred-proxy).
 */

import { query } from '@anthropic-ai/claude-agent-sdk';
import { createHash } from 'node:crypto';
import { gzip } from 'node:zlib';
import { promisify } from 'node:util';
import { writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const gzipAsync = promisify(gzip);

const serverUrl = process.env.GYRE_SERVER_URL;
const token = process.env.GYRE_AUTH_TOKEN;
const taskId = process.env.GYRE_TASK_ID;
const agentId = process.env.GYRE_AGENT_ID;
const branch = process.env.GYRE_BRANCH;
const repoId = process.env.GYRE_REPO_ID;
const credProxy = process.env.GYRE_CRED_PROXY;

if (!serverUrl || !taskId || !agentId || !branch) {
  console.error('ERROR: Required env vars missing (GYRE_SERVER_URL, GYRE_TASK_ID, GYRE_AGENT_ID, GYRE_BRANCH)');
  process.exit(1);
}

// M27: Warn if credentials are unexpectedly present in the environment.
// The entrypoint should have scrubbed GYRE_CRED_* before exec'ing this process.
if (process.env.ANTHROPIC_API_KEY && process.env.ANTHROPIC_API_KEY !== 'proxy-managed') {
  console.warn('[m27] WARNING: ANTHROPIC_API_KEY found in agent env — expected proxy-managed. Credential opacity may be incomplete.');
}

if (credProxy) {
  console.log(`[m27] Credential proxy active at ${credProxy}`);
  console.log(`[m27] Anthropic API calls routed via ANTHROPIC_BASE_URL=${process.env.ANTHROPIC_BASE_URL ?? '(not set)'}`);
}

if (process.env.CLAUDE_CODE_USE_VERTEX === '1') {
  console.log(`[vertex] Vertex AI mode: project=${process.env.ANTHROPIC_VERTEX_PROJECT_ID ?? '(not set)'} region=${process.env.CLOUD_ML_REGION ?? process.env.GYRE_VERTEX_LOCATION ?? '(not set)'}`);
}

const model = process.env.GYRE_AGENT_MODEL || 'claude-sonnet-4-6';

// ── Log posting ─────────────────────────────────────────────────────────────

/** Post a log line to the server so it appears in the agent UI. */
async function postLog(message) {
  try {
    await fetch(`${serverUrl}/api/v1/agents/${agentId}/logs`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify({ message }),
    });
  } catch {
    // Non-fatal — logs are best-effort
  }
}

// ── Conversation provenance state ───────────────────────────────────────────

/** @type {Array<{role: string, type: string, content?: string, name?: string, timestamp: number}>} */
const conversationLog = [];
let turnCounter = 0;

// ── Claude Code hooks for turn-commit linking ───────────────────────────────

/**
 * Write a Claude Code settings file with a PreToolUse hook that injects the
 * conversation turn header into git config before any git push.
 *
 * The hook script runs before each Bash tool call. If the command contains
 * "git push", it updates git's http.extraHeader to include the turn number.
 * This is mechanical — the agent doesn't know about provenance.
 */
function createHooksSettingsFile() {
  // Create a temporary directory for the hooks settings
  const settingsDir = process.env.HOME
    ? join(process.env.HOME, '.gyre-agent')
    : '/tmp/.gyre-agent';
  mkdirSync(settingsDir, { recursive: true });

  const hookScript = join(settingsDir, 'pre-push-hook.sh');
  const settingsFile = join(settingsDir, 'settings.json');

  // The hook script: if the Bash command contains "git push", inject the turn header.
  // We write the current turn counter to a file that the hook reads.
  const turnFile = join(settingsDir, 'current-turn');
  writeFileSync(turnFile, String(turnCounter));

  // Shell script that the hook runs
  writeFileSync(hookScript, `#!/bin/bash
# Gyre provenance hook: inject X-Gyre-Conversation-Turn header before git push
TURN_FILE="${turnFile}"
if [ -f "$TURN_FILE" ]; then
  TURN=$(cat "$TURN_FILE")
  git config --global http.extraHeader "X-Gyre-Conversation-Turn: $TURN"
fi
`, { mode: 0o755 });

  // Settings JSON for Claude Code hooks
  const settings = {
    hooks: {
      PreToolUse: [
        {
          matcher: "Bash",
          hooks: [
            {
              type: "command",
              command: hookScript,
            }
          ]
        }
      ]
    }
  };

  writeFileSync(settingsFile, JSON.stringify(settings, null, 2));
  return { settingsFile, turnFile };
}

/**
 * Update the turn counter file so the hook picks up the latest turn number.
 */
function updateTurnFile(turnFile) {
  try {
    writeFileSync(turnFile, String(turnCounter));
  } catch {
    // Non-fatal — provenance linking degrades gracefully
  }
}

// ── Conversation upload ─────────────────────────────────────────────────────

/**
 * Serialize the conversation log, compress with gzip, compute SHA-256,
 * and upload to the server via the conversation.upload MCP tool.
 *
 * @returns {Promise<string|null>} The SHA-256 of the compressed blob, or null on failure.
 */
async function uploadConversation() {
  if (conversationLog.length === 0) {
    console.log('[provenance] No conversation messages to upload');
    return null;
  }

  try {
    // Serialize to JSON
    const jsonStr = JSON.stringify({
      agent_id: agentId,
      task_id: taskId,
      branch,
      model,
      total_turns: turnCounter,
      messages: conversationLog,
    });

    // Compress with gzip
    const compressed = await gzipAsync(Buffer.from(jsonStr, 'utf-8'));

    // Compute SHA-256 of compressed bytes
    const sha256 = createHash('sha256').update(compressed).digest('hex');

    // Base64 encode for MCP tool (JSON transport)
    const blob = compressed.toString('base64');

    // Check size limit (10MB before base64)
    const maxBytes = parseInt(process.env.GYRE_MAX_CONVERSATION_SIZE || '10485760', 10);
    if (compressed.length > maxBytes) {
      console.warn(`[provenance] Conversation too large (${compressed.length} bytes > ${maxBytes}). Skipping upload.`);
      return null;
    }

    // Upload via direct POST to MCP (conversation.upload tool)
    const mcpReq = {
      jsonrpc: '2.0',
      id: 'conv-upload-1',
      method: 'tools/call',
      params: {
        name: 'conversation_upload',
        arguments: {
          blob,
          conversation_sha: sha256,
        },
      },
    };

    const resp = await fetch(`${serverUrl}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(mcpReq),
    });

    if (!resp.ok) {
      console.warn(`[provenance] conversation.upload HTTP ${resp.status}`);
      return null;
    }

    const result = await resp.json();
    if (result?.result?.isError) {
      console.warn(`[provenance] conversation.upload error: ${result.result.content?.[0]?.text}`);
      return null;
    }

    console.log(`[provenance] Conversation uploaded: sha=${sha256}, size=${compressed.length} bytes, turns=${turnCounter}`);
    return sha256;
  } catch (e) {
    console.warn(`[provenance] conversation upload failed (non-fatal): ${e.message}`);
    return null;
  }
}

// ── MCP configuration ───────────────────────────────────────────────────────

const options = {
  model,
  mcpServers: {
    gyre: {
      type: 'http',
      url: `${serverUrl}/mcp`,
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    },
  },
  allowedTools: [
    'Read', 'Write', 'Edit', 'Bash', 'Glob', 'Grep',
    'mcp__gyre__gyre_list_tasks', 'mcp__gyre__gyre_update_task',
    'mcp__gyre__gyre_agent_heartbeat', 'mcp__gyre__gyre_record_activity',
    'mcp__gyre__gyre_search', 'mcp__gyre__gyre_create_task',
    // gyre_agent_complete is NOT listed — the runner calls it after conversation upload
  ],
  permissionMode: 'acceptEdits',
};

const taskPrompt = process.env.GYRE_TASK_PROMPT ||
  `You are a Gyre autonomous agent. Your configuration:
- Agent ID: ${agentId}
- Task ID: ${taskId}
- Branch: ${branch}
- Repo ID: ${repoId || 'unknown'}
- Working directory: ${process.env.GYRE_WORK_DIR || process.cwd()}

You have been spawned to complete the task assigned to you. Your working directory
contains a checked-out git clone of the repository on your branch.

Instructions:
1. Use \`gyre_list_tasks\` to read your assigned task (id: ${taskId}) for full details.
2. Implement the task requirements by editing files in ${process.env.GYRE_WORK_DIR || process.cwd()}.
3. Commit your changes with a descriptive conventional-commit message.
4. Push your changes: \`git push origin ${branch}\`

Do NOT call gyre_agent_complete — the runner handles completion after uploading
conversation provenance. Just implement, commit, and push.

Use \`gyre_agent_heartbeat\` periodically to signal liveness.

Begin by reading your task description, then implement it completely.`;

// ── Main execution ──────────────────────────────────────────────────────────

console.log(`=== Gyre Agent Runner starting ===`);
console.log(`Agent: ${agentId} | Task: ${taskId} | Branch: ${branch}`);
console.log(`Model: ${model} | Server: ${serverUrl}`);
postLog(`Agent runner starting: model=${model}, branch=${branch}`);

// Set up provenance hooks
const { turnFile } = createHooksSettingsFile();

let messageCount = 0;
let lastHeartbeat = Date.now();
const HEARTBEAT_INTERVAL_MS = 15_000; // Must be < server's 60s stale timeout
let conversationSha = null;

try {
  for await (const message of query({ prompt: taskPrompt, options })) {
    messageCount++;

    // Record message for provenance
    const logEntry = {
      role: message.type === 'text' ? 'assistant' : message.type,
      type: message.type,
      timestamp: Date.now(),
    };

    if (message.type === 'text') {
      logEntry.content = message.content;
      process.stdout.write(message.content);
      postLog(`[assistant] ${message.content.slice(0, 500)}`);
    } else if (message.type === 'tool_use') {
      logEntry.name = message.name;
      logEntry.content = typeof message.input === 'string'
        ? message.input
        : JSON.stringify(message.input);
      console.log(`[tool] ${message.name}`);
      postLog(`[tool_use] ${message.name}`);
    } else if (message.type === 'tool_result') {
      // Truncate large tool results in provenance log (keep first 2KB)
      const resultText = typeof message.content === 'string'
        ? message.content
        : JSON.stringify(message.content);
      logEntry.content = resultText.length > 2048
        ? resultText.slice(0, 2048) + '...(truncated)'
        : resultText;
      if (messageCount % 10 === 0) {
        console.log(`[progress] ${messageCount} messages processed`);
        postLog(`[progress] ${messageCount} messages processed`);
      }
    }

    // Increment turn counter on assistant text messages
    if (message.type === 'text') {
      turnCounter++;
      updateTurnFile(turnFile);
    }

    conversationLog.push(logEntry);

    // Periodic heartbeat using GYRE_AUTH_TOKEN (interim; see M27.4 for full proxy plan)
    const now = Date.now();
    if (now - lastHeartbeat > HEARTBEAT_INTERVAL_MS) {
      lastHeartbeat = now;
      try {
        const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {
          method: 'PUT',
          headers: {
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({ pid: process.pid }),
        });
        if (!resp.ok) console.warn(`[heartbeat] failed: ${resp.status}`);
      } catch (e) {
        console.warn(`[heartbeat] error: ${e.message}`);
      }
    }
  }

  // ── Post-query: upload conversation, then signal completion ─────────────

  console.log(`[provenance] Uploading conversation (${conversationLog.length} messages, ${turnCounter} turns)...`);
  conversationSha = await uploadConversation();
  if (conversationSha) {
    console.log(`[provenance] Conversation SHA: ${conversationSha}`);
  }

  // Signal completion via REST API (not MCP — we need the token still valid)
  console.log(`[complete] Signaling agent completion...`);
  try {
    const completeResp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/complete`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        branch,
        title: `feat: implement task via autonomous agent`,
        target_branch: 'main',
        conversation_sha: conversationSha,
      }),
    });
    if (completeResp.ok) {
      const mr = await completeResp.json();
      console.log(`[complete] MR created: ${mr.id}`);
    } else {
      console.warn(`[complete] HTTP ${completeResp.status}: ${await completeResp.text()}`);
    }
  } catch (e) {
    console.warn(`[complete] failed: ${e.message}`);
  }

  console.log(`=== Agent runner complete (${messageCount} messages, ${turnCounter} turns) ===`);
  postLog(`Agent complete: ${messageCount} messages, ${turnCounter} turns, conversation_sha=${conversationSha || 'none'}`);
} catch (err) {
  // Best-effort: upload whatever conversation we have even on error
  console.log(`[provenance] Uploading partial conversation after error...`);
  conversationSha = await uploadConversation();

  // Still try to complete
  try {
    await fetch(`${serverUrl}/api/v1/agents/${agentId}/complete`, {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ branch, title: `feat: implement task (error recovery)`, target_branch: 'main' }),
    });
  } catch (_) { /* best effort */ }

  console.error(`=== Agent runner error: ${err.message} ===`);
  process.exit(1);
}
