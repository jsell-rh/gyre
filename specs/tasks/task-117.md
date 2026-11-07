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
commits: ["013a871a444b20079a21ea01843ddd36540b9222", "e5b481de02a551664a7bece73d7423fc0be6df30"]
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

Implementation commits `02966511`...`2b26880` (merge repair `c79542d8`),
review-repair commits `1fc4470b`, `d404a439`, and `e5b481de`
(round 4), the `6a9739e`/`722179f0` base merges (`976b339a`,
`7e672231`), the task-092 base merge `b30c8546` (web/dist rebuilt
from merged sources), and the round-4 base merge `a2bdf9a2` carry the
work below. Evidence is saved under `/tmp/stage/review-evidence/`
(per-round subdirectories; round 4 in `round4/`).

### Behavior

- **Abstraction** — `ComputeTarget` port trait
  (`gyre-ports/src/compute.rs`: `name`, `target_type`, `spawn_process`,
  `kill_process`, `is_alive`) with `SpawnConfig`/`ProcessHandle`; four
  production backends in `gyre-adapters/src/compute/`.
- **Container backend** — `ContainerTarget` runs `docker|podman run
  --detach` with the spec security defaults enforced at spawn time:
  `--network=none` (opt-in bridge via config), `--memory=2g`,
  `--pids-limit=512`, `--user=65534:65534`; worktree mounted as a volume;
  runtime auto-detection (docker -> podman). Since round 4 the argv
  construction is factored into `ContainerTarget::docker_run_args`
  (same pattern as `SshDockerTarget`) and `spawn_process` executes
  exactly that vector, so the defaults are pinned by tests against
  both the built argv and the argv the runtime binary actually
  received (stub docker/podman spawn tests).
- **SSH backend** — `SshDockerTarget` (`ssh.rs`) shells out to `ssh` with
  batch mode + bounded connect timeout and runs `docker run` on the remote
  host with the same security defaults; every remote argument is
  single-quote shell-escaped (`shell_quote`). Credentials (user, host,
  port, identity file) come from the compute-target config JSON.
- **Kubernetes backend** — `KubernetesTarget` (`kubernetes.rs`) creates a
  Pod via `kubectl apply` from a generated manifest (resource
  requests/limits, service account, namespace, image pull policy from
  config; DNS-1035/253-char-safe pod names); `kill_process` ->
  `kubectl delete pod`; `is_alive` -> pod phase check. An explicit
  `kubectl_path` target-config option (pinned/cluster-specific CLIs,
  e.g. OpenShift `oc`) drives every kubectl invocation. The Pod's
  `workingDir` is `/workspace` — the agent image entrypoint clones its
  own checkout there; the server-local worktree path is never forwarded
  (round-3 fix, see below).
- **Persistence** — `ComputeTargetConfig` entity behind the
  `ComputeTargetRepository` port, implemented by SQLite, PostgreSQL, and
  mem adapters (tenant-scoped list, default flag, duplicate-name guard).
- **Workspace selection + fallback chain** — `Workspace.compute_target_id`;
  spawn resolution is request `compute_target_id` -> workspace assignment ->
  tenant default -> local container auto-detection. Cross-tenant targets
  are rejected at BOTH the workspace-assignment path (4xx) and the spawn
  path (404, indistinguishable from missing — no existence confirmation).
- **Backend-aware lifecycle** — each spawned agent records both its
  `ProcessHandle` and the `SpawnBackend` that owns it, so admin kill
  routes to `docker rm` / `kubectl delete` / remote `docker rm` instead of
  a pid-based local kill (a silent no-op for container/Pod handles).
- **CRUD API** — `GET/POST /api/v1/compute-targets`,
  `GET/PUT/DELETE /api/v1/compute-targets/:id` (plus the pre-existing
  admin variants) with ABAC resolver mappings; the two stale
  exemption-file entries were removed (frozen count lowered, then
  re-lowered to the merged baseline 50). `PUT /api/v1/workspaces/:id`
  accepts `compute_target_id` (string or null). UI: WorkspaceSettings
  "Compute" tab with a target dropdown.
- **Nix image build + `wl_image_hash`** — `docker/gyre-agent/flake.nix`
  builds the agent image from nixpkgs pinned by the committed
  `docker/gyre-agent/flake.lock` (nixos-24.11 head `50ab7937`, NAR hash
  byte-verified — see round 3 below), with `npm ci` from the committed
  package-lock.json and fixed timestamps (`agent-image`,
  `agent-image-docker`, `agent-image-tar`); image `User = 65534:65534`.
  The spawn flow resolves the image digest BEFORE minting the agent JWT
  (`docker image inspect`) for container and SSH backends and mints via
  `mint_with_workload`, so the `wl_image_hash` claim is populated in the
  initial token. Kubernetes records the cluster-pulled `imageID` in the
  post-spawn workload attestation record (`GET /api/v1/agents/:id/workload`)
  — the imageID only exists once the Pod is scheduled, and the JWT must
  pre-exist the Pod it is injected into. The spec sentence (§ Nix-Based
  Image Build) states this backend split precisely.

### Review-round repairs

