#!/usr/bin/env bash
set -euo pipefail

# Gyre E2E Agent Runner — uses the proper agent-runner.mjs with Claude Agent SDK
#
# This script is spawned by the Gyre server as a real agent process.
# It receives GYRE_* env vars and runs the agent-runner.mjs which provides:
#   - Claude Agent SDK integration via MCP
#   - Conversation provenance (turn tracking, upload)
#   - Heartbeat management
#
# For local e2e runs, Vertex credentials come from gcloud ADC (no cred-proxy needed).
# The server passes CLAUDE_CODE_USE_VERTEX=1 and ANTHROPIC_VERTEX_PROJECT_ID
# via the container_env map.

: "${GYRE_SERVER_URL:?}" "${GYRE_AUTH_TOKEN:?}" "${GYRE_CLONE_URL:?}"
: "${GYRE_BRANCH:?}" "${GYRE_AGENT_ID:?}" "${GYRE_TASK_ID:?}"

echo "[agent] Starting: agent=${GYRE_AGENT_ID:0:8} task=${GYRE_TASK_ID:0:8} branch=${GYRE_BRANCH}"

# ── Install dependencies if needed ──────────────────────────────────────────

AGENT_DIR="$(cd "$(dirname "$0")/../docker/gyre-agent" && pwd)"

if [ ! -d "$AGENT_DIR/node_modules" ]; then
  echo "[agent] Installing npm dependencies in $AGENT_DIR..."
  (cd "$AGENT_DIR" && npm install --silent 2>&1) || {
    echo "[agent] WARNING: npm install failed — falling back to global SDK"
  }
fi

# ── Vertex AI configuration ─────────────────────────────────────────────────

# For local e2e: inherit Vertex ADC from environment (gcloud auth application-default login)
# CLAUDE_CODE_USE_VERTEX and ANTHROPIC_VERTEX_PROJECT_ID are forwarded by spawn.rs
# CLOUD_ML_REGION / GYRE_VERTEX_LOCATION provide the region

if [ -n "${GYRE_VERTEX_LOCATION:-}" ] && [ -z "${CLOUD_ML_REGION:-}" ]; then
  export CLOUD_ML_REGION="$GYRE_VERTEX_LOCATION"
fi

# ── Heartbeat ────────────────────────────────────────────────────────────────

curl -sf -X PUT -H "Authorization: Bearer $GYRE_AUTH_TOKEN" \
  "${GYRE_SERVER_URL}/api/v1/agents/${GYRE_AGENT_ID}/heartbeat" >/dev/null 2>&1 || true

# ── Clone repo ───────────────────────────────────────────────────────────────

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${GYRE_AUTH_TOKEN}" \
  git clone "${GYRE_CLONE_URL}.git" "$WORK/repo" 2>/dev/null || {
    mkdir -p "$WORK/repo"; cd "$WORK/repo"; git init
    git remote add origin "${GYRE_CLONE_URL}.git"
}

cd "$WORK/repo"
git config user.email "agent-${GYRE_AGENT_ID}@gyre.local"
git config user.name "Gyre Agent"

GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${GYRE_AUTH_TOKEN}" \
  git fetch origin main 2>/dev/null || true
git checkout -b "${GYRE_BRANCH}" origin/main 2>/dev/null || git checkout -b "${GYRE_BRANCH}" 2>/dev/null || true

# ── Run agent-runner.mjs ─────────────────────────────────────────────────────

echo "[agent] Running agent-runner.mjs..."
export GYRE_REPO_ID="${GYRE_REPO_ID:-}"

# Set NODE_PATH so the SDK can be resolved from the agent dir
export NODE_PATH="$AGENT_DIR/node_modules"

exec node "$AGENT_DIR/agent-runner.mjs"
