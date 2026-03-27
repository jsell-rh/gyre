# Gyre Server Configuration

## Running the Server

```bash
# Dev mode (defaults: port 3000, token gyre-dev-token, in-memory DB)
cargo run -p gyre-server

# With custom settings
GYRE_PORT=8080 GYRE_AUTH_TOKEN=my-token GYRE_DATABASE_URL=sqlite:///tmp/gyre.db RUST_LOG=debug \
  cargo run -p gyre-server

# Release build
cargo build --release -p gyre-server && ./target/release/gyre-server
```

Access at `http://localhost:3000`. Default token `gyre-dev-token` works against a local dev server.

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GYRE_PORT` | `3000` | TCP port to listen on |
| `GYRE_AUTH_TOKEN` | `gyre-dev-token` | Bearer token clients must send to authenticate |
| `GYRE_BASE_URL` | `http://localhost:<port>` | Public base URL (used in clone URLs returned by spawn API) |
| `GYRE_LOG_FORMAT` | _(human-readable)_ | Set to `json` for structured JSON log output (M4.1) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | _(disabled)_ | OTLP/gRPC collector URL, e.g. `http://otel-collector:4317` (M4.1) |
| `GYRE_OIDC_ISSUER` | _(disabled)_ | Keycloak realm URL, e.g. `http://keycloak:8080/realms/gyre` — enables JWT auth (M4.2) |
| `GYRE_OIDC_AUDIENCE` | _(none)_ | Optional JWT audience claim for Keycloak token validation (M4.2) |
| `RUST_LOG` | `info` | Log level filter (e.g. `debug`, `gyre_server=trace`) |
| `GYRE_SNAPSHOT_PATH` | `./snapshots/` | Directory for DB snapshot files (`POST /api/v1/admin/snapshot`) |
| `GYRE_MAX_BODY_SIZE` | `10485760` (10 MB) | Maximum HTTP request body size in bytes (M7.3) |
| `GYRE_CORS_ORIGINS` | `http://localhost:3000,...` | Comma-separated allowed CORS origins. Default: localhost:2222, localhost:3000, localhost:5173 **plus `http://localhost:{GYRE_PORT}` appended automatically when not already present**. Set to `*` to allow all (not recommended for production). (M7.3, M-5) |
| `GYRE_AGENT_JWT_TTL` | `300` | Lifetime in seconds for EdDSA JWT agent tokens issued by `POST /api/v1/agents/spawn`. After expiry, token is rejected even if not explicitly revoked. Reduced from 3600 to 300 in M27.5. (M18, M27) |
| `GYRE_SIGSTORE_MODE` | `local` | Commit signing backend for `jj squash`: `local` signs with the forge's Ed25519 key; `fulcio` is reserved for future external Fulcio CA integration (logs a warning, does not block). (M13.8) |
| `GYRE_TRUSTED_ISSUERS` | _(disabled)_ | Comma-separated base URLs of trusted remote Gyre instances (e.g. `https://gyre-2.example.com`). Enables G11 federation: JWTs minted by these instances are verified via remote OIDC discovery + JWKS (cached 5 min). Federated agents receive `Agent` role; `agent_id = "<remote-host>/<sub>"`. (G11) |
| `GYRE_RATE_LIMIT` | `100` | Requests per second allowed per IP before 429 (M7.3) |
| `GYRE_AUDIT_SIMULATE` | _(disabled)_ | Set to `true` to run the audit event simulator on startup (M7.1) |
| `GYRE_DEFAULT_COMPUTE_TARGET` | `local` | Default compute target type when no `compute_target_id` is supplied on spawn: `local` (subprocess) or `container` (Docker/Podman with G8 security defaults); requires Docker or Podman on `PATH` when set to `container` (M19.1) |
| `GYRE_PROCFS_MONITOR` | _(enabled)_ | Set to `false` to disable the procfs-based agent process monitor (G7). Polls `/proc/{pid}/fd/` and `/proc/{pid}/net/tcp` every 5 s per live agent PID; emits real `FileAccess` and `NetworkConnect` audit events. No-op on non-Linux platforms. |
| `GYRE_REPOS_PATH` | `./repos/` | Directory for bare git repositories on disk. Created on startup if absent. (M10.3) |
| `GYRE_GIT_PATH` | `git` | Path to the `git` binary. Defaults to `git` (resolved via `PATH`). Override for NixOS/container environments where git is at a fixed store path (e.g. `/nix/store/.../bin/git`). Used by smart HTTP handlers, merge processor, and spec lifecycle hooks. |
| `GYRE_DATABASE_URL` | _(unset -- in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |
| `GYRE_SCIM_TOKEN` | _(unset -- SCIM disabled)_ | Bearer token SCIM clients must send to `/scim/v2/` endpoints. When unset, SCIM provisioning endpoints return 401. Separate from `GYRE_AUTH_TOKEN`. (M23) |
| `GYRE_RTO` | _(unset)_ | Recovery Time Objective in seconds; returned by `GET /api/v1/admin/bcp/targets` (M23) |
| `GYRE_RPO` | _(unset)_ | Recovery Point Objective in seconds; returned by `GET /api/v1/admin/bcp/targets` (M23) |
| `GYRE_AGENT_CREDENTIALS` | _(unset)_ | Comma-separated `KEY=value` pairs injected into every container agent spawn (e.g. `ANTHROPIC_API_KEY=sk-ant-xxx`). **M27:** credentials are injected as `GYRE_CRED_KEY=value` and held by the `cred-proxy` sidecar -- raw values are never in the agent process env. Anthropic API calls are routed through the proxy via `ANTHROPIC_BASE_URL`. On startup, if Docker/Podman is on `PATH`, the server auto-registers a `gyre-agent-default` container compute target. (M25, M27) |
| `GYRE_AGENT_GCP_SA_JSON` | _(unset)_ | GCP service account JSON (full JSON string) for Vertex AI provider. Injected as `GYRE_CRED_GCP_SA_JSON` and held by `cred-proxy` which emulates the GCE metadata server on `127.0.0.1:8080` for OAuth2 token exchange. Agent env gets `GCE_METADATA_HOST=127.0.0.1:8080`. (M27) |
| `GYRE_CRED_ALLOWED_HOSTS` | `api.anthropic.com,gitlab.com,api.github.com` | Comma-separated allowlist of destination hostnames the `cred-proxy` sidecar will forward requests to. `POST /proxy` calls to unlisted hosts receive 403. Prevents SSRF via the credential proxy. (M27-A) |
| `GYRE_AGENT_INBOX_MAX` | `100` | Max unacked `Directed`-tier messages queued per agent before new sends return 429. (Message Bus Phase 3) |
| `GYRE_TELEMETRY_BUFFER_SIZE` | `10000` | In-memory ring buffer size for `Telemetry`-tier messages (replaced ActivityStore). (Message Bus Phase 4) |
| `GYRE_TELEMETRY_MAX_WORKSPACES` | `100` | Max number of workspaces tracked in the telemetry ring buffer. Oldest entries evicted when exceeded. |
| `GYRE_WG_ENABLED` | `false` | Enable WireGuard mesh coordination plane. When `true`, `POST /api/v1/network/peers` allocates mesh IPs and the stale peer detector runs. (M26) |
| `GYRE_WG_CIDR` | `10.100.0.0/16` | CIDR block for mesh IP allocation. The server claims `.1`; agents receive `.2`, `.3`, sequentially. (M26) |
| `GYRE_WG_SERVER_PUBKEY` | _(unset)_ | Server WireGuard public key (Curve25519, base64). Included in `GET /api/v1/network/peers` so agents can add the server as a peer. (M26) |
| `GYRE_WG_SERVER_ENDPOINT` | _(unset)_ | Server WireGuard endpoint `host:port` returned alongside `server_pubkey`. (M26) |
| `GYRE_WG_PEER_TTL` | `300` | Seconds of inactivity before a peer is marked stale and filtered from the peer list. Stale peer detector runs every 60 s. (M26) |
| `GYRE_DERP_SERVERS` | _(unset)_ | JSON array of DERP relay server configs served by `GET /api/v1/network/derp-map`. (M26) |
| `GYRE_DERP_URL` | _(unset)_ | URL to fetch the DERP relay map JSON (used when `GYRE_DERP_SERVERS` is unset). (M26) |
| `GYRE_DIVERGENCE_THRESHOLD` | `3` | Minimum number of conflicting node changes across agent deltas for the same spec before a `ConflictingInterpretations` (priority-5) inbox notification is emitted. One notification per spec_ref per push, sent to all Admin and Developer workspace members. Set to a higher value to reduce noise in large workspaces. (HSI §8) |
| `GYRE_VERTEX_PROJECT` | _(unset — LLM disabled)_ | Google Cloud project ID for Vertex AI LLM calls. When unset, all LLM endpoints return 503 with a hint to configure. Set alongside `GOOGLE_APPLICATION_CREDENTIALS` (service account JSON path) or use `GYRE_AGENT_GCP_SA_JSON` for credential injection. (LLM integration) |
| `GYRE_LLM_MODEL` | `gemini-2.0-flash` | Default Vertex AI model for LLM functions when no workspace or tenant override is configured. Overridden per-function via `PUT /api/v1/workspaces/{id}/llm/config/{function}`. (LLM integration) |