Round 2 (`1fc4470b`): duplicate test block from a base merge (compile
failure), tenant-checking of request-supplied and workspace-assigned
`compute_target_id` (cross-tenant 404s), `wl_image_hash` minted into the
initial JWT, task-097 secret-delivery tests repaired onto the real
kubernetes dispatch via a stub `kubectl`, exemption-file housekeeping.

Round 3 (`d404a439`, findings from review `00e38774`):

- **Kubernetes Pods could not start in the standard flow** — the k8s
  dispatch arm forwarded the canonicalized SERVER-LOCAL worktree path as
  the Pod `workingDir`; the manifest carries no volumes and the agent
  image contains no such path, so under CRI the container failed to start
  (runc chdir ENOENT) and the agent never ran. Fixed by mirroring the
  SSH container arm: the k8s `SpawnConfig` overrides `work_dir` to
  `/workspace` (the entrypoint clones its own checkout there). Regression
  test `kubernetes_pod_working_dir_is_workspace_not_local_worktree`
  captures the applied manifest through the stub-kubectl spawn path and
  asserts `workingDir == "/workspace"` plus absence of server-local
  volumes; mutation-checked (fails without the fix).
- **`flake.lock` committed** — the flake previously left nixpkgs on the
  moving `nixos-24.11` branch ref with no lock file ("created on first
  run"), so two builds at different times resolved different nixpkgs.
  `docker/gyre-agent/flake.lock` now pins the branch head `50ab7937` with
  its NAR hash. No nix binary exists in this sandbox, so the lock node
  was constructed and then independently verified: a NAR serializer
  replicated from nix's own `archive.cc`/`serialise.cc` (byte format
  cross-checked against nix's committed binary fixture
  `tests/functional/duplicate.nar`) was validated against three
  nix-generated public lock nodes (numtide/flake-utils,
  nixpkgs@d02d88f8, nixpkgs@edf04b75 — all matched byte-for-byte) before
  hashing the locked revision; methodology and match transcripts in
  `/tmp/stage/review-evidence/flake-lock-verification.txt`. The flake.nix
  comment now states the lock is committed.
- **`docs/api-reference.md` table repair** — the tenant-scoped
  compute-targets block was inserted mid-table, breaking the tail of the
  admin endpoint table (bcp/scim rows rendered as paragraph text). The
  block now sits below the admin table; stray trailing `|` dropped.
- **Task-092 base merge resolved** (`b30c8546`) — conflicts were confined
  to generated `web/dist` artifacts; both branches changed `web/src`, so
  dist was rebuilt with vite from the merged sources (`npm ci` + build;
  full web vitest: 65 files, 1624 passed / 41 skipped) rather than taking
  either side's stale bundle.

Round 4 (`e5b481de`, finding from review of `536c76ad`):

- **Container security defaults were unpinned by executing tests** —
  the `container.rs` `security_default_*` tests restated the
  `unwrap_or(<default>)` fallbacks against struct fields (mirrored
  logic), so the reviewer's mutation — replacing the four unconditional
  security args in `spawn_process` with `if let Some(...)` guarded
  versions, letting default-config containers spawn as root with host
  networking — left every suite green. Fixed by (a) factoring the
  `docker run` argv construction into `ContainerTarget::docker_run_args`
  and spawning through it, (b) replacing the six mirrored tests with
  real-vector assertions (defaults, structural flags, and overrides
  with no-linger-on-default checks), and (c) two end-to-end tests that
  drive the real `spawn_process` through stub `docker` and `podman`
  binaries (PATH-scoped, serialized via a `PATH_LOCK`) and assert the
  four flags on the argv the stub actually executed. Mutation-checked:
  the reviewer's exact mutation now fails
  `docker_run_args_enforce_security_defaults` and both stub tests
  (transcript in `round4/mutation-check.log`, exit 101).

### Test evidence

All logs with exit codes under `/tmp/stage/review-evidence/`
(rounds 2–3 at source `d404a439`, round 4 in `round4/` at source
`e5b481de`):

- Round 4 (`e5b481de`): `cargo test -p gyre-adapters compute --lib` ->
  **39 passed, 0 failed** (4 ignored: daemon-required integration
  tests; the six mirrored security tests were replaced by four
  mutation-killing argv/stub tests); `cargo test -p gyre-server --lib
  api::spawn` -> **39 passed, 0 failed**;
  `check-rustfmt-diff.py`/`check-clippy-diff.py` vs `HEAD^1` -> clean.
- Earlier rounds (`d404a439`): `cargo test -p gyre-server --lib
  compute_target` -> **24 passed**; web vitest on the merged sources
  -> **1624 passed, 41 skipped, 0 failed** (65 files); enforced static
  gates all exit 0 (arch, hierarchy, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, migration-versions,
  byte-slice-truncation, dead-message-kinds, mem-port-contracts,
  in-memory-state-stores, migration-sql-portability,
  relative-path-defaults, fail-open-ref-resolution,
  fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, unbounded-external-http,
  sandbox-sweep-artifacts); no new exemption entries.
- `cargo test --all` / nextest / GitHub CI are owned by the verification
  stage; not re-run here per assignment scope.
