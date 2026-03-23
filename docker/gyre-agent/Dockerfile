FROM node:22-alpine

# util-linux provides `shred` for secure secret file deletion (M27)
RUN apk add --no-cache git bash curl ca-certificates util-linux

# Install Claude Agent SDK globally (pin version for reproducible builds)
RUN npm install -g @anthropic-ai/claude-agent-sdk@0.2.81

# Create workspace, gyre, and runtime directories
RUN mkdir -p /gyre /workspace /run/gyre

# Copy agent runner, credential proxy, and entrypoint
COPY cred-proxy.mjs /gyre/cred-proxy.mjs
COPY agent-runner.mjs /gyre/agent-runner.mjs
COPY entrypoint.sh /gyre/entrypoint.sh
RUN chmod +x /gyre/entrypoint.sh

WORKDIR /workspace
ENTRYPOINT ["/gyre/entrypoint.sh"]
