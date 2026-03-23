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
