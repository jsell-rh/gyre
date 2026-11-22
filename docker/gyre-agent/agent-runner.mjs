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

 * Preview mode (meta-spec reconciliation §5): when GYRE_PREVIEW_ID is set and no
 * GYRE_TASK_PROMPT override is provided, the agent implements a real spec under a
 * draft meta-spec.  There is no task — the task-mutating MCP tools are withheld, the
 * conversation is not uploaded, and completion is never signalled.  The server derives
 * the result from the pushed preview branch, then kills the agent and deletes it.
 *
 * Vertex AI: When CLAUDE_CODE_USE_VERTEX=1, the SDK uses Vertex AI.
 * For Docker: cred-proxy handles GCE metadata emulation.
 * For local e2e: Vertex credentials come from gcloud ADC (no cred-proxy).
 */

import { query } from '@anthropic-ai/claude-agent-sdk';
import { createHash } from 'node:crypto';
import { gzip } from 'node:zlib';
import { promisify } from 'node:util';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { spawn as spawnChild } from 'node:child_process';
import { join } from 'node:path';

const gzipAsync = promisify(gzip);

const serverUrl = process.env.GYRE_SERVER_URL;
const token = process.env.GYRE_AUTH_TOKEN;
const taskId = process.env.GYRE_TASK_ID;
const previewId = process.env.GYRE_PREVIEW_ID;
// Preview mode is identified by GYRE_PREVIEW_ID; an explicit GYRE_TASK_PROMPT is an
// operator override and puts the runner back on the normal (task) path.
const previewMode = !!previewId && !process.env.GYRE_TASK_PROMPT;
const agentId = process.env.GYRE_AGENT_ID;
const branch = process.env.GYRE_BRANCH;
const repoId = process.env.GYRE_REPO_ID;
const credProxy = process.env.GYRE_CRED_PROXY;

// GYRE_TASK_ID is required in normal mode; a preview agent has no task and carries
// GYRE_PREVIEW_ID in its place (meta-spec reconciliation §5).
if (!serverUrl || !agentId || !branch || !(taskId || previewId)) {
  console.error('ERROR: Required env vars missing (GYRE_SERVER_URL, GYRE_AGENT_ID, GYRE_BRANCH and GYRE_TASK_ID or GYRE_PREVIEW_ID)');
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
  // Preview agents have no task: the task-mutating MCP tools are withheld so they can't
  // touch the task ledger.  Everything else is identical to a normal run.
  allowedTools: previewMode
    ? [
        'Read', 'Write', 'Edit', 'Bash', 'Glob', 'Grep',
        'mcp__gyre__gyre_agent_heartbeat', 'mcp__gyre__gyre_record_activity',
        'mcp__gyre__gyre_search',
        // No task tools, and gyre_agent_complete is never called in preview mode
      ]
    : [
        'Read', 'Write', 'Edit', 'Bash', 'Glob', 'Grep',
        'mcp__gyre__gyre_list_tasks', 'mcp__gyre__gyre_update_task',
        'mcp__gyre__gyre_agent_heartbeat', 'mcp__gyre__gyre_record_activity',
        'mcp__gyre__gyre_search', 'mcp__gyre__gyre_create_task',
        // gyre_agent_complete is NOT listed — the runner calls it after conversation upload
      ],
  permissionMode: 'acceptEdits',
};

const defaultPrompt =
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

The runner mechanically verifies your work: after you finish, it runs the
repository's locally-configured quality gates (GYRE_LOCAL_GATES) in your
working directory BEFORE completion. If they fail you will receive the
failing gate's name and captured output — fix the failures, commit, and push
again. No merge request is created until every gate passes.

Do NOT call gyre_agent_complete — the runner handles completion after uploading
conversation provenance. Just implement, commit, and push.

Use \`gyre_agent_heartbeat\` periodically to signal liveness.

Begin by reading your task description, then implement it completely.`;

const draftKind = process.env.GYRE_META_SPEC_DRAFT_KIND || 'unknown';
const draftContent = process.env.GYRE_META_SPEC_DRAFT_CONTENT || 'unknown';
const targetSpecPath = process.env.GYRE_TARGET_SPEC_PATH || 'unknown';

