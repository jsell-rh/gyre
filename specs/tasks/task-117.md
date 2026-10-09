---
title: "Implement compute target model"
spec_ref: "agent-runtime.md §3 Compute Target Model"
depends_on: []
progress: not-started
coverage_sections:
  - "agent-runtime.md §3. Compute Target Model"
  - "agent-runtime.md §Abstraction"
  - "agent-runtime.md §Supported Backends"
  - "agent-runtime.md §Nix-Based Image Build"
  - "agent-runtime.md §Tenant and Workspace Configuration"
commits: []
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
