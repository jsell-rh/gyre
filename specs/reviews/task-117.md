# Review — task-117 (Implement compute target model)

Spec: `specs/system/agent-runtime.md` §3 Compute Target Model (Abstraction, Supported Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate under review: `c9bd7c0e5558014c248a6b1f901fcb0b0c1e6861` against assigned base `dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a`.
Commits assigned: `71967943d`, `1ffe1397f`, `c6b520a1d`, `5482ae4f1`, `e5b481de` — all verified ancestors of the candidate.
Verdict: **approved** (round 13, re-integration review; no findings).

## Scope of this round

The base→candidate diff is docs-only (`specs/tasks/task-117.md`: progress/commits frontmatter
bookkeeping + round-12 notes). The task-117 implementation surface in the base descends from this
candidate lineage, so the review verified (a) the round-12 doc claims are true, and (b) the task
contract is genuinely satisfied by the candidate tree.

## Probes run (all at candidate `c9bd7c0e5`, evidence in `/tmp/stage/review-evidence/round13-reintegration/`)

- `git diff 447119c70 c9bd7c0e5 -- <task-117 surface>` → **empty** — the base's task-117 surface
  (`gyre-ports/src/compute.rs`, `compute_target_repository.rs`, `gyre-adapters/src/compute/`,
  spawn/compute-target/workspace handlers, `docker/gyre-agent/`) is byte-identical to the retained
  checkpoint, exactly as the round-12 note claims.
- `cargo test -p gyre-adapters compute --lib` → **39 passed, 0 failed** (4 ignored: daemon-required).
- `cargo test -p gyre-server --lib api::spawn` → **52 passed, 0 failed** (includes fallback-chain,
  cross-tenant 404, workspace-assignment, backend-aware kill, SSH-name-orphan tests).
- `cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed** (includes
  `update_workspace_binds_compute_target_in_tenant`, `_rejects_cross_tenant_compute_target`,
  `_rejects_unknown_compute_target`).
- Static gates: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-task-commit-attribution.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`,
  `check-migration-sql-portability.sh` → all exit 0.
- **Independent mutation probe** (not a replay of prior-round transcripts): flipped the container
  network default `unwrap_or("none")` → `unwrap_or("bridge")` in
  `ContainerTarget::docker_run_args` — `docker_run_args_enforce_security_defaults`,
  `spawn_executes_security_defaults_via_docker_stub`, and
  `spawn_executes_security_defaults_via_podman_stub` all **FAIL** (3 failed / 36 passed), proving
  the security-default tests observe the real argv the runtime binary receives, not mirrored
  struct fields. File restored; suite re-run green (39 passed); working tree clean.
- Definition-of-done grep: `grep -c '| not-started |' specs/coverage/system/agent-runtime.md` → **0**
  (rows 22–26 all `implemented` with code evidence).

## Verified

- **Frontmatter bookkeeping is correct:** the diff drops `013a871a` (a task-197 pipeline checkpoint
  commit, not task-117 product surface) from the `commits:` list and reorders; the resulting list
  matches the assignment exactly. The commit-attribution gate passes at the candidate.
- **Abstraction:** `ComputeTarget` port trait (`gyre-ports/src/compute.rs`) with
  `name`/`target_type`/`spawn_process`/`kill_process`/`is_alive`, `SpawnConfig`/`ProcessHandle`;
  four production backends in `gyre-adapters/src/compute/` (container, ssh, kubernetes, local) and
  a `SpawnBackend` lifecycle enum routing admin kill to the owning backend.
- **Container security defaults** are unconditional in `docker_run_args` and pinned by
  mutation-killing tests (see probe above).
- **SSH backend:** `SshDockerTarget` shells to `ssh` with `-o BatchMode`/`ConnectTimeout=10`,
  single-quote `shell_quote`s every remote argument, runs `docker run` remotely with the same
  security defaults.
- **Kubernetes backend:** Pod via `kubectl apply` from `build_pod_manifest` (resource
  requests/limits, serviceAccountName, namespace, `workingDir=/workspace`, no server-local
  volumes), `kill_process` → `kubectl delete pod`, `is_alive` → phase check, explicit
  `kubectl_path` config.
- **Persistence:** migration `000033_compute_targets` creates the `compute_targets` table +
  `workspaces.compute_target_id` FK; SQLite/PG/mem adapters behind `ComputeTargetRepository`;
  portability gate passes (no dialect-only SQL).
- **Workspace selection + fallback chain:** spawn resolution is request `compute_target_id` →
  workspace assignment → tenant default → local container auto-detection; cross-tenant ids
  rejected at both assignment (4xx) and spawn (404) — both tested in the passing suites.
- **CRUD API:** `/api/v1/compute-targets[/:id]` (+ admin variants) registered in `api/mod.rs`
  with `RouteResourceMapping` ABAC entries; `PUT /api/v1/workspaces/:id` accepts
  `compute_target_id` (string or null) with in-tenant validation.
- **Nix image build:** `docker/gyre-agent/flake.nix` (agent-image / -docker / -tar targets,
  `User = 65534:65534`) with committed `flake.lock`; `wl_image_hash` resolved pre-mint via
  `docker image inspect` and passed to `mint_with_workload` for container/SSH, cluster-pulled
  `imageID` recorded post-spawn for Kubernetes.

No findings. Full-workspace `cargo test --all`, nextest, and CI remain owned by the verification
stage per assignment scope.