const previewPrompt =
  `You are a Gyre autonomous agent running a THROWAWAY preview of a draft meta-spec. Your configuration:
- Agent ID: ${agentId}
- Preview ID: ${previewId}
- Branch: ${branch}
- Repo ID: ${repoId || 'unknown'}
- Working directory: ${process.env.GYRE_WORK_DIR || process.cwd()}
- Draft meta-spec kind: ${draftKind}
- Target spec: ${targetSpecPath}

You have been spawned so a human can see what the draft meta-spec below actually produces
against a real spec in a real repo. Your working directory contains a checked-out git clone
of the repository on your branch. The draft is unapproved, has no SHA, and is committed
nowhere — nothing you produce here will ever be merged.

Draft meta-spec (${draftKind}):
${draftContent}

Instructions:
1. Read \`${targetSpecPath}\` in ${process.env.GYRE_WORK_DIR || process.cwd()} — that is the spec you must implement.
2. Implement that spec exactly as the draft meta-spec above instructs. Where the draft and
   your usual conventions disagree, the draft wins.
3. Commit your changes with a descriptive conventional-commit message.
4. Push your changes: \`git push origin ${branch}\`

There is no task, no review and no merge request on this branch:
- Do NOT call gyre_agent_complete — the runner ends this run.
- Do NOT create or update tasks — you have no task and must not touch the task ledger.
- Do NOT expect a review or merge request. The push above is the entire deliverable, and
  the branch is deleted once a human has read the diff.

Use \`gyre_agent_heartbeat\` periodically to signal liveness.

Begin by reading ${targetSpecPath}, then implement it completely.`;

const taskPrompt = process.env.GYRE_TASK_PROMPT || (previewMode ? previewPrompt : defaultPrompt);

// ── Meta-spec system prompt (agent-runtime spec §2) ─────────────────────────
//
// The server assembles the agent's meta-spec prompt set (required tenant →
// required workspace → spec-level bindings at pinned versions) at spawn time
// and injects it as GYRE_META_SPEC_PROMPT. Prepend it to the task prompt so
// the behavioral instructions lead the context the LLM receives. Without this
// hop the registry is stored and attested but never actually shapes agent
// behavior — "the meta-spec registry IS the prompt configuration".
// Preview agents get no assembled set — their governing context is the draft
// meta-spec delivered via GYRE_META_SPEC_DRAFT_* in the preview prompt.
const metaSpecPrompt = (process.env.GYRE_META_SPEC_PROMPT || '').trim();
// agent-runtime.md §5 / task-268 (4): the PROTOCOL section is
// system-managed, injected at spawn as GYRE_PROTOCOL_PROMPT. Include it in
// the LLM query prompt so the protocol's instructions (Ralph loop rules,
// MCP tool catalog, and the Act-step norm "run the repo's quality gates")
// actually reach the model — an env var the model never sees shapes
// nothing.
const protocolPrompt = (process.env.GYRE_PROTOCOL_PROMPT || '').trim();
const fullPrompt = [metaSpecPrompt, protocolPrompt, taskPrompt]
  .filter((section) => section.length > 0)
  .join('\n---\n');

// ── Main execution ──────────────────────────────────────────────────────────

console.log(`=== Gyre Agent Runner starting ===`);
console.log(previewMode
  ? `Agent: ${agentId} | Preview: ${previewId} | Branch: ${branch}`
  : `Agent: ${agentId} | Task: ${taskId || 'none'} | Branch: ${branch}`);
console.log(`Model: ${model} | Server: ${serverUrl}`);
postLog(`Agent runner starting: model=${model}, branch=${branch}`);

// Set up provenance hooks
const { turnFile } = createHooksSettingsFile();

