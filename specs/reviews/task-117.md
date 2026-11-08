# Review — task-117 (agent-runtime.md §3 Compute Target Model)

Spec: `agent-runtime.md` §3 (Compute Target Model: Abstraction, Supported
Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate under review: `83863afd` (base `72ad2a128`; = round-5 merge
`d9e7785` plus two process commits touching only `specs/tasks/task-117.md`).

## Round 6 (independent)

Evidence with commands and exit codes: `/tmp/stage/review-evidence/`
(round-6 files: `adapters-compute-test.log`, `server-spawn-test.log`,
`server-compute-target-test.log`, `server-workspaces-test.log`,
`static-gates.log`, `mutation-check.log`, `web-workspacesettings-test.log`,
`web-rebuild.log`, `web-dist-stale-bundle.txt`,
`nix-flake-lock-verification.txt`, `nixpkgs-commit.json`).

Verified working (no findings):

- **Abstraction** — `ComputeTarget` port trait (`gyre-ports/src/compute.rs`:
  `name`, `target_type`, `spawn_process`, `kill_process`, `is_alive`) with
  `SpawnConfig`/`ProcessHandle`; four production backends in
  `gyre-adapters/src/compute/` (container, ssh/ssh-docker, kubernetes,
  local). Legacy `DockerTarget`/`SshTarget` retained only as escape hatches;
  dispatch defaults to the spec backends.
- **Container backend security defaults are real and pinned by executing
  tests.** Reproduced the round-4 mutation independently: replacing the four
  unconditional security args in `ContainerTarget::docker_run_args`
  (`--network=none`, `--memory=2g`, `--pids-limit=512`,
  `--user=65534:65534`) with `if let Some(...)` guarded versions fails
  `docker_run_args_enforce_security_defaults` AND both stub-runtime tests
  (`spawn_executes_security_defaults_via_docker_stub`, `..._via_podman_stub`)
  — 3 failed / 5 passed before restore (mutation-check.log). Source restored;
  working tree re-hashed against the review snapshot: 0 mismatches.
  Adapters compute suite at candidate: 39 passed, 0 failed (4 ignored:
  daemon-required).
- **SSH backend** — real `ssh` invocation (BatchMode, ConnectTimeout=10
  bounded), `docker run` on the remote host through
  `SshDockerTarget::docker_run_args` (same four security defaults, asserted
  by tests), every remote argument single-quote shell-escaped
  (`shell_quote`, hostile-image-tag test), credentials from compute-target
  config JSON.
- **Kubernetes backend** — real `kubectl apply -f -` with a generated Pod
  manifest (memory/cpu limits, `securityContext` runAsUser/runAsGroup 65534,
  runAsNonRoot, no privilege escalation, serviceAccountName at pod level,
  DNS-1035/253-char pod names, `kubectl_path` override driving the actual
  spawned binary — stub-kubectl test). `kill_process` → `kubectl delete
  pod`; `is_alive` → phase check. Spawn-path test through a stub kubectl
  asserts `workingDir == /workspace` and no server-local volumes
  (`kubernetes_pod_working_dir_is_workspace_not_local_worktree`).
  Note: no per-pod pids limit in the manifest — correct, the Kubernetes API
  has no such field (kubelet-level control); documented in code.
- **Persistence** — Diesel SQLite adapter (tenant-scoped list, default,
  duplicate-name guard, `has_workspace_references` via workspaces join),
  PostgreSQL adapter, mem adapter with a real shared-workspace-store scan;
  migration `2026-03-26-000033_compute_targets` with FK
  (`check-migration-versions.sh` exit 0).
- **Workspace selection + fallback chain** — request `compute_target_id`
  (tenant-checked, cross-tenant → 404 indistinguishable from missing) →
  workspace assignment (re-checked at spawn against the workspace tenant,
  stale cross-tenant assignment tested) → tenant default → local container
  auto-detection → bare local process only when no runtime exists. Server
  `api::spawn` suite: 41 passed, 0 failed (includes both cross-tenant
  rejection tests and workspace/default fallback tests).
- **CRUD API** — five tenant-scoped routes registered in `api/mod.rs` with
  ABAC resolver mappings (`RouteResourceMapping::api(...)` entries added;
  the two stale exemption-file entries removed; frozen exemption count
  lowered 52→50, the allowed direction; `check-abac-route-registry.sh` exit
  0). Per-handler tenant scoping on every route; Admin role on mutations;
  DELETE returns 409 while workspaces reference the target (tested in mem
  mode against the shared workspace store). `compute_target` server suite:
  24 passed.
- **Workspace binding validation** — `PUT /api/v1/workspaces/:id` accepts
  `compute_target_id` (string or JSON null, distinct from absent via
  `deserialize_present`); unknown and cross-tenant IDs rejected 400 with no
  partial persistence (three tests, workspaces suite 19 passed).
- **Backend-aware lifecycle** — `SpawnBackend` recorded per agent at spawn;
  admin kill routes to the owning backend (docker rm / kubectl delete /
  remote docker rm) instead of a pid-based local kill. End-to-end test
  kills a real process through the real admin route
  (`admin_kill_terminates_process_via_registered_backend`).
- **Nix image build + `wl_image_hash`** — `docker/gyre-agent/flake.nix`
  builds from nixpkgs pinned by the committed `flake.lock`; npm ci from the
  committed package-lock; fixed timestamps; image `User = 65534:65534`.
  Independently verified via the GitHub API that the pinned rev
  `50ab793786d9de88ee30ec4e4c24fb4236fc2674` is a real nixos-24.11 commit
  whose committer date (2025-06-30T09:05:12Z = epoch 1751274312) matches the
  lock's `lastModified` exactly (nixpkgs-commit.json). The NAR hash cannot
  be recomputed without a nix binary (absent in this sandbox); round-3's
  documented NAR-serializer methodology (cross-checked byte-for-byte
  against nix's own fixtures and three nix-generated public lock nodes)
  stands as the best available verification. Spawn resolves the image digest
  before minting (`docker image inspect`) for container/SSH backends and
  mints via `mint_with_workload` — EdDSA sign→verify round-trip with
  `wl_image_hash` intact (auth suite 54 passed); Kubernetes records the
  cluster-pulled `imageID` in the post-spawn workload attestation. The spec
  sentence was amended to state this backend split precisely and matches
  the code.
- **Static gates at candidate** — arch, task-commit-attribution,
  mcp-write-tools, abac-exempt-handlers, abac-route-registry,
  migration-versions: all exit 0 (static-gates.log).
- **Web UI** — `WorkspaceSettings.svelte` sends
  `{compute_target_id: X || null}` to the fixed DTO; vitest
  `WorkspaceSettings.test.js`: 71 passed (after `npm ci`).

### Non-blocking observations

- **Committed `web/dist` is base's bundle (sends `default_compute_target`,
  which the DTO ignores), not a rebuild of the merged `web/src`.** Traced
  per-commit: the task branch's round-3 rebuild (`b30c8546`,
  `index-BgPtEtKd.js`, contains the fix) was replaced by base-side bundles
  from `a2bdf9a2` onward. This is the pipeline's designed intermediate
  state, not an implementer omission: `pipeline-remote.py` calls
  `artifacts.restore_generated(checkout, base)` at every implement-stage
  checkpoint (resetting web/dist to base), and
  `pipeline-artifacts.py` `publish()` regenerates + validates verified
  assets as a protected controller stage — the commit pattern visible for
  tasks 078–096. The candidate's `web/src` is correct: a rebuild from the
  merged sources produces a bundle sending
  `updateWorkspace(..., {compute_target_id: ... || null})`
  (web-rebuild.log, web-dist-stale-bundle.txt). Recorded for verification,
  see below.
