# M27: Credential Opacity — Unix Socket Credential Server

**Status:** Draft
**Author:** CISO
**Requested by:** Operator (2026-03-23) — "approach 3"

---

## Motivation

M25 introduced `GYRE_AGENT_CREDENTIALS` which injects `ANTHROPIC_API_KEY` and other secrets
directly into the container environment. Claude (and any AI agent) can trivially read
`process.env.ANTHROPIC_API_KEY`, and the value may appear in the model's context window,
logs, or outputs.

This is a fundamental problem: even if we trust the *intent* of the agent, credentials that
appear as plaintext strings in the process environment will eventually leak — through debug
output, error messages, tool results, or model reasoning traces.

The operator identified this risk and selected **approach 3: Unix socket credential server**
as the solution, specifically because some credentials (e.g. GitLab personal access tokens)
cannot be scoped — the blast radius of exposure is the full token capability. The goal is
that agents can *use* credentials to make authenticated calls, but can never *read* the
credential value as a string.

Gyre must support multiple AI providers, each with different credential shapes:

| Provider | Env vars / credential type |
|---|---|
| Anthropic Direct API | `ANTHROPIC_API_KEY` (static key, `x-api-key` header) |
| Vertex AI (Google Cloud) | `GOOGLE_APPLICATION_CREDENTIALS` (service account JSON file) + `ANTHROPIC_VERTEX_PROJECT_ID` + `ANTHROPIC_VERTEX_REGION` — OAuth2 token exchange required |
| GitLab | `GITLAB_TOKEN` (`PRIVATE-TOKEN` header) |
| GitHub | `GITHUB_TOKEN` (`Authorization: token` header) |

Vertex AI is particularly complex: the credential is a service account JSON file, and the
SDK performs an OAuth2 exchange to get short-lived access tokens. This cannot be handled by
simple header injection — the proxy must either emulate the GCE metadata server or perform
the OAuth2 exchange on behalf of the agent.

---

## Design: Unix Socket Credential Proxy

### Architecture

```
┌─────────────────────────────────────────────────────┐
│  Container                                           │
│                                                      │
│  ┌──────────────────┐      ┌──────────────────────┐  │
│  │  Claude Agent    │      │  cred-proxy sidecar  │  │
│  │  (UID 65534)     │─────▶│  (UID 0 / separate)  │  │
│  │                  │ HTTP │  - holds all secrets  │  │
│  │  GYRE_CRED_PROXY │      │  - adds auth headers │  │
│  │  =http+unix://   │      │  - proxies to target  │  │
│  │  /run/gyre/      │      │  - audits every call  │  │
│  │  cred.sock       │◀─────│                      │  │
│  └──────────────────┘      └──────────────────────┘  │
│                                                      │
│  /run/gyre/cred.sock  (chmod 660, owned by proxy UID)│
└─────────────────────────────────────────────────────┘
```

The agent process (Claude Code / agent-runner.mjs) is given:
- `GYRE_CRED_PROXY=http+unix:///run/gyre/cred.sock` — address of the proxy
- No raw credential values in environment

The sidecar credential proxy:
- Is started by the entrypoint *before* dropping privileges to UID 65534
- Holds all secrets in memory (from a mounted secret file or env vars visible only to UID 0)
- Listens on a Unix domain socket owned by a non-agent UID
- Accepts HTTP CONNECT requests from the agent and proxies them, injecting appropriate credentials
- Logs every request (URL, method, timestamp) for audit

### Secret injection into the proxy (not the agent)

The server injects credentials into a tmpfs-mounted secret file rather than env vars:

```
/run/secrets/gyre-creds   (tmpfs, mode 0400, owned by root)
```

Content (newline-delimited key=value, never comma-separated):
```
ANTHROPIC_API_KEY=sk-ant-...
GITLAB_TOKEN=glpat-...
```

The agent entrypoint reads this file *as root*, loads it into the proxy process, then
`shred`s the file before `exec`-ing the agent as UID 65534. The file never exists when the
agent process is running.

### What the agent sees

```bash
# Agent environment (no secrets)
GYRE_SERVER_URL=http://gyre-server:3000
GYRE_AUTH_TOKEN=<jwt>          # still needed for Gyre API; handled separately (see below)
GYRE_CRED_PROXY=http+unix:///run/gyre/cred.sock
GCE_METADATA_HOST=localhost:8080   # set when PROVIDER=vertex; SDK calls this for OAuth2 tokens
ANTHROPIC_VERTEX_PROJECT_ID=my-gcp-project   # non-secret; safe in env
ANTHROPIC_VERTEX_REGION=us-east5             # non-secret; safe in env
GYRE_BRANCH=feat/my-feature
GYRE_TASK_ID=<uuid>
# NOTE: ANTHROPIC_API_KEY, GOOGLE_APPLICATION_CREDENTIALS, GITLAB_TOKEN etc. are ABSENT
```

The `GCE_METADATA_HOST` variable is only injected when the operator configures a Vertex AI
provider. The Anthropic SDK and Google Cloud SDK automatically use this endpoint instead of
the real GCE metadata service when the variable is set.

### GYRE_AUTH_TOKEN handling (Gyre JWT)

The Gyre JWT (`GYRE_AUTH_TOKEN`) is already short-lived (configurable via `GYRE_AGENT_JWT_TTL`,
default 3600s). As an interim measure, reduce default to 300s immediately.

