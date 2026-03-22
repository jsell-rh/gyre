# M19: Container Runtime

**Status:** Done
**Predecessor:** M18 (Agent Identity)
**Successor:** M20 (UI Accountability)

---

## Objective

Wire Docker/Podman as a first-class compute target for agent workloads, with hardened security defaults enforced at the platform layer. Complement container isolation with procfs-based liveness monitoring, workload attestation embedded in JWT claims, and SSH reverse tunnels for air-gapped compute.

---

## Acceptance Criteria

### M19.1 -- ContainerTarget compute adapter

- [x] `ContainerTarget` struct in `crates/gyre-adapters/src/compute/container.rs` implements the `ComputeTarget` port trait
- [x] Auto-detects available runtime: prefers `docker`, falls back to `podman` via `which`; errors if neither found
- [x] `spawn_process` translates `SpawnConfig` to `docker run` / `podman run` invocation with all security flags applied
- [x] `is_alive` checks container status via `docker inspect --format={{.State.Running}}`
- [x] `kill_process` issues `docker rm -f <id>`
- [x] `ProcessHandle.target_type` = `"container"` for attribution in workload attestation
- [x] `GYRE_DEFAULT_COMPUTE_TARGET=local|container` env var selects default at spawn when no `compute_target_id` supplied; defaults to `local` for backwards compatibility (M19.1, PR #276)
- [x] Spawn response includes `container_id` field when agent is launched in a container (M19.1, PR #276)

### M19.2 -- Security defaults (CISO G8-A/B/C)

- [x] **G8-A (MEDIUM):** `--network=none` by default -- no outbound network; opt in via `network` field or `with_network()`
- [x] **G8-B (LOW):** `--memory=2g --pids-limit=512` by default -- prevents OOM and fork bombs; override via `memory_limit`/`pids_limit`
- [x] **G8-C (LOW):** `--user=65534:65534` (nobody:nogroup) by default -- non-root; override via `user` field
- [x] Security defaults enforced in unit tests (G8-A, G8-B, G8-C test coverage in `container.rs`)
- [x] Compute target API (`POST /api/v1/admin/compute-targets`) exposes `network`, `memory_limit`, `pids_limit`, `user` override fields documented in CLAUDE.md

### M19.3 -- Container audit trail + procfs liveness monitor

- [x] `ContainerAuditRecord` captured via `docker inspect` at spawn: `container_id`, `image`, `image_hash`, `spawned_at`; background monitor updates `exited_at`/`exit_code` on container exit (M19.3, PR #276)
- [x] `AgentContainerSpawned` domain event broadcast over WebSocket at spawn (M19.3, PR #276)
- [x] `GET /api/v1/agents/{id}/container` -- container audit record; 404 if agent not container-spawned (M19.3, PR #276)
- [x] `GYRE_PROCFS_MONITOR` env var (default: enabled); set to `false` to disable
- [x] Polls `/proc/{pid}/fd/` and `/proc/{pid}/net/tcp` every 5 s per live agent PID on Linux
- [x] Emits real `FileAccess` and `NetworkConnect` audit events from procfs data
- [x] No-op on non-Linux platforms
- [x] Agent heartbeat (`PUT /api/v1/agents/{id}/heartbeat`) re-checks PID liveness via `/proc/{pid}` and logs warning if process not running (G10)

### M19.4 -- Workload attestation (G10)

- [x] `POST /api/v1/agents/{id}/stack` -- agent self-reports runtime stack fingerprint at spawn
- [x] `GET /api/v1/agents/{id}/stack` -- query registered stack fingerprint
- [x] `GET /api/v1/agents/{id}/workload` -- current workload attestation: `{pid, hostname, compute_target, stack_hash, alive}`; `alive` re-checked via `/proc/{pid}` on Linux
- [x] JWT agent tokens embed workload claims at spawn: `wl_pid`, `wl_hostname`, `wl_compute_target`, `wl_stack_hash` (M18 integration)
- [x] Container-spawned agents additionally embed `wl_container_id` and `wl_image_hash` JWT claims (M19.4, PR #276)
- [x] Heartbeat verifies container liveness via `docker inspect` when agent is container-spawned (M19.4, PR #276)
- [x] Stack fingerprint required for push attestation on repos with `stack-policy` set (`GET/PUT /api/v1/repos/{id}/stack-policy`)

### M19.5 -- SSH compute targets + reverse tunnels (G12)

- [x] `POST /api/v1/admin/compute-targets` accepts `target_type: "ssh"` with `host` field
- [x] `POST /api/v1/admin/compute-targets/{id}/tunnel` -- open SSH tunnel: `{direction: "forward"|"reverse", local_port, remote_port, local_host?, remote_host?}`; reverse tunnels (`-R`) let air-gapped agents dial out through NAT
- [x] `GET /api/v1/admin/compute-targets/{id}/tunnel` -- list active tunnels
- [x] `DELETE /api/v1/admin/compute-targets/{id}/tunnel/{tid}` -- close tunnel (SIGTERM to `ssh -N` process)
- [x] **M19.5-A (MEDIUM):** `agent.name` validated against `[a-zA-Z0-9._-]{1,63}` -- shell metacharacters rejected with 400; SSH+container spawn uses direct docker arg array instead of `sh -c` to prevent command injection (PR #278)

---

## Implementation Notes

- `ContainerTarget` lives in `crates/gyre-adapters/src/compute/container.rs` (hexagonal adapter -- no domain imports)
- Runtime auto-detection uses `which docker` / `which podman` at spawn time (not at server startup), resolving lazily
- `ComputeTarget` port trait in `gyre-ports` is the extension point for future targets (Kubernetes, Firecracker, etc.)
- Container integration test (`container_spawn_is_alive_kill`) is `#[ignore]` in CI -- requires a live Docker/Podman daemon; run manually with `cargo test -- --ignored`

---

## Security Posture

| Finding | Severity | Status |
|---|---|---|
| G8-A: container network isolation | MEDIUM | Closed -- `--network=none` default |
| G8-B: container resource limits | LOW | Closed -- `--memory=2g --pids-limit=512` default |
| G8-C: container non-root | LOW | Closed -- `--user=65534:65534` default |
| G7: procfs agent monitor | LOW | Closed -- procfs polling every 5s on Linux |
| G10: workload attestation | LOW | Closed -- JWT claims + `/workload` endpoint |
| G12: SSH tunnel support | -- | Closed -- forward/reverse tunnel API |
| M19.5-A: shell injection in SSH+container spawn | MEDIUM | Closed -- `agent.name` validated against `[a-zA-Z0-9._-]{1,63}`; direct docker arg array replaces `sh -c` (PR #278) |
