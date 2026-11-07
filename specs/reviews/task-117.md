# Review — task-117 (Implement compute target model)

Spec: `agent-runtime.md` §3 "Compute Target Model" (rows 22–26 of
`specs/coverage/system/agent-runtime.md`).
Candidate under review: `536c76ad249a409c22f9a64b0a4d29587a59b177`
(work recovered into checkpoint commit `013a871a` after a worker lease
expiry; process commit `536c76ad` records the branch commits).
Base: `9bef21a4`. Evidence: `/tmp/stage/review-evidence/`.

## Round 4

What was independently verified (commands, exit codes, and outputs under
`/tmp/stage/review-evidence/`):

- **Trait + backends.** `ComputeTarget` port (`gyre-ports/src/compute.rs`:
  `name`, `target_type`, `spawn_process`, `kill_process`, `is_alive`) with
  `ContainerTarget`, `SshDockerTarget`/`SshTarget`, `KubernetesTarget`,
  `LocalTarget` in `gyre-adapters/src/compute/`. Backend-aware lifecycle:
  `SpawnBackend` recorded per agent (`register_spawned_agent`), admin kill
  routes through the owning backend (`admin.rs`), exit monitor probes
  liveness lock-free. Tested end-to-end:
  `admin_kill_terminates_process_via_registered_backend` kills a real
  process through the full router.
- **Kubernetes arm is genuinely pinned.** Mutation check M1: removing the
  `work_dir: "/workspace"` override makes
  `kubernetes_pod_working_dir_is_workspace_not_local_worktree` FAIL (it
  captures the applied manifest through a stub `kubectl` invoked by the
  real spawn dispatch).
- **Tenant guards are genuinely pinned.** Mutation check M2: removing the
  request-supplied `compute_target_id` tenant guard makes
  `spawn_rejects_cross_tenant_compute_target` FAIL. The workspace
  assignment path (`PUT /api/v1/workspaces/:id`) rejects unknown and
  cross-tenant IDs with 400 and does not partially apply; the spawn path
  re-checks stale workspace assignments (404).
- **Persistence.** SQLite/PostgreSQL adapters + migration `000033`
  (pre-existing at base, `UNIQUE(tenant_id, name)`); the mem adapter's
  `has_workspace_references` now scans a workspace store shared with
  `MemWorkspaceRepository` (wired in `lib.rs` `build_state` and
  `mem::test_state`), so the 409 delete guard is real in mem mode too.
- **CRUD + ABAC.** `GET/POST /api/v1/compute-targets`,
  `GET/PUT/DELETE /api/v1/compute-targets/:id` with resolver mappings in
  `abac_middleware.rs`; the two stale exemption entries removed and the
  frozen count lowered to 50; handlers derive `tenant_id` from auth
  (never the body) and gate mutations on the Admin role. All 21 enforced
  static gates exit 0 at the candidate (enforced-gates.log).
- **`wl_image_hash`.** Digest resolved pre-mint (`docker image inspect`
  local for Container, remote for SSH) and passed to
  `mint_with_workload`; EdDSA round-trip test asserts the claim survives
  sign→verify and is absent on bare mints. Kubernetes records the
  cluster-pulled `imageID` post-spawn; the spec sentence was amended
  (spec lifecycle) to state this backend split — the amendment is
  physically accurate (a Pod's `imageID` exists only post-scheduling,
  the JWT must pre-exist the Pod).
- **flake.lock.** Pinned rev `50ab7937…` verified to exist on
  nixos-24.11 (`api.github.com`, saved response) and its `lastModified`
  1751274312 matches the commit's committer timestamp to the second.
  The `narHash` is NOT recomputable in this sandbox (no nix binary;
  codeload/tarball endpoints blocked) — recorded as a verifier check:
  `cd docker/gyre-agent && nix build .#agent-image` must succeed; a
  wrong hash fails loudly at fetch time, it cannot silently build a
  different tree.
- **Focused suites.** adapters compute 41/0 (4 daemon-required ignored),
  server api::spawn 39/0, compute_target 24/0, workspaces 15/0,
  admin 33/0; web WorkspaceSettings 50/50 after `npm ci`. web/dist is
  reproducible from web/src (build.rs rebuilt it during the test runs;
  `git status` stayed clean).

Findings:

- [ ] **F1 (major, test): the container backend's security defaults are enforced in code but pinned by no executing test — mutation check M3 shows every suite stays green with the G8 flags removed from `spawn_process`.** Removed the four unconditional security args (`--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`) from `ContainerTarget::spawn_process` (guarded `if let Some(...)` versions instead, i.e. default-config containers spawn as root on the host network stack with no caps) and re-ran: adapters compute 41 passed / 0 failed, server api::spawn 39/0, compute_target 24/0. The `container.rs` "security default" tests are mirrored logic — they restate `t.network.as_deref().unwrap_or("none")` against struct fields rather than asserting the spawned `docker run` argv, exactly the self-confirming class the task rules exclude from closing a section. Nothing in the repository captures the local container arm's argv (no docker stub exists, unlike the kubectl stub for the kubernetes arm and the factored, asserted `docker_run_args`/`build_pod_manifest` of the SSH and k8s backends). Fix direction: mirror `ssh.rs` — factor the `docker run` argument construction out of `spawn_process` into a testable `fn docker_run_args(&self, config) -> Vec<String>` and assert the defaults (and that defaults do not linger alongside overrides) against the real vector, and/or drive the spawn path with a stub `docker`/`podman` binary on `PATH` (as the k8s tests do with `kubectl_path`) asserting `--network=none --memory=2g --pids-limit=512 --user=65534:65534` on the executed argv. Note the acceptance criterion "Container backend enforces security defaults" is satisfied in production code today (verified by reading `spawn_process`); the defect is that the section's closure cites suites that cannot detect the enforcement being dropped — a CISO G8 security invariant must not fail silently under refactor.

Non-blocking notes (no repair required):

- `ComputeTarget::target_type()` returns `&'static str` rather than the
  spec sketch's `ComputeTargetType` enum (the typed enum exists in the
  domain and types the persisted entity); functionally equivalent
  identification, sketch is illustrative.
- `SshDockerTarget::kill_process` discards the `remote_docker` result
  without logging (`let _ =`); a genuinely failed remote kill is silent
  (container backend logs the equivalent). Observability nit.
- `docker/gyre-agent/Dockerfile` is retained alongside the flake; the
  flake is the canonical pinned build path (documented in flake.nix),
  the Dockerfile serves non-nix environments.

Verdict: not approved (F1). All other acceptance criteria carry
independent evidence; the repair is small and well-scoped.
