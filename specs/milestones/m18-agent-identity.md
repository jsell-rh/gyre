# M18 — Agent Identity

## Goal

Give every agent a cryptographically verifiable identity: EdDSA JWT tokens issued by the forge's built-in OIDC provider, verifiable by any external party without a shared secret.

## Deliverables

### M18.1 — OIDC Provider

The server acts as a minimal OIDC identity provider for its own agents.

| Endpoint | Description |
|---|---|
| `GET /.well-known/openid-configuration` | OIDC discovery document — issuer URL, JWKS URI, supported algorithms. No auth required. |
| `GET /.well-known/jwks.json` | Ed25519 JWK Set. No auth required. Rotate the key by restarting with a new `GYRE_SIGNING_KEY` (or let the server generate one). |

### M18.2 — EdDSA JWT Agent Tokens

`POST /api/v1/agents/spawn` now returns a signed JWT (`token` field) instead of a UUID token:

- Algorithm: `EdDSA` (Ed25519)
- Claims: `sub` (agent_id), `task_id`, `spawned_by`, `exp` (Unix expiry)
- Starts with `ey` — three dot-separated base64url parts
- Verify offline using the public key from `/.well-known/jwks.json`
- TTL configured via `GYRE_AGENT_JWT_TTL` (default: `3600` seconds)
- Legacy UUID tokens (from `POST /api/v1/agents`) are still accepted for backwards compatibility

### M18.3 — Token Introspection

`GET /api/v1/auth/token-info` — returns the decoded identity of the caller's token:

```json
{
  "kind": "agent_jwt",          // agent_jwt | uuid_token | api_key | global
  "agent_id": "...",
  "task_id": "...",
  "spawned_by": "...",
  "exp": 1234567890
}
```

### M18.4 — JWT Revocation on Complete

When `POST /api/v1/agents/{id}/complete` succeeds, the agent's JWT is revoked in the database. Subsequent API calls with the same token return `401`. Agents must not reuse a token after completing. (See also M13.7.)

### M18.5 — Spec Policy: Stale Spec Detection

Per-repo spec enforcement policy (`GET/PUT /api/v1/repos/{id}/spec-policy`) gained two new fields:

| Field | Default | Behaviour |
|---|---|---|
| `warn_stale_spec` | `false` | Emits `StaleSpecWarning` domain event when an MR's `spec_ref` SHA differs from the current HEAD of the spec file |
| `require_current_spec` | `false` | Blocks the merge queue when the bound spec is stale (requires updated `spec_ref` before merge) |

`StaleSpecWarning` is broadcast over WebSocket to all connected clients when triggered.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `GYRE_AGENT_JWT_TTL` | `3600` | Lifetime in seconds for EdDSA JWT agent tokens. After expiry, token is rejected even if not explicitly revoked. |

## Test Suite

`crates/gyre-server/tests/m18_oidc_integration.rs` — 8 integration tests:

| Test | Coverage |
|---|---|
| OIDC discovery document shape | `/.well-known/openid-configuration` response fields |
| JWKS JWK format | Ed25519 JWK in `/.well-known/jwks.json` |
| JWT spawn token | `POST /agents/spawn` returns `ey...` JWT |
| JWT auth | Spawned JWT accepted on authenticated endpoints |
| Token-info claims | `GET /auth/token-info` returns correct kind + claims |
| JWT revocation after complete | Token rejected after `POST /agents/{id}/complete` |
| Expired JWT rejected | Token with past `exp` returns 401 |
| UUID token still accepted | Legacy tokens from `POST /api/v1/agents` remain valid |
