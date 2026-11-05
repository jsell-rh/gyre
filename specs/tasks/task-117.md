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
commits: ["c79542d8e436d565175a8b249e1c4c36524d69da", "029665115e9520dcb12625191f031697f0862db4", "57234e0fde87294b242c39e65535f85a7489e074", "9f9c7d9d2ec076a6882c2521836fc7b5d22441f9", "c88766e703b5fa5a2952b8d01d034cc117af6355", "5fcbe3614376eaa46e954a60475f9bd39b6015a9", "53e3830f8ba25070857de3d82d0a6467900751bb", "c04167b869690f970fac6f81ef0b1d762a63d1ae", "61f0d08d5d8a86f9313eec0531aeee0ef2049260", "940a86b1680eb94b55bf518430194da6458449ca", "2b2688014f2c647b2911647bf819d9d1cfe1106a", "976b339afcd1ef253c5819251184097adaa1d917", "7e672231f6e04d027607064a0eef88196c577dd5", "1fc4470b33cfa686d066c1d08d69a83bf846e94c"]
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

Resumed from the `cd426f7a` checkpoint; implementation commits
(`02966511`…`2b26880`, merge repair `c79542d8`), the `6a9739e`/`722179f0`
base merges (`976b339a`, `7e672231`), and the review-repair commit
`1fc4470b` carry the work below. Evidence for this round is saved under
`/tmp/stage/review-evidence/` (source SHAs, per-suite logs with exit codes,
enforced-gate log, rustfmt/clippy diff checks).

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
  `kubectl delete pod`; `is_alive` → pod phase check. New this round:
  an explicit `kubectl_path` target-config option (pinned/cluster-specific
  CLIs, e.g. OpenShift `oc`) drives every kubectl invocation.
- **Persistence** — `ComputeTargetConfig` entity behind the
  `ComputeTargetRepository` port, implemented by SQLite, PostgreSQL, and
  mem adapters (tenant-scoped list, default flag, duplicate-name guard).
- **Workspace selection + fallback chain** — `Workspace.compute_target_id`;
  spawn resolution is request `compute_target_id` → workspace assignment →
  tenant default → local container auto-detection. Cross-tenant targets
  are rejected at BOTH the workspace-assignment path (4xx) and the spawn
  path (404, indistinguishable from missing — no existence confirmation):
  the request-supplied id AND a stale/tampered workspace assignment are
  both tenant-checked against the workspace's tenant (review F2 fix).
- **Backend-aware lifecycle** — each spawned agent records both its
  `ProcessHandle` and the `SpawnBackend` that owns it, so admin kill
  routes to `docker rm` / `kubectl delete` / remote `docker rm` instead of
  a pid-based local kill (a silent no-op for container/Pod handles).
- **CRUD API** — `GET/POST /api/v1/compute-targets`,
  `GET/PUT/DELETE /api/v1/compute-targets/:id` (plus the pre-existing
  admin variants) with ABAC resolver mappings; the two stale
  exemption-file entries were removed (frozen count lowered). Merged-base
  drift resolved this round: `FROZEN_EXEMPTION_COUNT` lowered to the
  merged exemption-file baseline (50). `PUT /api/v1/workspaces/:id`
  accepts `compute_target_id` (string or null). UI: WorkspaceSettings
  "Compute" tab with a target dropdown.
- **Nix image build + `wl_image_hash` (review F3 fix)** —
  `docker/gyre-agent/flake.nix` builds the agent image from pinned
  nixpkgs with `npm ci` from the committed lockfile and fixed timestamps
  (`agent-image`, `agent-image-docker`, `agent-image-tar`); image
  `User = 65534:65534`. The spawn flow now resolves the image digest
  BEFORE minting the agent JWT (`docker image inspect` needs no running
  container and returns exactly the image `docker run` will use) for
  container and SSH backends and mints via `mint_with_workload`, so the
  `wl_image_hash` claim is populated in the initial token. Kubernetes
  records the cluster-pulled `imageID` in the post-spawn workload
  attestation record (`GET /api/v1/agents/:id/workload`) — the imageID
  only exists once the Pod is scheduled, and the JWT must pre-exist the
  Pod it is injected into. The spec sentence (§ Nix-Based Image Build) was
  amended to state this backend split precisely instead of leaving the
  deviation undocumented.

