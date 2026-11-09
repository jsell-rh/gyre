# Review — task-117 (Implement compute target model)

Spec: `specs/system/agent-runtime.md` §3 Compute Target Model (Abstraction, Supported Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate under review: `5c8fbcd7985c6eabbba224469db44c9a2ac6dd11` (assignment base `fefde2946`; the base SHA in job.json is a 39-char typo of `fefde2946f3a0af1f212588f54591516fee0f7b2`).
Verdict: **needs-revision** (one finding).

## Round 9

### Test and gate runs (candidate HEAD; logs with exit codes in /tmp/stage/review-evidence/)

- `/tmp/stage/bin/cargo test -p gyre-adapters compute --lib` → **39 passed, 0 failed** (4 ignored: daemon-required).
- `/tmp/stage/bin/cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed**; `api::spawn` → **43 passed, 0 failed** (incl. the six fallback-chain / cross-tenant / backend-kill tests and the round-3 `kubernetes_pod_working_dir_is_workspace_not_local_worktree` regression, re-confirmed individually).
- Static gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-in-memory-state-stores.sh`, `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh` → all exit 0.
- Web: `npm ci` + `npx vitest run src/__tests__/WorkspaceSettings.test.js` → **72 passed, 0 failed** (field rename `default_compute_target` → `compute_target_id` matches the actual API).
- Mutation checks (both reverted; tree verified clean after each):
  1. Round-4 reviewer mutation re-applied independently — the four container security args in `ContainerTarget::docker_run_args` made `if let Some(...)`-conditional (default-config containers spawn as root with host networking): `cargo test -p gyre-adapters compute --lib` → **FAILED, 3 tests** (`docker_run_args_enforce_security_defaults`, both stub-runtime `spawn_executes_security_defaults_via_*` tests). Mutation killed — the security defaults are pinned on the argv the runtime binary actually executes.
  2. Spawn-time tenant check removed (request-supplied and workspace-assigned `compute_target_id` accepted without tenant comparison): `spawn_rejects_cross_tenant_compute_target` + `spawn_rejects_stale_cross_tenant_workspace_assignment` → **FAILED, both**. Mutation killed.

### Verified working (no findings)

- **Abstraction**: `ComputeTarget` port trait (`gyre-ports/src/compute.rs`) with the five specced methods; four production backends in `gyre-adapters/src/compute/`; `SpawnBackend` wrapper routes admin kill / exit-monitor `kill_process`/`is_alive` to the backend that owns the handle (container ID → `docker rm --force`, Pod → `kubectl delete pod`, remote container → SSH), with lock-dropped-before-await in the exit monitor so a blocking SSH probe cannot stall kills/registrations.
- **Container security defaults**: enforced unconditionally in `docker_run_args` (pinned by argv + stub-execution tests, mutation-verified above); worktree mounted `--volume={work_dir}:{work_dir}`; runtime auto-detection docker→podman.
- **SSH backend**: `SshDockerTarget` runs `docker run` on the remote host with the same four defaults, every remote argument single-quote shell-escaped (hostile-image-tag test), bounded `ConnectTimeout=10`, batch mode; `container_mode: false` legacy escape hatch.
- **Kubernetes backend**: Pod via `kubectl apply -f -` from a generated manifest (runAsUser/Group 65534, runAsNonRoot, allowPrivilegeEscalation:false, memory 2Gi default, serviceAccountName at Pod-spec level, DNS-1035/253-safe pod names matching the ProcessHandle id), kill = `kubectl delete pod`, is_alive = phase check; `kubectl_path` config override drives the real invocation (stub-kubectl test captures the applied manifest via stdin). `workingDir` is `/workspace` — the server-local worktree path is never forwarded (round-3 regression test re-confirmed).
- **Persistence**: `ComputeTargetEntity` behind `ComputeTargetRepository` (SQLite + PostgreSQL + mem; tenant-scoped list, default flag, workspace-reference guard shared with the mem workspace store so the 409-on-referenced DELETE holds in-memory too).
- **Workspace selection + fallback chain**: request id → workspace assignment → tenant default → local container auto-detection → bare local process; cross-tenant ids rejected at assignment (4xx), at spawn for request-supplied ids (404) and for stale/tampered workspace assignments (404, no existence confirmation) — all tenant checks mutation-verified.
- **CRUD API**: full tenant-scoped CRUD with ABAC resolver mappings (`compute_target` resource); the two stale exemption-file entries removed and `FROZEN_EXEMPTION_COUNT` lowered 52→50 (shrink-only, allowed); non-Admin mutations 403; duplicate names 409; DELETE 409 when referenced.
- **Nix flake**: `docker/gyre-agent/flake.nix` + committed `flake.lock` pinning nixpkgs@`50ab7937` with NAR hash, `npm ci` from the committed lockfile, fixed timestamps, image `User = 65534:65534`. Lock structure verified (40-hex rev, sha256 NAR hash, lock v7, lastModified 2025-06-30 — plausible nixos-24.11 head date). **Not recomputable in this sandbox**: no `nix` binary and codeload.github.com:443 refused (network restriction — infrastructure, not a code defect). Verifier with network should run `nix flake lock --check` (or `nix build .#agent-image-docker`) in `docker/gyre-agent/` to confirm the pinned narHash matches upstream nixpkgs@50ab7937. `wl_image_hash`: resolved pre-mint via `docker image inspect` for container/SSH (EdDSA round-trip test), cluster-pulled `imageID` recorded post-spawn for Kubernetes — matches the spec sentence as amended.
- Coverage rows 22–26 marked implemented consistent with the shipped code.

### Findings

- **F1 (moderate): rejected SSH spawn leaves orphaned Active agent + InProgress task + worktree.** `crates/gyre-server/src/api/spawn.rs` SSH dispatch arm (~lines 1001–1010): the agent-name charset/length validation returns `ApiError::InvalidInput` (400) *after* the agent record, agent token, git worktree (+DB record), and task assignment (task → InProgress, `assigned_to` the new agent) have been persisted. Reproduced with a temporary probe test at the candidate (name `"bad name with space"` against an SSH compute target): response 400, yet `orphan_agent = Active`, `task_after = InProgress + assigned`, `worktree_count = 1` (transcript: `/tmp/stage/review-evidence/probe-ssh-name-orphan.log`). This violates the function's own compensating-rollback invariant — the worktree-creation-failure and interrogation-policy-failure paths both delete the just-created agent record and token, and `spawn_rejects_cross_tenant_compute_target` asserts "rejected spawn must not create an agent". The check's comment says "validate early" but it runs after every write. The identical late `return Err` exists in the base `fefde2946` but only for opt-in `container_mode: true` (base default false); the candidate's spec-mandated flip of the default to `true` (correct per §3) put this orphaning error path on every default SSH-compute-target spawn with a non-`[a-zA-Z0-9._-]` or >63-char agent name, and the candidate restructured this arm (moving the check above the container_mode split) without fixing its placement. Remedy: hoist the validation before `state.agents.create(&agent)` (beside the duplicate-name check, which already returns before any write) and delete the late check; add a regression test asserting the 400 leaves no agent/task/worktree behind.

### Notes for the verifier (not findings)

- `cargo test --all` is verification-owned; all task-117-focused suites pass at the candidate.
- The flake.lock NAR-hash recomputation and a real `nix build` need network/nix — see the check command above.
- The web/dist churn produced by cargo test runs in this sandbox was restored to the committed state (`git status` clean at `5c8fbcd7`).
