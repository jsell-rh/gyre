# M26: WireGuard Mesh Networking

**Status:** Draft
**Author:** CISO
**Requested by:** Operator (2026-03-23)

---

## Motivation

Gyre currently implements a WireGuard peer *registry* — agents can register public keys and
endpoints via `POST /api/v1/network/peers`, and the data is persisted. However, no actual
WireGuard tunnel is ever established: no `wg` interface is created, no key exchange occurs,
and no traffic flows through an encrypted mesh.

The gap between the registry and a real mesh has concrete security consequences:

- Agent-to-agent communication is unencrypted (falls back to plain HTTP or TLS-only if configured)
- The registry has no ownership enforcement — any authenticated client can register any `agent_id`'s pubkey
- `last_seen` is never updated (no heartbeat integration)
- The DERP relay map returns an empty nodes list — NAT traversal is non-functional
- G12 (air-gapped connectivity) was closed via SSH reverse tunnels, which work but provide
  no peer-to-peer agent communication path

This milestone delivers a real WireGuard mesh: agents register keys, the server coordinates
key distribution and DERP relay configuration, and agents form encrypted tunnels.

---

## Goals

### M26.1 — WireGuard kernel integration (server-side coordination)

The server becomes a WireGuard *coordination plane* (not a relay). It:

- Validates pubkeys on registration (must be valid 32-byte Curve25519 base64)
- Enforces that each agent may only register/update its own pubkey (ownership check)
- Distributes the full peer list to agents on request (`GET /api/v1/network/peers` already exists)
- Updates `last_seen` on every heartbeat (`PUT /api/v1/agents/{id}/heartbeat` calls
  `network_peers.touch(agent_id)`)
- Issues short-lived WireGuard *allowed-IPs* allocations from a configured CIDR pool
  (`GYRE_WG_CIDR`, default `10.100.0.0/16`)

**New env vars:**

| Variable | Default | Description |
|---|---|---|
| `GYRE_WG_ENABLED` | `false` | Enable WireGuard coordination plane |
| `GYRE_WG_CIDR` | `10.100.0.0/16` | IP pool for agent mesh addresses |
| `GYRE_WG_SERVER_PUBKEY` | _(required if enabled)_ | Server's WireGuard public key |
| `GYRE_WG_SERVER_ENDPOINT` | _(required if enabled)_ | Server's WireGuard endpoint (`host:port`) |

**API changes:**

- `POST /api/v1/network/peers` — enforce caller is the agent being registered (JWT `sub` must
  match `agent_id`); return allocated mesh IP in response
- `GET /api/v1/network/peers` — include `mesh_ip` field per peer for route configuration
- `PUT /api/v1/network/peers/{id}` — allow agents to update their own endpoint (roaming)

### M26.2 — Agent-side WireGuard setup (entrypoint integration)

Update `docker/gyre-agent/entrypoint.sh` to:

1. On startup, generate a WireGuard keypair (`wg genkey | tee privkey | wg pubkey > pubkey`)
2. Register the pubkey with the server (`POST /api/v1/network/peers`)
3. Fetch the full peer list and configure `wg0` interface
4. Establish tunnels to all active peers

The private key is generated locally and never sent to the server. The server only ever sees
the public key.

Requires `wireguard-tools` in the container image.

### M26.3 — DERP relay for NAT traversal

Replace the stub `derp_map` endpoint with a real DERP server configuration:

- Support external DERP servers via `GYRE_DERP_SERVERS` env var
  (JSON array of `{region_id, region_name, nodes: [{name, host_name, ipv4, stun_port, derp_port}]}`)
- If no external DERP configured, document that agents must have direct connectivity
  or use the existing SSH tunnel mechanism (G12) as a fallback

The `GET /api/v1/network/derp-map` endpoint returns real DERP configuration instead of
the empty stub.

### M26.4 — Security hardening

Close the gaps identified in the CISO audit (2026-03-23):

- **Ownership enforcement:** JWT bearer must match `agent_id` on peer register/update/delete.
  Non-JWT callers (global token, API key) require Admin role.
- **Pubkey validation:** Reject registrations with invalid Curve25519 keys (not 44-char base64
  encoding of 32 bytes). Prevents junk data in the registry.
- **last_seen heartbeat:** `PUT /api/v1/agents/{id}/heartbeat` calls
  `state.network_peers.update_last_seen(agent_id, now)` when a peer record exists.
- **Peer expiry:** Background job marks peers as `stale` if `last_seen` is older than
  `GYRE_WG_PEER_TTL` (default 300s). Stale peers are excluded from the distributed peer list
  but retained in the DB for audit purposes.

---

## Non-Goals

- Running a WireGuard relay server within Gyre itself (use external DERP or SSH tunnels)
- Key rotation automation (agents re-register on restart; TTL handles cleanup)
- IPv6 mesh addresses (IPv4 only for M26)
- Integration with Tailscale or Headscale (possible future milestone)

---

## Security Considerations

- WireGuard private keys are generated and held exclusively by each agent process; the server
  stores only public keys
- Mesh IPs are allocated by the server from the configured CIDR and cannot be self-assigned
  by agents (prevents IP spoofing within the mesh)
- `GYRE_WG_ENABLED=false` by default — existing deployments are unaffected
- The coordination endpoints follow the existing RBAC/ABAC model; no new auth surface

---

## Acceptance Criteria

- [ ] Agent registers WireGuard pubkey on spawn; allocated mesh IP returned in spawn response
- [ ] `GET /api/v1/network/peers` returns `mesh_ip` field per peer
- [ ] `last_seen` updates on each agent heartbeat when a peer record exists
- [ ] Peer registration rejected if JWT `sub` does not match `agent_id`
- [ ] Invalid Curve25519 pubkeys return 400
- [ ] Stale peer detector marks peers inactive after `GYRE_WG_PEER_TTL` seconds
- [ ] `GET /api/v1/network/derp-map` returns configured DERP regions (not empty stub)
- [ ] `GYRE_WG_ENABLED=false` (default) leaves existing behaviour unchanged
- [ ] All new paths covered by integration tests

---

## Open Questions for Review

1. Should M26 include the agent-side `wg0` interface setup (M26.2), or ship coordination-plane
   only first and tackle agent-side in a follow-up?
2. Should we integrate with an existing DERP server (e.g. Tailscale's public DERP), or
   require operators to run their own?
3. Priority relative to M25 security fixes (M25-A/B/C findings) — those may need to land first.