- The trait's `target_type()` returns `&'static str` rather than the spec
  sketch's `ComputeTargetType` enum (the entity/persistence layer uses the
  enum). All five spec-named methods exist with matching semantics; the
  spec block is an abstraction sketch. Non-blocking.
- SSH executed-remote-argv is pinned by builder tests
  (`ssh_docker_run_args_*`) rather than a stub-ssh executing test. Unlike
  the round-4 container defect there is no duplicated argv logic —
  `remote_docker(&self.docker_run_args(config))` is the single call site —
  so the defaults cannot drift from the builder. Non-blocking.

### Deferred to verification (concrete checks)

1. `cargo test --all` / nextest / CI — workspace-wide runs belong to the
   verification stage (the executor wrapper rejects them here). Focused
   suites at the identical code tree are green; round-5's full
   `cargo test -p gyre-server --lib` (1413 passed) ran at `d9e7785`, whose
   tree differs from the candidate only in `specs/tasks/task-117.md`.
2. Publication asset rebuild: confirm the `build(web): regenerate verified
   assets (task-117)` commit's bundle contains `compute_target_id`
   (the current committed dist is base's by design — see observation above).
3. When a nix binary is available: `nix build docker/gyre-agent#agent-image`
   (or `.#agent-image-docker`) to confirm the lock's NAR hash resolves and
   the build is reproducible.

## Verdict

Approved. No findings; the observations above are recorded for the
verifier and the controller-owned publication stage.
