# Review — task-117 (Implement compute target model)

Spec: `specs/system/agent-runtime.md` §3 Compute Target Model (Abstraction, Supported Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Commit under review: `8b1246aa9` (candidate; base `774737638`). Round 11 — the two commits after the round-10 base merge `e28d79994` are process-only (`80397e992`, `8b1246aa9`: task notes + frontmatter); the code surface is unchanged from round 10.
Verdict: **approved** (round 11). Prior rounds' repairs (2–10) were re-verified independently this round in a fresh sandbox.

## Round 11

All probes run independently this round (prior rounds' evidence directories were not present in the sandbox). Logs under `/tmp/stage/review-evidence/round11-review/`.

Test runs (via `/tmp/stage/bin/cargo`, focused scopes only — full suites belong to verification):

- `cargo test -p gyre-adapters compute --lib` → **39 passed, 0 failed** (4 ignored: daemon-required).
- `cargo test -p gyre-server --lib api::spawn` → **47 passed, 0 failed**.
- `cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::admin` → **36 passed, 0 failed** (backend-aware kill + `killed_pid` audit).
- `cargo test -p gyre-server --lib kubernetes_pod_working_dir_is_workspace_not_local_worktree` → **1 passed** (round-3 regression).
- `cargo test -p gyre-server --lib spawn_rejects_unsafe_agent_name_on_ssh_target_without_orphans` → **1 passed** (round-9 regression).
- Web: `npm ci` (exit 0) then `npx vitest run src/__tests__/WorkspaceSettings.test.js` → **72 passed** (Compute tab + target dropdown).

Independent mutation check (transcript: `round11-review/mutation-check.log`): replaced the four unconditional container security-default args in `ContainerTarget::docker_run_args` with `if let Some(...)` guarded pushes (no fallback — default-config containers would spawn as root with host networking). Result: **3 test failures** (`docker_run_args_enforce_security_defaults` + both stub-execution tests); the panic output shows the executed stub argv with all four flags absent. The security-default tests are pinned to real production behavior, not mirrored logic. Source restored byte-identical after the probe; tree clean.

Static gates (all exit 0): arch, abac-route-registry, task-commit-attribution, mem-port-contracts, abac-exempt-handlers, in-memory-state-stores, forged-scope-fields, inert-enforcement, scope-literal-defaults, fabricated-scope-defaults, forwarded-header-trust.

Verified working (no findings):

- **Abstraction** — `ComputeTarget` port trait (`gyre-ports/src/compute.rs`: `name`, `target_type`, `spawn_process`, `kill_process`, `is_alive`) with `SpawnConfig`/`ProcessHandle`; four production backends in `gyre-adapters/src/compute/` (container, ssh, kubernetes, local).
- **Container backend** — `docker_run_args` builds `docker|podman run --detach` with the spec security defaults (`--network=none` opt-in bridge, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`), worktree volume, runtime auto-detection (docker → podman); `spawn_process` executes exactly that vector (proven by the mutation check above).
- **SSH backend** — `SshDockerTarget` runs `docker run` on the remote host with the same security defaults; every remote argument individually single-quote shell-escaped; bounded `ConnectTimeout=10`; `BatchMode=yes`; credentials from compute-target config JSON; legacy bare-process escape only behind opt-in `container_mode:false`.
- **Kubernetes backend** — Pod manifest via `kubectl apply` (stdin, no shell): `runAsUser/runAsGroup 65534`, `runAsNonRoot`, `allowPrivilegeEscalation: false`, memory limit 2Gi, `serviceAccountName` at pod level (not container), DNS-1035/253-safe pod names, `workingDir: /workspace` (round-3 fix; regression test passes); `kill_process` → `kubectl delete pod`; `is_alive` → phase check; explicit `kubectl_path` config option drives every invocation (stub-kubectl test).
- **Persistence** — `ComputeTargetEntity` behind `ComputeTargetRepository` (SQLite, PostgreSQL, mem): tenant-scoped list, default flag, duplicate-name guard, workspace-reference guard.
- **Workspace selection + fallback chain** — `Workspace.compute_target_id` (PUT validates existence AND tenant); spawn resolution is request `compute_target_id` → workspace assignment → tenant default → local container auto-detection → bare local process; tenant guard at BOTH the request path and the workspace-assignment path (cross-tenant 404, indistinguishable from missing).
- **Backend-aware lifecycle** — spawned agents record handle + owning `SpawnBackend`; admin kill routes to `docker rm` / `kubectl delete` / remote `docker rm`; round-10 merge resolution verified: `revoke_agent_task_secrets` folded into `monitor_spawned_agent_exit` (`spawn.rs:1558`) so exit bookkeeping covers every backend.
- **CRUD API** — `GET/POST /api/v1/compute-targets`, `GET/PUT/DELETE /api/v1/compute-targets/:id` (+ admin variants): Admin-role gating on mutations, tenant-scoped reads (cross-tenant 404), duplicate-name 409, delete 409 when workspaces reference the target (mem + SQLite both guarded). Routes registered in `api/mod.rs` with ABAC resolver mappings (`abac_middleware.rs:363-364`); no exemption-file entries.
- **Nix image build** — `docker/gyre-agent/flake.nix` + committed `flake.lock` (nixpkgs pinned at rev `50ab7937` with NAR hash; `npm ci` from committed package-lock; fixed timestamps; image `User = 65534:65534`). No nix binary exists in this sandbox, so `nix build` is not runnable here — an environment restriction, not a code defect (lock-node byte-verification methodology documented in round 3). Image-hash recording follows the spec's backend split: container/SSH resolve the local image digest before JWT minting (`wl_image_hash` in the initial token); Kubernetes records the cluster-pulled `imageID` in the post-spawn workload attestation.
- **Attribution** — the four task-labeled product-surface commits reachable from the candidate (`c6b520a1`, `5482ae4f`, `e5b481de`, `013a871a`) exactly match the frontmatter `commits:` list; `check-task-commit-attribution.sh` exit 0. (`02966511` from the original implementation round lives only on superseded, unreachable pipeline branches.)
- **Round-9 fix present** — SSH agent-name charset/length validation hoisted to the top of `spawn_agent_core` beside the duplicate-name check, before any write; the orphan regression test passes.

No findings. All acceptance criteria are met with real production code and mutation-checked tests.
