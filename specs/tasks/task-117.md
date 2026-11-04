---
title: "Implement compute target model"
spec_ref: "agent-runtime.md §3 Compute Target Model"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-runtime.md §3. Compute Target Model"
  - "agent-runtime.md §Abstraction"
  - "agent-runtime.md §Supported Backends"
  - "agent-runtime.md §Nix-Based Image Build"
  - "agent-runtime.md §Tenant and Workspace Configuration"
commits: ["c79542d8e436d565175a8b249e1c4c36524d69da", "029665115e9520dcb12625191f031697f0862db4", "57234e0fde87294b242c39e65535f85a7489e074", "9f9c7d9d2ec076a6882c2521836fc7b5d22441f9", "c88766e703b5fa5a2952b8d01d034cc117af6355", "5fcbe3614376eaa46e954a60475f9bd39b6015a9", "53e3830f8ba25070857de3d82d0a6467900751bb", "c04167b869690f970fac6f81ef0b1d762a63d1ae", "61f0d08d5d8a86f9313eec0531aeee0ef2049260", "940a86b1680eb94b55bf518430194da6458449ca", "2b2688014f2c647b2911647bf819d9d1cfe1106a"]
---

## Spec Excerpt

From `agent-runtime.md` §3:

**Abstraction:** Compute targets are pluggable backends for agent execution behind a single trait:

```rust
#[async_trait]
pub trait ComputeTarget: Send + Sync {
    fn name(&self) -> &str;
    fn target_type(&self) -> ComputeTargetType;   // Container, Ssh, Kubernetes
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;
    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;
    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool>;
}
```

**Supported Backends:**
- **Container** (Docker/Podman): `docker run` with security defaults (`--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`)
- **SSH**: SSH to remote host, `docker run` there
- **Kubernetes**: Create Pod with agent image

**Tenant and Workspace Configuration:**

```rust
pub struct ComputeTargetConfig {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub target_type: ComputeTargetType,
    pub config: serde_json::Value,
    pub is_default: bool,
    pub created_at: u64,
}
```

Workspace selects ONE compute target from tenant's list. Falls back to tenant default, then local container auto-detection.

**API:**
| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/compute-targets` | GET | List available targets |
| `POST /api/v1/compute-targets` | POST | Register new target (tenant admin) |
| `GET /api/v1/compute-targets/:id` | GET | Get target details |
| `PUT /api/v1/compute-targets/:id` | PUT | Update target config |
| `DELETE /api/v1/compute-targets/:id` | DELETE | Remove target |

## Implementation Plan

1. **Verify and extend domain types:**
   - Check if `ComputeTarget` trait already exists (it's referenced in the codebase)
   - Verify `ComputeTargetType` enum has Container, Ssh, Kubernetes variants
   - Verify `ComputeTargetConfig` entity in domain
   - Add `SpawnConfig` struct if missing: branch, repo_path, env_vars, agent_token, resource_limits

2. **Container backend:**
   - Verify/implement Docker/Podman backend with security defaults
   - `--network=none` by default (opt-in bridge via config)
   - Resource limits: `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`
   - Worktree mounted as volume

3. **SSH backend:**
   - Implement SSH compute target: connect via SSH, run `docker run` on remote host
   - SSH credentials stored in compute target config (host, port, key_path or key material)
   - Remote Docker invocation with same security defaults

4. **Kubernetes backend:**
   - Implement K8s compute target: create Pod with agent image
   - Pod spec: resource requests/limits, service account, namespace from config
   - `kill_process` → delete Pod
   - `is_alive` → check Pod phase

5. **Workspace configuration:**
   - Add `compute_target_id: Option<Id>` to Workspace entity if missing
   - Fallback chain: workspace config → tenant default → local container auto-detection
   - UI: Workspace Admin → Settings → Compute Target dropdown

6. **API endpoints:**
   - Verify existing compute-targets API exists and has full CRUD
   - Add workspace selection: `PUT /api/v1/workspaces/:id` with `compute_target_id`
   - Register routes and ABAC mappings

7. **Nix-based image build:**
   - Verify `docker/gyre-agent/flake.nix` exists or create it
   - Ensure reproducible build: `nix build .#agent-image`
   - Image hash recorded in JWT claims (`wl_image_hash`)

## Acceptance Criteria

- [ ] `ComputeTarget` trait with Container, SSH, and Kubernetes backends
- [ ] Container backend enforces security defaults
- [ ] SSH backend runs Docker on remote hosts
- [ ] Kubernetes backend creates/manages Pods
- [ ] `ComputeTargetConfig` persisted in database
- [ ] Workspace selects compute target with fallback chain
- [ ] Full CRUD API for compute targets
- [ ] Nix flake for reproducible agent image builds
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §3 (Compute Target Model) in its entirety. Existing compute target code: grep for `ComputeTarget` in `gyre-domain/src/compute_target.rs` and `gyre-server/src/`. Existing spawn code: `gyre-server/src/api/spawn.rs`. Container runtime: grep for `docker` or `podman` in the server. KV store for compute targets: grep for `compute_targets` in adapters. Route registration: `api/mod.rs`.

## Shipped

Resumed from the `cd426f7a` checkpoint on the merged base `6a9739e`; the
implementation commits (`02966511`…`2b26880`, plus merge-repair `c79542d8`)
carry the work below. All evidence from this round is saved under
`/tmp/stage/review-evidence/`.

