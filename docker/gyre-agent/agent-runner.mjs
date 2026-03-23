#!/usr/bin/env node
/**
 * Gyre Agent Runner — M25 / M27
 *
 * Runs a Claude agent inside the gyre-agent container using the Claude Agent
 * SDK.  Connects back to the Gyre server via MCP so the agent can manage
 * tasks, send heartbeats, and call gyre_agent_complete when done.
 *
 * M27: When GYRE_CRED_PROXY is set, Anthropic API calls are routed through
 * the credential proxy via ANTHROPIC_BASE_URL (set by entrypoint.sh).
 * The GYRE_AUTH_TOKEN is still used directly for Gyre API calls as an
 * interim measure (see spec M27.4 for full opacity plan).
 */

import { query } from '@anthropic-ai/claude-agent-sdk';

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

const model = process.env.GYRE_AGENT_MODEL || 'claude-sonnet-4-6';

const options = {
  model,
  mcpServers: {
    gyre: {
      type: 'http',
      url: `${serverUrl}/mcp`,
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    },
  },
  allowedTools: ['Read', 'Write', 'Edit', 'Bash', 'Glob', 'Grep', 'mcp__gyre__*'],
  permissionMode: 'acceptEdits',
};

const taskPrompt = process.env.GYRE_TASK_PROMPT ||
  `You are a Gyre autonomous agent. Your configuration:
- Agent ID: ${agentId}
- Task ID: ${taskId}
- Branch: ${branch}
- Repo ID: ${repoId || 'unknown'}
- Working directory: /workspace/repo

You have been spawned to complete the task assigned to you. Your working directory
contains a checked-out git clone of the repository on your branch.

Instructions:
1. Use \`gyre_list_tasks\` to read your assigned task (id: ${taskId}) for full details.
2. Implement the task requirements by editing files in /workspace/repo.
3. Commit your changes with a descriptive conventional-commit message.
4. Push your changes: \`git push origin ${branch}\`
5. Use \`gyre_agent_complete\` to signal completion (branch: "${branch}", target_branch: "main").

Use \`gyre_agent_heartbeat\` periodically to signal liveness.
Use \`gyre_record_activity\` to log significant progress milestones.

Begin by reading your task description, then implement it completely.`;

console.log(`=== Gyre Agent Runner starting ===`);
console.log(`Agent: ${agentId} | Task: ${taskId} | Branch: ${branch}`);
console.log(`Model: ${model} | Server: ${serverUrl}`);

let messageCount = 0;
let lastHeartbeat = Date.now();
const HEARTBEAT_INTERVAL_MS = 60_000;

try {
  for await (const message of query({ prompt: taskPrompt, options })) {
    messageCount++;

    if (message.type === 'text') {
      process.stdout.write(message.content);
    } else if (message.type === 'tool_use') {
      console.log(`[tool] ${message.name}`);
    } else if (message.type === 'tool_result') {
      if (messageCount % 10 === 0) {
        console.log(`[progress] ${messageCount} messages processed`);
      }
    }

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

  console.log(`=== Agent runner complete (${messageCount} messages) ===`);
} catch (err) {
  console.error(`=== Agent runner error: ${err.message} ===`);
  process.exit(1);
}
