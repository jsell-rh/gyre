#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Gyre Agent Entrypoint (local / e2e)
#
# Local equivalent of docker/gyre-agent/entrypoint.sh. Spawned by the server
# as the GYRE_AGENT_COMMAND. Handles:
#   1. Git clone + branch setup
#   2. Credential configuration (no cred-proxy for local — uses ADC)
#   3. Delegates to agent-runner.mjs (Claude Agent SDK)
#
# Env vars injected by the server:
#   GYRE_SERVER_URL, GYRE_AUTH_TOKEN (JWT), GYRE_CLONE_URL, GYRE_BRANCH,
#   GYRE_AGENT_ID, GYRE_TASK_ID, GYRE_REPO_ID,
#   CLAUDE_CODE_USE_VERTEX, ANTHROPIC_VERTEX_PROJECT_ID, CLOUD_ML_REGION
# =============================================================================

: "${GYRE_SERVER_URL:?}" "${GYRE_AUTH_TOKEN:?}" "${GYRE_CLONE_URL:?}"
: "${GYRE_BRANCH:?}" "${GYRE_AGENT_ID:?}" "${GYRE_TASK_ID:?}"

echo "=== Gyre Agent Bootstrap (local) ==="
echo "Agent: ${GYRE_AGENT_ID}"
echo "Task:  ${GYRE_TASK_ID}"
echo "Branch: ${GYRE_BRANCH}"
echo "Server: ${GYRE_SERVER_URL}"

# ── Resolve paths ────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AGENT_DIR="$(cd "$SCRIPT_DIR/../docker/gyre-agent" && pwd)"

if [ ! -f "$AGENT_DIR/agent-runner.mjs" ]; then
  echo "ERROR: agent-runner.mjs not found at $AGENT_DIR"
  exit 1
fi

# ── Install SDK if needed ────────────────────────────────────────────────────

if [ ! -d "$AGENT_DIR/node_modules/@anthropic-ai" ]; then
  echo "Installing Claude Agent SDK..."
  (cd "$AGENT_DIR" && npm install --silent 2>&1) || {
    echo "ERROR: npm install failed"
    exit 1
  }
fi

# ── Vertex AI (local: ADC, no cred-proxy) ────────────────────────────────────

if [ -n "${GYRE_VERTEX_LOCATION:-}" ] && [ -z "${CLOUD_ML_REGION:-}" ]; then
  export CLOUD_ML_REGION="$GYRE_VERTEX_LOCATION"
fi

if [ "${CLAUDE_CODE_USE_VERTEX:-}" = "1" ]; then
  echo "Vertex AI: project=${ANTHROPIC_VERTEX_PROJECT_ID:-unset} region=${CLOUD_ML_REGION:-unset}"
fi

# ── Heartbeat ────────────────────────────────────────────────────────────────

curl -sf -X PUT -H "Authorization: Bearer $GYRE_AUTH_TOKEN" \
  "${GYRE_SERVER_URL}/api/v1/agents/${GYRE_AGENT_ID}/heartbeat" >/dev/null 2>&1 || true

# ── Clone repo ───────────────────────────────────────────────────────────────

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "Cloning ${GYRE_CLONE_URL}..."
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${GYRE_AUTH_TOKEN}" \
  git clone "${GYRE_CLONE_URL}.git" "$WORK/repo" 2>/dev/null || {
    echo "Empty repo — initializing..."
    mkdir -p "$WORK/repo"
    cd "$WORK/repo"
    git init
    git remote add origin "${GYRE_CLONE_URL}.git"
  }

cd "$WORK/repo"

# Configure git identity and auth
git config user.email "agent-${GYRE_AGENT_ID}@gyre.local"
git config user.name "Gyre Agent ${GYRE_AGENT_ID:0:8}"
git config "http.${GYRE_SERVER_URL}/.extraHeader" "Authorization: Bearer ${GYRE_AUTH_TOKEN}"

# Fetch and create feature branch
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="Authorization: Bearer ${GYRE_AUTH_TOKEN}" \
  git fetch origin main 2>/dev/null || true

git checkout -b "${GYRE_BRANCH}" origin/main 2>/dev/null || \
  git checkout -b "${GYRE_BRANCH}" 2>/dev/null || true

echo "=== Agent ready. Workspace: $(pwd) ==="

# ── Run agent-runner.mjs ─────────────────────────────────────────────────────

# agent-runner.mjs must run from its own directory (for SDK module resolution).
# Export the working directory so the runner can tell Claude where to work.
export GYRE_WORK_DIR="$(pwd)"

cd "$AGENT_DIR"
exec node agent-runner.mjs
