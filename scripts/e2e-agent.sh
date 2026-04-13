#!/usr/bin/env bash
# =============================================================================
# e2e-agent.sh — Minimal agent process for e2e testing
#
# This script is spawned by the Gyre server as a real agent process.
# It receives env vars (GYRE_SERVER_URL, GYRE_AUTH_TOKEN, GYRE_CLONE_URL,
# GYRE_BRANCH, GYRE_AGENT_ID, GYRE_TASK_ID, GYRE_REPO_ID) and performs
# the full agent lifecycle: clone → implement → commit → push → complete.
#
# Unlike the simulated agent in e2e-flow.sh, this script is actually
# spawned by the server's agent spawn machinery with a real JWT.
# =============================================================================

set -euo pipefail

# --- Validate required env vars ---
: "${GYRE_SERVER_URL:?GYRE_SERVER_URL not set}"
: "${GYRE_AUTH_TOKEN:?GYRE_AUTH_TOKEN not set}"
: "${GYRE_CLONE_URL:?GYRE_CLONE_URL not set}"
: "${GYRE_BRANCH:?GYRE_BRANCH not set}"
: "${GYRE_AGENT_ID:?GYRE_AGENT_ID not set}"
: "${GYRE_TASK_ID:?GYRE_TASK_ID not set}"

API="${GYRE_SERVER_URL}/api/v1"
AUTH="Authorization: Bearer ${GYRE_AUTH_TOKEN}"

log() { echo "[e2e-agent] $1"; }

log "Starting: agent=${GYRE_AGENT_ID} task=${GYRE_TASK_ID} branch=${GYRE_BRANCH}"

# --- Heartbeat ---
curl -sf -X PUT -H "$AUTH" "${API}/agents/${GYRE_AGENT_ID}/heartbeat" >/dev/null 2>&1 || true
log "Heartbeat sent"

# --- Read task description ---
TASK=$(curl -sf -H "$AUTH" "${API}/tasks/${GYRE_TASK_ID}" 2>/dev/null) || TASK="{}"
TASK_TITLE=$(echo "$TASK" | jq -r '.title // "unknown"')
TASK_DESC=$(echo "$TASK" | jq -r '.description // ""')
log "Task: ${TASK_TITLE}"

# --- Clone repo ---
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

log "Cloning ${GYRE_CLONE_URL}..."
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="$AUTH" \
  git clone "${GYRE_CLONE_URL}.git" "$WORK_DIR/repo" 2>/dev/null || {
    # Empty repo — init
    mkdir -p "$WORK_DIR/repo"
    cd "$WORK_DIR/repo"
    git init
    git remote add origin "${GYRE_CLONE_URL}.git"
}

cd "$WORK_DIR/repo"
git config user.email "agent-${GYRE_AGENT_ID}@gyre.local"
git config user.name "Gyre Agent ${GYRE_AGENT_ID:0:8}"

# Fetch latest main and create feature branch
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="$AUTH" \
  git fetch origin main 2>/dev/null || true

git checkout -b "${GYRE_BRANCH}" origin/main 2>/dev/null || git checkout -b "${GYRE_BRANCH}" 2>/dev/null || true

# --- Implement the task ---
log "Implementing..."

# Create a real implementation based on the task
mkdir -p src
cat > src/agent_impl.rs << 'RUST'
//! Agent-generated implementation.
//!
//! This module was created by an autonomous Gyre agent
//! executing a task from the spec→agent pipeline.

/// Result of agent work.
pub struct AgentResult {
    /// Whether the implementation succeeded.
    pub success: bool,
    /// Summary of what was done.
    pub summary: String,
}

impl AgentResult {
    /// Create a successful result.
    pub fn ok(summary: &str) -> Self {
        Self {
            success: true,
            summary: summary.to_string(),
        }
    }
}

/// Verify the implementation meets spec requirements.
pub fn verify() -> AgentResult {
    AgentResult::ok("All spec requirements implemented and verified")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify() {
        let result = verify();
        assert!(result.success);
    }
}
RUST

# Add module to lib.rs if it exists
if [ -f src/lib.rs ]; then
  if ! grep -q "mod agent_impl" src/lib.rs; then
    echo "" >> src/lib.rs
    echo "pub mod agent_impl;" >> src/lib.rs
  fi
fi

git add .
git commit -m "feat: implement task via autonomous agent

Task: ${TASK_TITLE}
Agent-ID: ${GYRE_AGENT_ID}
Task-ID: ${GYRE_TASK_ID}" --no-gpg-sign

# --- Push ---
log "Pushing to ${GYRE_BRANCH}..."
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="http.extraHeader" \
  GIT_CONFIG_VALUE_0="$AUTH" \
  git push origin "${GYRE_BRANCH}" 2>&1 || {
    log "ERROR: push failed"
    exit 1
  }

log "Push complete"

# --- Heartbeat again ---
curl -sf -X PUT -H "$AUTH" "${API}/agents/${GYRE_AGENT_ID}/heartbeat" >/dev/null 2>&1 || true

# --- Complete ---
log "Signaling completion..."
COMPLETE_RESP=$(curl -sf -H "$AUTH" -H "Content-Type: application/json" \
  -d "{\"branch\":\"${GYRE_BRANCH}\",\"title\":\"feat: implement task via autonomous agent\",\"target_branch\":\"main\"}" \
  "${API}/agents/${GYRE_AGENT_ID}/complete" 2>/dev/null) || {
    log "ERROR: complete call failed"
    exit 1
  }

MR_ID=$(echo "$COMPLETE_RESP" | jq -r '.id // "unknown"')
log "Complete! MR created: ${MR_ID}"
log "Agent lifecycle finished successfully"
