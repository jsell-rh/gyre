FROM node:22-alpine

RUN apk add --no-cache git bash curl ca-certificates

# Install Claude Agent SDK globally (pin version for reproducible builds)
RUN npm install -g @anthropic-ai/claude-agent-sdk@0.2.81

# Create workspace and gyre directories
RUN mkdir -p /gyre /workspace

# Copy agent runner script and entrypoint
COPY agent-runner.mjs /gyre/agent-runner.mjs
COPY entrypoint.sh /gyre/entrypoint.sh
RUN chmod +x /gyre/entrypoint.sh

WORKDIR /workspace
ENTRYPOINT ["/gyre/entrypoint.sh"]
