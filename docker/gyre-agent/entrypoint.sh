#!/bin/bash
set -euo pipefail

# Verify required env vars (GYRE_AUTH_TOKEN still present at this stage for cred-proxy startup)
for var in GYRE_SERVER_URL GYRE_CLONE_URL GYRE_BRANCH GYRE_AGENT_ID; do
    if [ -z "${!var:-}" ]; then
        echo "ERROR: Required env var $var is not set"
        exit 1
    fi
done

echo "=== Gyre Agent Bootstrap ==="
echo "Agent ID: $GYRE_AGENT_ID"
echo "Server: $GYRE_SERVER_URL"
echo "Branch: $GYRE_BRANCH"

# M27: Start credential proxy before touching any credentials.
# cred-proxy.mjs reads GYRE_CRED_* env vars into memory and scrubs them.
mkdir -p /run/gyre

echo "Starting cred-proxy..."
node /gyre/cred-proxy.mjs &
CRED_PROXY_PID=$!

# Wait for TCP listener to be ready (up to 5 s)
for i in $(seq 1 10); do
    if curl -sf http://127.0.0.1:8765/proxy -X POST \
            -H 'Content-Type: application/json' \
            -d '{}' >/dev/null 2>&1 || \
       [ -S /run/gyre/cred.sock ]; then
        break
    fi
    sleep 0.5
done

# Set proxy env vars for the agent process
export GYRE_CRED_PROXY=http://127.0.0.1:8765
export ANTHROPIC_BASE_URL=http://127.0.0.1:8765
# Placeholder so the Anthropic SDK initialises without error; real key is in the proxy
export ANTHROPIC_API_KEY=proxy-managed

# If Vertex AI is configured (GCE metadata emulator started by cred-proxy)
# inject non-secret Vertex config into agent env.  The SA JSON was scrubbed by cred-proxy.
if [ -n "${ANTHROPIC_VERTEX_PROJECT_ID:-}" ]; then
    export GCE_METADATA_HOST=127.0.0.1:8080
fi

# M27: Scrub any GYRE_CRED_* vars that weren't already consumed by cred-proxy
# (belt-and-suspenders: cred-proxy also scrubs these from its own process.env).
for var in $(printenv | grep '^GYRE_CRED_' | cut -d= -f1 2>/dev/null || true); do
    unset "$var" 2>/dev/null || true
done

# Configure git credentials via credential helper (token not embedded in URLs)
cat > /tmp/git-credential-gyre <<'CRED_EOF'
#!/bin/bash
echo "username=agent"
echo "password=$GYRE_AUTH_TOKEN"
CRED_EOF
chmod +x /tmp/git-credential-gyre
git config --global credential.helper '/tmp/git-credential-gyre'
git config --global user.name "gyre-agent-$GYRE_AGENT_ID"
git config --global user.email "agent-$GYRE_AGENT_ID@gyre.local"

# Clone the repository
echo "Cloning $GYRE_CLONE_URL..."
if ! git clone -b "$GYRE_BRANCH" "$GYRE_CLONE_URL" /workspace/repo 2>&1; then
    git clone "$GYRE_CLONE_URL" /workspace/repo 2>&1
    cd /workspace/repo
    git checkout -b "$GYRE_BRANCH"
fi

cd /workspace/repo

# Send heartbeat to signal the agent is alive
curl -s -X PUT "$GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/heartbeat" \
    -H "Authorization: Bearer ${GYRE_AUTH_TOKEN:-}" \
    -H "Content-Type: application/json" \
    -d '{"pid": '"$$"'}' || true

echo "=== Agent ready. Workspace: /workspace/repo ==="

# If GYRE_AGENT_COMMAND is set, run it (server-controlled, not user input)
if [ -n "${GYRE_AGENT_COMMAND:-}" ]; then
    exec $GYRE_AGENT_COMMAND
else
    echo "Starting Claude agent runner..."
    exec node /gyre/agent-runner.mjs
fi
