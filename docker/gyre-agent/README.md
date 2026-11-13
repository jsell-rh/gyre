# gyre-agent Docker Image

Minimal reference Docker image for Gyre container agents. Provides a bootstrap environment that automatically clones the assigned repo, configures git credentials, and sends an initial heartbeat when spawned by the Gyre server.

## Build

```bash
docker build -t gyre-agent:latest .
```

## Register as a Compute Target

1. Open the Gyre dashboard → Admin → Compute → Add
2. Set type to `container`, image to `gyre-agent:latest`
3. The target will appear in the Spawn Agent modal dropdown

Or via API:
```bash
curl -X POST http://localhost:3000/api/v1/admin/compute-targets \
  -H "Authorization: Bearer gyre-dev-token" \
  -H "Content-Type: application/json" \
  -d '{"name":"local-docker","target_type":"container","config":{"image":"gyre-agent:latest"}}'
```

## Environment Variables

The Gyre server injects these automatically at spawn time — you do not need to set them manually:

| Variable | Description |
|---|---|
| `GYRE_SERVER_URL` | Gyre server base URL |
| `GYRE_AUTH_TOKEN` | Pre-minted EdDSA JWT for API auth |
| `GYRE_CLONE_URL` | Git Smart HTTP URL for the assigned repo |
| `GYRE_BRANCH` | Branch to clone |
| `GYRE_AGENT_ID` | Agent UUID |
| `GYRE_TASK_ID` | Assigned task UUID |
| `GYRE_REPO_ID` | Repository UUID |
| `GYRE_AGENT_COMMAND` | _(optional)_ Command to exec after bootstrap (e.g. a CI script path inside the image) |

### cred-proxy (optional overrides)

The credential proxy reads these at startup; unset values fall back to the
listed defaults:

| Variable | Default | Description |
|---|---|---|
| `GYRE_ANTHROPIC_BASE` | `https://api.anthropic.com` | Upstream for `/v1/*` LLM calls (Anthropic-compatible gateways) |
| `GYRE_PRICING_JSON` | _(built-in table)_ | Cost table override: `{"claude-<model>":[input_per_million_usd, output_per_million_usd], ...}` |
| `GYRE_CRED_SOCK_PATH` | `/run/gyre/cred.sock` | Unix socket path for the primary proxy listener |
| `GYRE_CRED_ALLOWED_HOSTS` | `api.anthropic.com,gitlab.com,api.github.com` | Destination allowlist for `POST /proxy` |

### Usage reporting

After each proxied LLM call, cred-proxy reports token usage back to the
server via `POST $GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/usage`
(authenticated with `GYRE_AUTH_TOKEN`). The server aggregates usage per
workspace and runs the budget enforcement cascade (agent-runtime.md §4:
80% warn / 100% graceful stop / beyond-limit hard kill). Reports are
best-effort and never fail the LLM call itself.

## What the Entrypoint Does

1. Validates that required env vars are set
2. Configures git to use `GYRE_AUTH_TOKEN` as the HTTP password
3. Clones `GYRE_CLONE_URL` branch `GYRE_BRANCH` into `/workspace/repo`
4. Sends an initial heartbeat to `POST /api/v1/agents/$GYRE_AGENT_ID/heartbeat`
5. If `GYRE_AGENT_COMMAND` is set, execs it (the agent takes over from here)
6. Otherwise, sleeps 1 hour (useful for interactive `docker exec` debugging)

## Networking

Agent containers default to `bridge` networking so they can reach the Gyre server. This is intentional — agents need LAN access to clone the repo and send API calls. For gate/validation containers (untrusted code review), use `--network=none`.

## Customizing

Extend this image for language-specific workloads:

```dockerfile
FROM gyre-agent:latest
RUN apt-get install -y nodejs npm
```

Or point to a custom entrypoint via the compute target `config.command` field.
