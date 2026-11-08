# Review — task-117 (Implement compute target model, agent-runtime.md §3)

Spec: `specs/system/agent-runtime.md` §3 "Compute Target Model" (Abstraction, Supported Backends, Nix-Based Image Build, Tenant and Workspace Configuration), per `specs/tasks/task-117.md`.
Candidate under review: `3382242006f7b12e30b570ba93fa3a44a1123dd2` against base `c5deca801a331e049f35b181f42d898df24e7e2b`. This is round 7: the substantive delta since the round-5-reviewed state (`d9e778536`) is the round-6 base merge `4ef962e48` (task-105 log compression upstream; conflicts in `lib.rs`/`mem.rs`) plus the notes-only commit `338224200`. Rounds 1–6 reviewed the original implementation and repairs; this review re-verified the acceptance criteria independently rather than trusting the labels.

Verdict: **approved** (no findings). Evidence under `/tmp/stage/review-evidence/round7/`.

## Round 7 probes (all at candidate `338224200`, tree clean before and after)

- `cargo test -p gyre-adapters compute --lib` → **39 passed, 0 failed** (4 ignored: daemon-required).
- `cargo test -p gyre-server --lib api::spawn` → **41 passed, 0 failed**.
- `cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed**.
- `cargo test -p gyre-server --lib log_compression` → **11 passed**; `api::agent_logs` → **2 passed** (base feature survived the round-6 merge).
- Full `cargo test -p gyre-server --lib` → **1440 passed, 0 failed** (merge-integrity check).
- Static gates: `check-abac-route-registry.sh`, `check-arch.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh` → all exit 0.
- Mutation checks (source mutated, tested, restored; transcripts in `round7/mutation*.log`):
  1. Container network default `none`→`bridge` in `docker_run_args` → **3 tests fail** (`docker_run_args_enforce_security_defaults` + both stub-execution tests asserting on the argv the runtime binary actually received). The round-4 repair genuinely pins the spec §3 security defaults on executed argv.
  2. Request-supplied `compute_target_id` tenant guard disabled (`true ||`, spawn.rs:420) → `spawn_rejects_cross_tenant_compute_target` fails.
  3. Workspace-assigned `compute_target_id` tenant guard disabled (spawn.rs:442) → `spawn_rejects_stale_cross_tenant_workspace_assignment` fails. Both cross-tenant guards are independently pinned.

## Verified against the acceptance criteria

- **Abstraction** — `ComputeTarget` port trait (`gyre-ports/src/compute.rs`) with `name`/`target_type`/`spawn_process`/`kill_process`/`is_alive`; four production backends (`container.rs`, `ssh.rs`, `kubernetes.rs`, `local.rs`), all shelling out to real binaries (docker/podman, ssh, kubectl). Fail-fast tests prove spawn errors when the binary is absent.
- **Container security defaults** — enforced in `docker_run_args`, executed by `spawn_process` through exactly that vector; mutation-killed (probe 1). Overrides replace defaults with no-linger checks.
- **SSH backend** — `SshDockerTarget` runs `docker run` on the remote host with the same four defaults, every remote argument single-quote shell-escaped (`shell_quote`; injection test with a hostile image tag), `ConnectTimeout=10` bounding probes, batch mode.
- **Kubernetes backend** — Pod via `kubectl apply` from a generated manifest (memory/cpu limits, `runAsUser/Group 65534`, `runAsNonRoot`, no privilege escalation, `serviceAccountName` at pod level, DNS-1035/253-safe names); `kill_process` deletes the Pod, `is_alive` checks phase. `kubectl_path` config drives the invoked binary (stub-kubectl test). The round-3 `workingDir=/workspace` regression test is a real end-to-end spawn through the API with a stub kubectl capturing the applied manifest.
- **Persistence** — `ComputeTargetEntity` behind `ComputeTargetRepository` (SQLite `2026-03-26-000033_compute_targets`, PostgreSQL, mem with shared workspace store so `has_workspace_references` is a real scan); duplicate-name guard, tenant-scoped list, default flag all tested.
- **Workspace selection + fallback chain** — request id → workspace assignment → tenant default → local container auto-detection → bare local; every link has a passing test; cross-tenant rejection at both the assignment path (4xx) and spawn path (404, indistinguishable from missing), both mutation-killed (probes 2–3).
- **CRUD API** — full CRUD at `/api/v1/compute-targets` with per-handler tenant checks, Admin-role checks on mutations, 409 on workspace-referenced delete, ABAC resolver mappings present; exemption file at frozen baseline 50.
- **Backend-aware lifecycle** — `spawned_backends` records the owning `SpawnBackend`; admin kill routes to `docker rm`/`kubectl delete`/remote `docker rm` instead of pid-kill. End-to-end test kills a real spawned process through the full router. The round-6 merge resolution verified correct: `AppState` keeps both `spawned_backends` (task-117) and the `AgentLogBuffer`-typed `agent_logs` (task-105); `mem.rs` test-state builders carry both.
- **Nix flake** — `docker/gyre-agent/flake.nix` + committed `flake.lock` pinning nixpkgs `50ab7937` with NAR hash; `wl_image_hash` resolved pre-mint for container/SSH (EdDSA round-trip test) and cluster-pulled `imageID` recorded post-spawn for Kubernetes, matching the amended spec sentence. No nix binary exists in this sandbox (round 3 documented the lock-node verification methodology); files verified structurally.
- **cargo test --all** — owned by the verification stage; the full `gyre-server --lib` suite plus all focused suites pass here.

## Minor observations (not blocking, recorded for completeness)

- `SshDockerTarget::kill_process` and `KubernetesTarget::kill_process` treat any non-zero kubectl/ssh status as a successful kill (warn + `Ok(())`) — idempotent-kill posture that also masks a genuinely unreachable remote/cluster. Consistent with the container backend's `rm --force` warn-and-Ok and with the spec, which does not define kill error semantics. A future hardening could distinguish "not found" from "unreachable" via stderr.
- `is_alive` returning `false` on probe *failure* (all backends, plus the exit monitor's `unwrap_or(false)`) means a transient probe error can mark a live agent as exited and unregister it. Same tradeoff exists for the pre-existing local/container backends; liveness-monitor availability posture, not a scope or security defect.
- The port trait's `target_type()` returns `&'static str` where the spec's illustrative snippet shows a `ComputeTargetType` enum return (ports cannot depend on `gyre-domain`; the enum exists in domain and types the entity/API). Functional contract — one trait, three distinguishable backend types — is met.
- Task-117's web files (`WorkspaceSettings.svelte` + test) are unchanged since the round-5 vitest run (71 passed) on merged sources; the web/src churn since then is base-side task-088 work owned by its own task, so no frontend re-run this round.