For full opacity: the proxy also handles Gyre API calls — the agent sends unauthenticated
requests to `http+unix:///run/gyre/cred.sock/api/v1/...` and the proxy adds the Gyre JWT.
The agent env carries only the socket path, not the token.

---

## Implementation Plan

### M27.1 — cred-proxy binary (multi-provider)

Add `docker/gyre-agent/cred-proxy/` containing a minimal Rust or Go HTTP proxy with
provider-aware credential injection:

**Anthropic Direct API:**
- Intercepts requests to `api.anthropic.com`
- Injects `x-api-key: <ANTHROPIC_API_KEY>` header from secret store

**Vertex AI (Google Cloud):**
- Implements a minimal GCE metadata server emulator on `localhost:8080`
  (path: `GET /computeMetadata/v1/instance/service-accounts/default/token`)
- Agent env carries `GCE_METADATA_HOST=localhost:8080` — the Anthropic Vertex SDK and
  Google Cloud SDK call this endpoint automatically to obtain OAuth2 tokens
- The proxy holds the service account JSON, performs the OAuth2 exchange with
  `accounts.google.com`, and returns short-lived access tokens (cached, refreshed before expiry)
- `ANTHROPIC_VERTEX_PROJECT_ID` and `ANTHROPIC_VERTEX_REGION` remain in agent env
  (these are non-secret configuration values, not credentials)
- The service account JSON file is never written to any agent-accessible path

**GitLab / GitHub / generic token:**
- Intercepts by URL prefix
- Injects appropriate auth header per provider config in `/run/secrets/gyre-routes`

**Common proxy behaviour:**
- Listens on a Unix domain socket (`/run/gyre/cred.sock`) for HTTP requests
- Also listens on `localhost:8080` for GCE metadata emulation (Vertex AI)
- Logs: `{timestamp, method, url_prefix, status_code}` — no credential values ever logged
- Rejects `CONNECT` tunnels (prevents MITM of proxy itself)
- Enforces allowlist of permitted destination hosts (configurable via `GYRE_CRED_ALLOWED_HOSTS`)
- Enforces per-provider rate limits to detect runaway agents

### M27.2 — Secret file injection (server-side)

Modify `spawn.rs` container launch:

- Write credentials to a tmpfs secret file instead of env vars
- Mount the file into the container at `/run/secrets/gyre-creds` (read-only, UID 0)
- Remove `GYRE_AGENT_CREDENTIALS` env var injection entirely
- Add `GYRE_CRED_PROXY=http+unix:///run/gyre/cred.sock` to agent env

### M27.3 — Entrypoint update

Update `docker/gyre-agent/entrypoint.sh`:

1. Start `cred-proxy` as root (reads secret file, creates socket)
2. `shred -u /run/secrets/gyre-creds` (destroy secret file)
3. `exec` agent as UID 65534 (cannot read root-owned socket creation artifacts)

### M27.4 — agent-runner.mjs update

Update `docker/gyre-agent/agent-runner.mjs` to route API calls through
`GYRE_CRED_PROXY` when set, instead of using `GYRE_AUTH_TOKEN` directly:

```js
// Before: direct token use
headers: { Authorization: `Bearer ${token}` }

// After: route through proxy (no token in agent process)
const proxyUrl = process.env.GYRE_CRED_PROXY;
// fetch() with unix socket support via undici or node-fetch with agent
```

### M27.5 — GYRE_AGENT_JWT_TTL default reduction

Immediate config change (no code): reduce `GYRE_AGENT_JWT_TTL` default from `3600` to `300`.

---

## Security Properties Achieved

| Property | Before M27 | After M27 |
|---|---|---|
| ANTHROPIC_API_KEY readable by agent | ✅ yes (env) | ❌ no |
| GitLab token readable by agent | ✅ yes (env) | ❌ no |
| Gyre JWT readable by agent | ✅ yes (env) | ❌ no (socket only) |
| Credential appears in agent logs | possible | no (proxy never logs values) |
| Credential in model context window | possible | no |
| Audit trail of credential use | no | yes (proxy logs every call) |

---

## Non-Goals

- Zero-trust between agent and proxy (agent and proxy share a container; the threat model
  is credential opacity from the *model*, not from a compromised container process)
- Kernel keyring integration (deferred; Unix socket is sufficient for container workloads)
- Secret rotation mid-run (agent gets one set of credentials per spawn)

---

## Acceptance Criteria

- [ ] Container env contains no raw credential values (ANTHROPIC_API_KEY, GitLab tokens, etc.)
- [ ] Agent makes successful Anthropic API calls via proxy without reading the key
- [ ] `shred` of secret file completes before agent process starts
- [ ] Proxy audit log records every outbound call (URL prefix + status, no secret values)
- [ ] `GYRE_AGENT_CREDENTIALS` env var injection removed from spawn.rs
- [ ] `GYRE_AGENT_JWT_TTL` default reduced to 300s
- [ ] Integration test: agent process cannot read /run/secrets/gyre-creds (file absent)
- [ ] Integration test: agent API calls succeed via proxy

---

## Open Questions for Review

1. Should the proxy allowlist destination hosts by default (secure) or denylist (flexible)?
2. Should M27 ship before or concurrently with M26 WireGuard?
3. The `GYRE_AUTH_TOKEN` in `agent-runner.mjs` MCP headers — should the SDK support
   unix-socket proxies, or should we patch the agent runner to use the proxy for Gyre calls?
