#!/bin/bash
set -euo pipefail

# Verify required env vars
for var in GYRE_SERVER_URL GYRE_AUTH_TOKEN GYRE_CLONE_URL GYRE_BRANCH GYRE_AGENT_ID; do
    if [ -z "${!var:-}" ]; then
        echo "ERROR: Required env var $var is not set"
        exit 1
    fi
done

echo "=== Gyre Agent Bootstrap ==="
echo "Agent ID: $GYRE_AGENT_ID"
echo "Server: $GYRE_SERVER_URL"
echo "Branch: $GYRE_BRANCH"

# Configure git credentials for Gyre server
git config --global credential.helper '!f() { echo "password=$GYRE_AUTH_TOKEN"; }; f'
git config --global user.name "gyre-agent-$GYRE_AGENT_ID"
git config --global user.email "agent-$GYRE_AGENT_ID@gyre.local"

# Embed auth token in clone URL for git
CLONE_URL_WITH_AUTH=$(echo "$GYRE_CLONE_URL" | sed "s|://|://agent:$GYRE_AUTH_TOKEN@|")

# Clone the repository
echo "Cloning $GYRE_CLONE_URL..."
if ! git clone -b "$GYRE_BRANCH" "$CLONE_URL_WITH_AUTH" /workspace/repo 2>&1; then
    # Branch might not exist yet — clone default and create branch
    git clone "$CLONE_URL_WITH_AUTH" /workspace/repo 2>&1
    cd /workspace/repo
    git checkout -b "$GYRE_BRANCH"
fi

cd /workspace/repo

# Send heartbeat to signal the agent is alive
curl -s -X PUT "$GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/heartbeat" \
    -H "Authorization: Bearer $GYRE_AUTH_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"pid": '"$$"'}' || true

echo "=== Agent ready. Workspace: /workspace/repo ==="
echo "=== Run your agent command or Claude Agent SDK here ==="

# If GYRE_AGENT_COMMAND is set, run it
if [ -n "${GYRE_AGENT_COMMAND:-}" ]; then
    eval "$GYRE_AGENT_COMMAND"
else
    # Keep container alive for interactive/SDK use
    echo "No GYRE_AGENT_COMMAND set. Container will stay alive for 1 hour."
    echo "Connect via: docker exec -it <container_id> bash"
    sleep 3600
fi