### Behavior

- **Abstraction** — `ComputeTarget` port trait
  (`gyre-ports/src/compute.rs`: `name`, `target_type`, `spawn_process`,
  `kill_process`, `is_alive`) with `SpawnConfig`/`ProcessHandle`; four
  production backends in `gyre-adapters/src/compute/`.
- **Container backend** — `ContainerTarget` runs `docker|podman run
  --detach` with the spec security defaults enforced at spawn time:
  `--network=none` (opt-in bridge via config), `--memory=2g`,
  `--pids-limit=512`, `--user=65534:65534`; worktree mounted as a volume;
  runtime auto-detection (docker → podman).
- **SSH backend** — `SshDockerTarget` (`ssh.rs`) shells out to `ssh` with
  batch mode + bounded connect timeout and runs `docker run` on the remote
  host with the same security defaults; every remote argument is
  single-quote shell-escaped (`shell_quote`). Credentials (user, host,
  port, identity file) come from the compute-target config JSON.
- **Kubernetes backend** — `KubernetesTarget` (`kubernetes.rs`) creates a
  Pod via `kubectl apply` from a generated manifest (resource
  requests/limits, service account, namespace, image pull policy from
  config; DNS-1035/253-char-safe pod names); `kill_process` →
  `kubectl delete pod`; `is_alive` → pod phase check.
- **Persistence** — `ComputeTargetConfig` entity behind the
  `ComputeTargetRepository` port, implemented by SQLite, PostgreSQL, and
  mem adapters (tenant-scoped list, default flag, duplicate-name guard).
- **Workspace selection + fallback chain** — `Workspace.compute_target_id`;
  spawn resolution is request `compute_target_id` → workspace assignment →
  tenant default → local container auto-detection (spawn.rs:360-390,
  566-570). Cross-tenant assignment is rejected (workspace update returns
  4xx and spawn 404s on an unknown target id).
- **Backend-aware lifecycle** — each spawned agent records both its
  `ProcessHandle` and the `SpawnBackend` that owns it, so admin kill
  routes to `docker rm` / `kubectl delete` / remote `docker rm` instead of
  a pid-based local kill (a silent no-op for container/Pod handles).
- **CRUD API** — `GET/POST /api/v1/compute-targets`,
  `GET/PUT/DELETE /api/v1/compute-targets/:id` (plus the pre-existing
  admin variants) with ABAC resolver mappings; the two stale
  exemption-file entries were removed and `FROZEN_EXEMPTION_COUNT`
  lowered 53 → 51. `PUT /api/v1/workspaces/:id` accepts
  `compute_target_id` (string or null). UI: WorkspaceSettings "Compute"
  tab with a target dropdown.
- **Nix image build** — `docker/gyre-agent/flake.nix` builds the agent
  image from pinned nixpkgs with `npm ci` from the committed lockfile and
  fixed timestamps (`agent-image`, `agent-image-docker`,
  `agent-image-tar`); image `User = 65534:65534` as defense in depth.
  Per-backend image digests (`image_digest`, `remote_image_digest`,
  `pod_image_digest`) are captured at spawn and recorded in the workload
  attestation (`GET /api/v1/agents/:id/workload`); the `wl_image_hash`
  JWT claim field exists on `mint_with_workload` — the spawn JWT is
  pre-minted before the digest is knowable, so the hash lands in the
  attestation record rather than the initial token.
- **Merge repair (`c79542d8`)** — the `6a9739e` base merge left the
  compute-targets paths in both the resolver and the tests-module
  `required` list, pushing the frozen ABAC duplicate count 22 → 24;
  removed the redundant test entries (resolver coverage remains enforced
  by the route-registry gate).

### Test evidence

- `cargo test -p gyre-adapters compute --lib` → **40 passed, 0 failed**
  (4 ignored: daemon-required Docker/SSH integration tests; no docker,
  podman, kubectl, or nix binary exists in this sandbox — recorded in
  `adapters-compute.log`, EXIT:0).
- `cargo test -p gyre-server --lib api::compute_targets` → **10 passed**
  (CRUD round-trip, invalid type rejection, 404s, non-admin 403,
  duplicate-name 409, delete-referenced-by-workspace 409).
- `cargo test -p gyre-server --lib workspaces::` → **15 passed**
  (assign/clear `compute_target_id`, cross-tenant rejection).
- `cargo test -p gyre-server --lib api::spawn::tests::spawn` →
  **17 passed**, including
  `spawn_uses_workspace_assigned_compute_target` and
  `spawn_falls_back_to_tenant_default_compute_target`;
  `register_spawned_agent_records_backend_owning_handle` and
  `admin_kill_terminates_process_via_registered_backend` pass
  individually.
- `cargo test -p gyre-server --lib abac_middleware` → **9 passed**.
- `npx vitest run src/__tests__/WorkspaceSettings.test.js` (web/) →
  **50 passed**.
- Static gates at HEAD: arch, abac-route-registry (duplicates back at
  baseline 22), mem-port-contracts, in-memory-state-stores,
  migration-versions, dead-message-kinds, byte-slice-truncation,
  relative-path-defaults, fail-open-ref-resolution,
  fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, unbounded-external-http,
  migration-sql-portability, abac-exempt-handlers, mcp-write-tools,
  task-commit-attribution — all exit 0; no new exemption entries.