### Review-round repairs (commit `1fc4470b`)

- **F1 (blocking)** — the `976b339a` base merge kept BOTH parents' copies
  of the ~1000-line test block in `api/spawn.rs` (36 E0428
  duplicate-definition errors; `cargo test -p gyre-server --lib` failed to
  compile). One copy removed; the suite compiles and passes. The merge
  commits are now recorded in the `commits:` frontmatter (they were
  absent, making the breaking change invisible to review scoping).
- **F2 (unsafe scope handling)** — request-supplied `compute_target_id`
  was resolved via global `get_by_id` with no tenant validation (an Admin
  caller could bind a spawn to a foreign tenant's target, 201 Created).\  Both resolution paths now tenant-check; covered by
  `spawn_rejects_cross_tenant_compute_target` (foreign-tenant id → 404,
  no agent created) and
  `spawn_rejects_stale_cross_tenant_workspace_assignment` (tampered
  workspace assignment → 404).
- **F3** — see `wl_image_hash` above; claim round-trip through real
  EdDSA sign/verify covered by
  `auth::tests::mint_with_workload_roundtrips_wl_image_hash_claim`.
- **Merge regression (pre-existing on this branch, found while
  re-running the suite)** — the task-117 kubernetes dispatch arm broke
  the five task-097 secret-delivery tests, which had used Kubernetes-
  typed targets as a local-process vehicle (they relied on the
  pre-task-117 fall-through to LocalTarget; `kubectl` does not exist in
  CI, so the dump script never ran). Repaired by giving the tests the
  REAL kubernetes dispatch through a stub `kubectl` binary (via the new
  `kubectl_path` config) that captures the applied Pod manifest; the
  assertions parse `spec.containers[0].env` into a name→value map, so
  env-delivery is now verified with structured name/value pairing
  against the production manifest builder — equal or stronger
  assertions, no cluster dependency.
- **Housekeeping** — reworded the `./repos/` prose comment in spawn.rs
  (line shifts broke the line-pinned exemption) and removed the stale
  exemption entry (file shrinks); rustfmt-normalized the two files this
  task owns (`spawn.rs`, `kubernetes.rs`).

### Test evidence

All logs with exit codes under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-server --lib api::spawn` → **38 passed, 0 failed**
  (includes the 5 repaired secret-delivery tests and the 2 new
  cross-tenant rejection tests; was: compile failure via F1).
- `cargo test -p gyre-adapters compute --lib` → **41 passed, 0 failed**
  (4 ignored: daemon-required Docker/SSH integration tests; no docker,
  podman, kubectl, or nix binary exists in this sandbox). New:
  `spawn_uses_configured_kubectl_path` (stub kubectl captures the real
  applied manifest).
- `cargo test -p gyre-server --lib compute_target` → **24 passed**
  (CRUD round-trip, invalid type rejection, 404s, non-admin 403,
  duplicate-name 409, delete-referenced-by-workspace 409, admin
  variants).
- `cargo test -p gyre-server --lib workspaces::` → **15 passed**
  (assign/clear `compute_target_id`, cross-tenant rejection).
- `cargo test -p gyre-server --lib abac_middleware` → **9 passed**.
- `cargo test -p gyre-server --lib auth::tests::mint_with_workload` →
  **1 passed** (wl_image_hash EdDSA round-trip).
- `python3 scripts/check-rustfmt-diff.py HEAD^1` → clean;
  `python3 scripts/check-clippy-diff.py HEAD^1` → clean (changed lines).
- Enforced static gates at HEAD (`enforced-gates.log`): arch, hierarchy,
  abac-route-registry (frozen exemption count 50, duplicates at baseline
  22), migration-versions, byte-slice-truncation, dead-message-kinds,
  mem-port-contracts, in-memory-state-stores, migration-sql-portability,
  abac-exempt-handlers, mcp-write-tools, relative-path-defaults,
  fail-open-ref-resolution, fabricated-scope-defaults,
  lossy-secret-conversion, scope-literal-defaults, inert-enforcement,
  forged-scope-fields, forwarded-header-trust, unbounded-external-http,
  task-commit-attribution — all exit 0 (attribution gate green after the
  frontmatter update in this round); no new exemption entries.
- `cargo test --all` / nextest / web vitest are owned by the verification
  stage; not re-run here per assignment scope.
