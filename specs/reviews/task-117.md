# Review — task-117 (agent-runtime §3 Compute Target Model)

Spec: `specs/system/agent-runtime.md` §3 (Abstraction, Supported Backends,
Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate: `b5bbe45e` (base `1d9547de`; production code identical to the
round-4 mutation-checked `e5b481de` — the two commits since touch only
`specs/tasks/task-117.md`). This is review round 5; rounds 2–4 repairs
(`1fc4470b`, `d404a439`, `e5b481de`) were re-verified independently below.
Verdict: **approved**.

## Independent evidence (round 5)

All logs with exit codes under `/tmp/stage/review-evidence/round5/`.

- `cargo test -p gyre-adapters compute --lib` → **39 passed, 0 failed**
  (4 ignored: daemon-required integration tests).
- `cargo test -p gyre-server --lib api::spawn` → **39 passed, 0 failed**.
- `cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed**.
- **Mutation check (fresh, this review):** replaced the four unconditional
  security args in `ContainerTarget::docker_run_args` with `if let Some(...)`
  guarded versions (the exact round-4 finding mutation — default-config
  containers would spawn as root with host networking). Result:
  `docker_run_args_enforce_security_defaults`,
  `spawn_executes_security_defaults_via_docker_stub`, and
  `spawn_executes_security_defaults_via_podman_stub` all **FAILED**
  (36 passed / 3 failed, exit 1; transcript `mutation-check.log`). The
  container security defaults are pinned by executing tests against the argv
  the runtime binary actually receives, not mirrored logic. Working tree
  restored to pristine `b5bbe45e` afterwards (`git status` clean, verified).
- Static gates: `check-abac-route-registry`, `check-arch`,
  `check-abac-exempt-handlers`, `check-migration-versions`,
  `check-mem-port-contracts`, `check-in-memory-state-stores` → all exit 0.
  The two `/api/v1/compute-targets*` exemption entries were removed and the
  frozen count lowered 52→50 (routes moved into the resolver — the correct
  direction).
- **flake.lock remote verification** (`flake-lock-remote-check.txt`): pinned
  rev `50ab7937` exists on GitHub, is the exact head of `nixos-24.11`, and
  the lock's `lastModified` (1751274312 = 2025-06-30T09:05:12Z) matches the
  commit's committer date exactly — internally consistent, resolvable pin.
  No nix binary exists in this sandbox, so `nix build .#agent-image` is
  deferred to the verifier (environment restriction, not a code defect).

## Verified working (no findings)

- **Abstraction** — `ComputeTarget` port trait
  (`gyre-ports/src/compute.rs`) with all five spec methods; four production
  backends in `gyre-adapters/src/compute/` (container, ssh incl.
  `SshDockerTarget`, kubernetes, local). All invoke real CLIs
  (docker/podman/ssh/kubectl); failure paths propagate errors (tests assert
  failure when the binary is absent). Minor non-blocking note: the port's
  `target_type()` returns `&'static str` rather than the spec sketch's
  `ComputeTargetType` enum — the enum exists in domain and drives
  persistence/dispatch; functionally equivalent.
- **Container backend** — `docker|podman run --detach` with
  `--network=none` (opt-in bridge), `--memory=2g`, `--pids-limit=512`,
  `--user=65534:65534`, worktree volume mount, runtime auto-detection;
  argv construction factored into `docker_run_args` and executed verbatim
  by `spawn_process` (proven by the stub-binary tests above).
- **SSH backend** — `SshDockerTarget` runs docker on the remote host with
  the same four defaults (asserted on the real argv), every remote argument
  single-quote shell-escaped (injection test), `ConnectTimeout=10` bounds
  probes; credentials from the target config JSON; `container_mode: false`
  legacy bare-process mode retained.
- **Kubernetes backend** — Pod via `kubectl apply` from a generated manifest
  (resource limits, serviceAccountName at pod level, securityContext
  runAsUser/Group 65534 + runAsNonRoot + no privilege escalation, image
  pull policy, DNS-1035/253-safe pod names); `kill_process` deletes the
  Pod, `is_alive` checks phase. `workingDir` is `/workspace` (round-3 fix,
  regression-tested end-to-end through a stub kubectl against the actually
  applied manifest, including no-server-local-volumes assertion). Explicit
  `kubectl_path` config option drives every invocation (stub-tested).
- **Persistence** — `ComputeTargetRepository` implemented by SQLite,
  PostgreSQL, and mem adapters; tenant-scoped list, default flag,
  duplicate-name guard (409), `has_workspace_references` real in mem too
  (shared workspace store). Migration `000033_compute_targets` pre-existed
  in base.
- **Workspace selection + fallback chain** — request `compute_target_id` →
  workspace assignment → tenant default → local container auto-detection →
  bare local process. Cross-tenant targets rejected at BOTH the
  workspace-assignment path and the spawn path (404, no existence
  confirmation); both tested with hard assertions including
  no-agent-left-behind checks.
- **Backend-aware lifecycle** — `register_spawned_agent` records handle +
  owning `SpawnBackend`; admin kill routes to `docker rm` / `kubectl
  delete` / remote `docker rm`; exit monitor drops locks before awaiting
  probes (no cross-backend stall).
- **CRUD API** — `/api/v1/compute-targets` (+`/:id`) with per-handler Admin
  role enforcement, tenant scoping on every read/write, 409 on
  referenced delete; ABAC resolver mappings registered; `PUT
  /api/v1/workspaces/:id` accepts `compute_target_id` (string or null)
  with tenant validation; UI WorkspaceSettings Compute dropdown wired to
  the real field.
- **Nix image build + `wl_image_hash`** — `flake.nix` builds
  agent-image/agent-image-docker/agent-image-tar from lockfile-pinned
  nixpkgs with fixed timestamps, image `User = 65534:65534`; spawn resolves
  the image digest before minting and `mint_with_workload` carries
  `wl_image_hash` in the initial JWT (EdDSA sign→verify round-trip test);
  Kubernetes records the cluster-pulled `imageID` post-scheduling — the
  spec sentence was amended (in the spec, per lifecycle) to state this
  backend split precisely, which matches the physical constraint.

## Notes for the verifier

- `cargo test --all` / nextest / GitHub CI not re-run here (owned by the
  verification stage, per assignment scope).
- `nix build .#agent-image` requires a nix binary unavailable in this
  sandbox; the flake and committed lock were verified statically and
  against GitHub (see above).