let messageCount = 0;
let lastHeartbeat = Date.now();
// Must stay < the server's 60s stale-agent timeout (stale_agents.rs).
// GYRE_HEARTBEAT_INTERVAL_MS overrides the interval for tests/tuning;
// anything missing/invalid keeps the safe default.
const HEARTBEAT_INTERVAL_MS = (() => {
  const parsed = Number.parseInt(process.env.GYRE_HEARTBEAT_INTERVAL_MS ?? '', 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 15_000;
})();
let conversationSha = null;

// ── WebSocket transport (architecture.md §API, task-310) ────────────────────
//
// "WebSocket as the primary transport for agent ↔ server communication".
// The runner holds one authenticated WS connection for the whole session:
// liveness heartbeats travel here first (the HTTP heartbeat stays as the
// documented fallback when the transport is down), and server-directed
// messages (Destination::Agent: shutdown commands, gate-failure feedback,
// spec-rejection grace signals) are PUSHED to the live session in real
// time instead of never arriving. Reconnect with exponential backoff so
// a transient drop does not end the transport.

const WS_RECONNECT_BASE_MS = positiveIntEnv('GYRE_WS_RECONNECT_BASE_MS', 500);
const WS_RECONNECT_MAX_MS = positiveIntEnv('GYRE_WS_RECONNECT_MAX_MS', 15_000);
const WS_HEARTBEAT_ACK_TIMEOUT_MS = positiveIntEnv('GYRE_WS_ACK_TIMEOUT_MS', 10_000);

/** Parse a positive-integer env override; anything missing/invalid keeps
 * the safe default (same pattern as GYRE_HEARTBEAT_INTERVAL_MS). */
function positiveIntEnv(name, fallback) {
  const parsed = Number.parseInt(process.env[name] ?? '', 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

let ws = null; // active WebSocket (null while down)
let wsReconnectTimer = null;
let wsBackoffMs = WS_RECONNECT_BASE_MS;
let wsSessionEnded = false; // terminal: stop reconnecting
let wsHeartbeatPending = null; // { settle, timer } awaiting HeartbeatAck
// Replay boundary for Subscribe: bounded forward from every received bus
// message (server clock) so a reconnect replays exactly what was missed
// during the drop — and never re-fires stale persisted events (old
// shutdown commands included) at/after session start.
let lastSeenMs = Date.now();
// Server-directed shutdown (agent-runtime.md §1 spec rejection): set
// once, honored by the query loop, the gate loop, and the main flow.
let shutdownSignal = null;
// Abort handle for the in-flight SDK query (server-directed shutdown).
let activeQueryAbort = null;

function wsUrl() {
  return `${serverUrl.replace(/^http/, 'ws')}/ws`;
}

function wsIsOpen() {
  return ws !== null && ws.readyState === WebSocket.OPEN;
}

function connectWs() {
  if (wsSessionEnded || !token) return;
  if (typeof WebSocket === 'undefined') {
    // Node < 22 has no global WebSocket — degrade to the HTTP surfaces.
    console.warn('[ws] WebSocket API unavailable — transport degraded to HTTP fallbacks');
    wsSessionEnded = true;
    return;
  }
  let socket;
  try {
    socket = new WebSocket(wsUrl());
  } catch {
    scheduleWsReconnect();
    return;
  }
  ws = socket;
  socket.onopen = () => {
    // First message must be Auth (ws.rs): the server validates the agent
    // token and registers the connection under the verified identity.
    socket.send(JSON.stringify({ type: 'Auth', token }));
  };
  socket.onmessage = (event) => handleWsServerMessage(event.data);
  socket.onclose = () => {
    if (ws === socket) ws = null;
    failPendingWsHeartbeat();
    scheduleWsReconnect();
  };
  socket.onerror = () => {
    // onclose follows every error; reconnection is driven from there.
  };
}

function scheduleWsReconnect() {
  if (wsSessionEnded || wsReconnectTimer !== null) return;
  const delay = wsBackoffMs;
  wsBackoffMs = Math.min(wsBackoffMs * 2, WS_RECONNECT_MAX_MS);
  wsReconnectTimer = setTimeout(() => {
    wsReconnectTimer = null;
    connectWs();
  }, delay);
}

/** Close the transport for good (terminal run state) so the process can
 * exit and no reconnect timer keeps the event loop alive. */
function stopWsTransport(reason) {
  const alreadyDown = wsSessionEnded && ws === null && wsReconnectTimer === null;
  wsSessionEnded = true;
  if (wsReconnectTimer !== null) {
    clearTimeout(wsReconnectTimer);
    wsReconnectTimer = null;
  }
  failPendingWsHeartbeat();
  if (ws !== null) {
    try {
      ws.close();
    } catch {
      // already closed
    }
  }
  if (!alreadyDown) console.log(`[ws] transport closed (${reason})`);
}

function handleWsServerMessage(raw) {
  let msg;
  try {
    msg = JSON.parse(raw);
  } catch {
    return;
  }
  if (msg === null || typeof msg !== 'object') return;
  if (msg.type === undefined) {
    // No `type` tag → a message-bus envelope (gyre message::Message)
    // pushed to this live session.
    handleBusMessage(msg);
    return;
  }
  switch (msg.type) {
    case 'AuthResult':
      if (msg.success) {
        wsBackoffMs = WS_RECONNECT_BASE_MS; // a good connect resets backoff
        subscribeWs();
      } else {
        // A token that fails auth will not become valid by reconnecting.
        console.warn(`[ws] auth rejected: ${msg.message} — transport disabled`);
        wsSessionEnded = true;
        try {
          ws?.close();
        } catch {
          // already closed
        }
      }
      break;
    case 'HeartbeatAck':
      settlePendingWsHeartbeat(msg.success === true);
      break;
    default:
      // Pong / ReplayCatchUp / SubscribeError / future additive types:
      // forward-compatible, ignored.
      break;
  }
}

function subscribeWs() {
  if (!wsIsOpen()) return;
  // Subscribe to the agent's own workspace (GYRE_WORKSPACE_ID, injected
  // at spawn). Agent-directed delivery is identity-gated server-side and
  // works regardless; the subscription adds workspace-scoped push.
  const workspaceId = process.env.GYRE_WORKSPACE_ID;
  if (!workspaceId) {
    console.warn('[ws] GYRE_WORKSPACE_ID not set — subscribing with no scopes');
  }
  const scopes = workspaceId ? [{ workspace_id: workspaceId }] : [];
  ws.send(JSON.stringify({ type: 'Subscribe', scopes, last_seen: lastSeenMs }));
}

/** Handle one pushed message-bus envelope. Shutdown commands trigger
 * the graceful stop; other agent-directed signals are surfaced on the
 * runner log (the operator-visible feedback channel). */
function handleBusMessage(msg) {
  const kind = typeof msg.kind === 'string' ? msg.kind : 'unknown';
  if (typeof msg.created_at === 'number' && msg.created_at > lastSeenMs) {
    lastSeenMs = msg.created_at;
  }
  const toAgent =
    msg.to && typeof msg.to === 'object' && typeof msg.to.agent === 'string'
      ? msg.to.agent
      : null;
  const payload = msg.payload && typeof msg.payload === 'object' ? msg.payload : {};

  // Server-directed shutdown (agent-runtime.md §1 spec rejection /
  // cascaded shutdown): react in real time — graceful stop within the
  // specced grace window instead of never receiving the signal.
  if (kind === 'status_update' && payload.action === 'shutdown') {
    requestShutdown(payload);
    return;
  }

  if (toAgent === agentId) {
    const summary = JSON.stringify(payload).slice(0, 500);
    console.log(`[ws] server message (${kind}): ${summary}`);
    postLog(`[server] ${kind}: ${summary}`);
  }
}

/** Record the server-directed shutdown and abort the in-flight SDK query
 * immediately — waiting for the message loop's next iteration could burn
 * the whole grace window on one long LLM call. */
function requestShutdown(payload) {
  if (shutdownSignal) return; // already stopping
  const payloadGrace = Number(payload.grace_period_secs);
  const graceSecs =
    Number.isFinite(payloadGrace) && payloadGrace > 0 ? payloadGrace : 60;
  shutdownSignal = { reason: payload.reason || 'server-directed shutdown', graceSecs };
  console.warn(
    `[ws] shutdown signal received: ${shutdownSignal.reason} (grace: ${graceSecs}s) — stopping`,
  );
  postLog(`Shutdown signal received: ${shutdownSignal.reason}`);
  try {
    activeQueryAbort?.abort();
  } catch {
    // already aborted
  }
}

/**
 * Server-directed graceful stop (agent-runtime.md §1 spec-rejection
 * shutdown): the task was cancelled server-side, so no local gates run
 * and no completion/MR is created — record provenance (best-effort),
 * log, and exit 0 inside the grace window.
 */
async function gracefulStop() {
  console.warn(`[ws] graceful stop: ${shutdownSignal.reason}`);
  if (!previewMode) {
    try {
      conversationSha = await uploadConversation();
    } catch {
      // best-effort — the stop itself must not be delayed past the window
    }
  }
  await postLog(`Agent stopped by server signal: ${shutdownSignal.reason}`);
  stopWsTransport('graceful stop');
  process.exit(0);
}

function settlePendingWsHeartbeat(ok) {
  if (wsHeartbeatPending) {
    const { settle, timer } = wsHeartbeatPending;
    clearTimeout(timer);
    wsHeartbeatPending = null;
    settle(ok);
  }
}

function failPendingWsHeartbeat() {
  settlePendingWsHeartbeat(false);
}

/**
 * Send one heartbeat over the WS transport and await its ack. Resolves
 * false when the transport is down, the ack times out, or the heartbeat
 * was rejected — the caller then falls back to the HTTP surface.
 */
function sendWsHeartbeat(svid, stackFingerprint) {
  return new Promise((resolve) => {
    if (!wsIsOpen()) {
      resolve(false);
      return;
    }
    // One heartbeat is outstanding at a time (15s interval ≫ ack timeout).
    settlePendingWsHeartbeat(false);
    const timer = setTimeout(
      () => settlePendingWsHeartbeat(false),
      WS_HEARTBEAT_ACK_TIMEOUT_MS,
    );
    wsHeartbeatPending = { settle: resolve, timer };
    try {
      ws.send(
        JSON.stringify({ type: 'Heartbeat', svid, stack_fingerprint: stackFingerprint }),
      );
    } catch {
      settlePendingWsHeartbeat(false);
    }
  });
}

/**
 * Send one heartbeat to the server. architecture.md §API (task-310): the
 * WebSocket is the PRIMARY channel — heartbeats ride the authenticated
 * transport and fall back to the documented HTTP surface
 * (PUT /api/v1/agents/{id}/heartbeat) only when the transport is down or
 * the ack fails, so stale-agent detection is fed identically on either
 * channel. identity-security.md Layer 1: the heartbeat carries the
 * session's X.509-SVID for fail-closed workload-identity verification
 * (GYRE_SVID_PEM is injected at spawn).
 */
async function sendHeartbeat() {
  lastHeartbeat = Date.now();
  const svid = process.env.GYRE_SVID_PEM || '';
  const stackFingerprint = process.env.GYRE_STACK_HASH || '';
  if (wsIsOpen()) {
    const acked = await sendWsHeartbeat(svid, stackFingerprint);
    if (acked) return;
  }
  await sendHttpHeartbeat(svid, stackFingerprint);
}

/** HTTP heartbeat fallback (docs/agent-protocol.md): kept for
 * compatibility and for transports where the WS channel is unavailable. */
async function sendHttpHeartbeat(svid, stackFingerprint) {
  try {
    const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {
      method: 'PUT',
      headers: {
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ svid, stack_fingerprint: stackFingerprint }),
    });
    if (!resp.ok) console.warn(`[heartbeat] failed: ${resp.status}`);
  } catch (e) {
    console.warn(`[heartbeat] error: ${e.message}`);
  }
}

/** Fire a heartbeat if the interval has elapsed since the last one.
 * Shared by the SDK message loop AND the local-gate execution phase
 * below — gate subprocesses must not starve the liveness signal. */
function maybeHeartbeat() {
  if (Date.now() - lastHeartbeat > HEARTBEAT_INTERVAL_MS) {
    return sendHeartbeat();
  }
}

/**
 * Run one SDK query to completion, collecting provenance, forwarding
 * progress to stdout/server logs, and heartbeating. Used for the initial
 * task prompt AND for pre-PR gate-failure remediation queries.
 */
async function runAgentQuery(prompt) {
  // Abort handle for server-directed shutdown: requestShutdown() aborts
  // the in-flight query so the graceful stop lands inside the grace
  // window instead of after the current LLM call finishes.
  const abortController = new AbortController();
  activeQueryAbort = abortController;
  try {
    for await (const message of query({
      prompt,
      options: { ...options, abortController },
    })) {
      messageCount++;

      // Server-directed stop: leave the loop as soon as the signal lands.
      if (shutdownSignal) break;

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

      // Periodic heartbeat (shared with the local-gate phase below).
      await maybeHeartbeat();
    }
  } finally {
    if (activeQueryAbort === abortController) activeQueryAbort = null;
  }
}

// ── Pre-PR local verification (agent-workflow.md §Immediate Feedback) ───────
//
// The server delivers the repo's locally-runnable command gates as
// GYRE_LOCAL_GATES JSON ([{id, name, command, timeout_secs?}]), derived
// from the repo's real QualityGate configuration. After the agent query
// loop and BEFORE any conversation upload / completion call (which creates
// the MR), each command is executed as a real subprocess in the agent's
// working directory with a bounded timeout — no unbounded waits: slow or
// absent feedback is a platform bug per the spec. On failure the captured
// output is fed back to the agent as a follow-up SDK query for remediation;
// completion happens only when every gate passes. If the retry cap is
// exhausted the runner terminates with a nonzero exit and no completion
// call — an unambiguous failure, never a knowingly-broken MR.

const LOCAL_GATE_DEFAULT_TIMEOUT_MS = 300_000;
const LOCAL_GATE_MAX_OUTPUT_CHARS = 8_000;

/**
 * Parse GYRE_LOCAL_GATES. Returns an array (possibly empty) on success, or
 * null when the variable is set but malformed — the platform delivered
 * corrupt gate config, and treating that as "no gates" would silently skip
// local verification and open an MR anyway. The caller fails closed.
 */
function parseLocalGates() {
  const raw = process.env.GYRE_LOCAL_GATES;
  if (!raw || !raw.trim()) return [];
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (e) {
    console.error(`[local-gates] GYRE_LOCAL_GATES is malformed JSON — failing closed, no MR will be created: ${e.message}`);
    return null;
  }
  if (!Array.isArray(parsed)) {
    console.error('[local-gates] GYRE_LOCAL_GATES is not a JSON array — failing closed, no MR will be created');
    return null;
  }
  return parsed.filter((g) => g && typeof g.command === 'string' && g.command.trim() !== '');
}

/** The directory gate commands run in: the agent worktree when it exists,
 * else the process cwd (the entrypoint starts the runner there). */
function gateWorkDir() {
  const configured = process.env.GYRE_WORK_DIR;
  if (configured && existsSync(configured)) return configured;
  return process.cwd();
}

function truncateForFeedback(text) {
  if (text.length <= LOCAL_GATE_MAX_OUTPUT_CHARS) return text;
  return text.slice(0, LOCAL_GATE_MAX_OUTPUT_CHARS) + '\n...(truncated)';
}

/**
 * Execute one gate command as a real subprocess with a bounded timeout.
 * Resolves {ok, code, timedOut, output}; a hang is SIGKILLed at the timeout
 * and reported as a failure with feedback, never an indefinite wait.
 */
function runGateCommand(gate, cwd) {
  const timeoutMs = Number.isFinite(gate.timeout_secs) && gate.timeout_secs > 0
    ? gate.timeout_secs * 1000
    : LOCAL_GATE_DEFAULT_TIMEOUT_MS;
  return new Promise((resolve) => {
    // Detached process group (posix): the shell runs the command in its own
    // group so a timeout kill can take down the WHOLE tree — killing only
    // the shell would orphan the real command, which would keep the stdio
    // pipes (and this promise) open well past the timeout.
    const child = spawnChild(gate.command, {
      shell: true,
      cwd,
      detached: process.platform !== 'win32',
    });
    let output = '';
    let timedOut = false;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    const killTree = () => {
      try {
        if (process.platform === 'win32' || !child.pid) {
          child.kill('SIGKILL');
        } else {
          process.kill(-child.pid, 'SIGKILL');
        }
      } catch {
        try { child.kill('SIGKILL'); } catch { /* already gone */ }
      }
    };
    const timer = setTimeout(() => {
      timedOut = true;
      killTree();
      // Guarantee the bounded wait even if the kill cannot take down the
      // whole tree (e.g. an orphaned grandchild holding the stdio pipes):
      // destroy our ends of the pipes and settle NOW with a timeout
      // failure — never an indefinite hang.
      try {
        child.stdout?.destroy();
        child.stderr?.destroy();
      } catch { /* already closed */ }
      finish({
        ok: false,
        code: null,
        timedOut: true,
        output: `${output}\n[gate timed out after ${timeoutMs}ms and was killed]`,
      });
    }, timeoutMs);
    child.stdout?.on('data', (d) => { output += d.toString(); });
    child.stderr?.on('data', (d) => { output += d.toString(); });
    child.on('error', (err) => {
      finish({ ok: false, code: null, timedOut: false, output: `${output}\n[gate process failed to start: ${err.message}]` });
    });
    child.on('close', (code) => {
      if (timedOut) return; // already settled by the timeout path
      finish({ ok: code === 0, code, timedOut: false, output });
    });
  });
}

/** Run every local gate; log each result (failures include the captured
 * output — the runner log is the unambiguous feedback channel) and return
 * the failing gates with their results. */
async function runLocalGates(gates) {
  const cwd = gateWorkDir();
  const failures = [];
  for (const gate of gates) {
    const label = gate.name || gate.command;
    console.log(`[local-gates] running gate "${label}": ${gate.command}`);
    postLog(`[local-gates] running gate "${label}"`);
    const result = await runGateCommand(gate, cwd);
    if (result.ok) {
      console.log(`[local-gates] gate "${label}" passed`);
      postLog(`[local-gates] gate "${label}" passed`);
    } else {
      const reason = result.timedOut
        ? `timed out after ${gate.timeout_secs ?? LOCAL_GATE_DEFAULT_TIMEOUT_MS / 1000}s and was killed`
        : result.code === null
          ? 'failed to start'
          : `exited with code ${result.code}`;
      console.error(`[local-gates] gate "${label}" FAILED (${reason})`);
      console.error(`[local-gates] gate "${label}" output:\n${truncateForFeedback(result.output)}`);
      postLog(`[local-gates] gate "${label}" FAILED (${reason})`);
      failures.push({ gate, result });
    }
  }
  return failures;
}

/** Build the remediation follow-up query: the failing gates' names and
 * captured output fed back to the agent so it can fix the work. */
function gateFailureFeedback(failures) {
  const sections = failures.map(({ gate, result }) => {
    const label = gate.name || gate.command;
    const reason = result.timedOut
      ? 'the command did not finish within its timeout and was killed'
      : result.code === null
        ? 'the command failed to start'
        : `the command exited with code ${result.code}`;
    return `#### Gate "${label}" failed\nCommand: \`${gate.command}\`\nReason: ${reason}\nCaptured output:\n\`\`\`\n${truncateForFeedback(result.output)}\n\`\`\``;
  });
  return `The repository's local quality gates were executed in your working directory and FAILED.
No merge request will be created until every gate passes. Fix the failures listed below,
commit your changes, and push your branch.

${sections.join('\n\n')}`;
}

/** Upload conversation provenance and signal completion (creates the MR).
 * Called ONLY after all local gates pass. */
async function completeAgent() {
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
}

// Primary transport (architecture.md §API, task-310): open the
// authenticated WS connection for the session's duration. Heartbeats
// ride it first; server-directed messages (shutdown commands, gate
// feedback) are pushed to this live session in real time.
connectWs();

try {
  await runAgentQuery(fullPrompt);

  // Server-directed shutdown arrived mid-query: stop now, inside the
  // grace window — no gate phase, no completion (the task was cancelled
  // server-side; an MR for cancelled work must not open).
  if (shutdownSignal) {
    await gracefulStop();
  }

  // ── Preview mode: no gates, no provenance upload, no completion call ─────
  // Spec §5 skips quality gates, provenance recording and MR creation — the
  // server kills the agent and derives the diff from the pushed branch, then
  // garbage-collects it.
  if (previewMode) {
    console.log(`=== Preview mode complete: ${messageCount} messages, ${turnCounter} turns, branch ${branch} (throwaway; no upload, no completion) ===`);
    await postLog(`Preview complete: ${messageCount} messages, ${turnCounter} turns, branch=${branch}`);
    stopWsTransport('preview complete');
    process.exit(0);
  }

  // ── Pre-PR local verification: run the repo's command gates BEFORE any
  // completion call. No MR is opened until every gate passes.
  const localGates = parseLocalGates();
  if (localGates === null) {
    // Malformed gate config — fail closed (provenance only, no completion).
    await uploadConversation();
    console.error('=== Agent runner aborted: malformed GYRE_LOCAL_GATES, no MR created ===');
    postLog('Agent runner aborted: malformed GYRE_LOCAL_GATES, no MR created');
    stopWsTransport('malformed gate config');
    process.exit(1);
  }
  if (localGates.length > 0) {
    const parsedCap = parseInt(process.env.GYRE_LOCAL_GATE_MAX_RETRIES ?? '2', 10);
    const maxRemediations = Number.isFinite(parsedCap) && parsedCap >= 0 ? parsedCap : 2;
    let remediations = 0;
    // Heartbeat keep-alive for the gate-execution phase: gate subprocesses
    // routinely run for minutes (a real test suite — the whole point of
    // this feature), and the server's stale-agent detector applies the
    // disconnected behavior after 60s without a heartbeat, tearing the
    // agent down mid-verification. That is exactly the "slow or absent
    // feedback is a platform bug" failure the spec forbids, so keep
    // heartbeating on the same interval until local verification ends.
    const gateHeartbeat = setInterval(() => { maybeHeartbeat(); }, HEARTBEAT_INTERVAL_MS);
    try {
      for (;;) {
        // A shutdown signal that lands during gate execution stops the
        // run now — no further gates, no completion, graceful exit.
        if (shutdownSignal) {
          await gracefulStop();
        }
        const failures = await runLocalGates(localGates);
        if (failures.length === 0) break;
        if (remediations >= maxRemediations) {
          console.error(`[local-gates] ${failures.length} gate(s) still failing after ${remediations} remediation attempt(s) — retry cap exhausted`);
          console.error('[local-gates] NOT signaling completion: no MR will be created for failing work');
          postLog(`Agent runner failed: ${failures.length} local gate(s) still failing after ${remediations} remediation attempt(s); no MR created`);
          // Provenance is uploaded (the conversation explains why), but the
          // completion call — the MR-creating step — is never made.
          await uploadConversation();
          stopWsTransport('gate failure');
          process.exit(1);
        }
        remediations++;
        console.log(`[local-gates] ${failures.length} gate(s) failed — feeding failure output back to the agent (remediation attempt ${remediations}/${maxRemediations})`);
        postLog(`[local-gates] ${failures.length} gate(s) failed — remediation attempt ${remediations}/${maxRemediations}`);
        await runAgentQuery(gateFailureFeedback(failures));
      }
    } finally {
      clearInterval(gateHeartbeat);
    }
  }

  // ── All local gates passed (or none configured): upload conversation,
  // then signal completion ─────────────────────────────────────────────

  await completeAgent();

  console.log(`=== Agent runner complete (${messageCount} messages, ${turnCounter} turns) ===`);
  postLog(`Agent complete: ${messageCount} messages, ${turnCounter} turns, conversation_sha=${conversationSha || 'none'}`);
  stopWsTransport('run complete');
} catch (err) {
  // A server-directed shutdown aborts the in-flight SDK query; that
  // abort is the graceful stop, not a runner error.
  if (shutdownSignal) {
    await gracefulStop();
  }

  // Preview mode records no provenance and never signals completion — the
  // server reads the branch (or its absence) and garbage-collects it.
  if (!previewMode) {
    // Best-effort: upload whatever conversation we have even on error —
    // provenance only. The error-path force-completion is deliberately
    // REMOVED (task-268): a runner error before local verification passes
    // must not open an MR.
    console.log(`[provenance] Uploading partial conversation after error...`);
    conversationSha = await uploadConversation();
  }

  console.error(`=== Agent runner error: ${err.message} ===`);
  console.error(`=== Agent runner error: ${err.message} ===`);
  console.error('[complete] skipped: runner error before local verification passed — no MR created');
  postLog(`Agent runner error: ${err.message}; completion skipped, no MR created`);
  stopWsTransport('runner error');
  process.exit(1);
}
