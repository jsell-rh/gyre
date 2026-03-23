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

# Configure git credentials via credential helper (avoids embedding
# token in URLs, .git/config, or process listings).
cat > /tmp/git-credential-gyre <<'CRED_EOF'
#!/bin/bash
echo "username=agent"
echo "password=$GYRE_AUTH_TOKEN"
CRED_EOF
chmod +x /tmp/git-credential-gyre
git config --global credential.helper '/tmp/git-credential-gyre'
git config --global user.name "gyre-agent-$GYRE_AGENT_ID"
git config --global user.email "agent-$GYRE_AGENT_ID@gyre.local"

# Clone the repository (token provided via credential helper, not in URL)
echo "Cloning $GYRE_CLONE_URL..."
if ! git clone -b "$GYRE_BRANCH" "$GYRE_CLONE_URL" /workspace/repo 2>&1; then
    # Branch might not exist yet — clone default and create branch
    git clone "$GYRE_CLONE_URL" /workspace/repo 2>&1
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

# If GYRE_AGENT_COMMAND is set, run it (server-controlled, not user input)
if [ -n "${GYRE_AGENT_COMMAND:-}" ]; then
    exec $GYRE_AGENT_COMMAND
else
    # Default: run the Claude Agent SDK runner (M25 zero-config)
    echo "Starting Claude agent runner..."
    exec node /gyre/agent-runner.mjs
fi
