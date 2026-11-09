# Review — task-117 (Implement compute target model)

Spec: `agent-runtime.md` §3 Compute Target Model (Abstraction, Supported
Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate under review: `d13ea54ac10298b7d68efdbd4e3a991b1600ea4a`
(base `fefde2946`). Evidence: `/tmp/stage/review-evidence/` (this stage's
fresh probes; earlier rounds' evidence lived in prior stage sandboxes).

## Round 8 (this review — fresh independent verification after the round-7
base merge and the shipped-notes process commit)

No findings. Approve.

Verified working, with independent evidence:

- **Trait abstraction** — `gyre-ports/src/compute.rs` matches the spec
  signature (`name`, `target_type`, `spawn_process`, `kill_process`,
  `is_alive`) with `SpawnConfig`/`ProcessHandle`; four production backends
  (`container.rs`, `ssh.rs`, `kubernetes.rs`, `local.rs`) plus the legacy
  `docker.rs` updated to the trait.
- **Container security defaults are unconditional and pinned on the executed
  argv** — `ContainerTarget::docker_run_args` emits `--network=none`,
  `--memory=2g`, `--pids-limit=512`, `--user=65534:65534` via
  `unwrap_or(<spec default>)`; `spawn_process` executes exactly that vector.
  Mutation check (round-4's exact mutation re-applied: flags made
  `if let Some`-conditional) → exit 101, killed by
  `docker_run_args_enforce_security_defaults` AND both stub-runtime tests
  (`spawn_executes_security_defaults_via_docker_stub`/`_podman_stub`),
  which assert the flags on the argv a stub docker/podman binary actually
  received. Restored; suite re-run green (39 passed, 4 ignored
  daemon-required).
- **SSH backend** — `SshDockerTarget` runs `docker run` on the remote host
  with the same four defaults, every remote argument single-quote
  shell-escaped (`shell_quote`; hostile-image-tag test), batch mode +
  `ConnectTimeout=10` bounding, credentials from target config JSON.
- **Kubernetes backend** — Pod via `kubectl apply -f -` (stdin, no shell),
  manifest with `runAsUser/Group 65534`, `runAsNonRoot`,
  `allowPrivilegeEscalation: false`, memory/cpu limits, pod-level
  `serviceAccountName`, DNS-1035/253-safe pod names matching the handle id;
  kill = `kubectl delete pod`, is_alive = phase check. Round-3 fix
  re-verified by mutation: forwarding the server-local worktree as
  `workingDir` (the defect shape) → exit 101, killed by
  `kubernetes_pod_working_dir_is_workspace_not_local_worktree` (stub-kubectl
  end-to-end through the real spawn API, also asserts no server-local
  volumes). Restored; `api::spawn` re-run green (43 passed).
- **Persistence + workspace selection** — `ComputeTargetEntity` behind
  `ComputeTargetRepository` (SQLite/Postgres/mem; duplicate-name guard and a
  real workspace-scan `has_workspace_references` in mem). Fallback chain
  request id → workspace assignment → tenant default → local container
  auto-detection implemented in `spawn.rs`; cross-tenant targets rejected at
  BOTH the assignment path (400, `update_workspace_rejects_cross_tenant_*)
  and the spawn path (404, no existence oracle — request-supplied AND
  stale/tampered workspace-assigned ids, both tested, with no-orphan-agent
  assertions).
- **Backend-aware lifecycle** — `SpawnBackend` recorded per agent at spawn;
  admin kill routes to `docker rm`/`kubectl delete`/remote `docker rm`
  instead of a pid-based local kill; `killed_pid` captured before registry
  consumption with attestation fallback (round-7 merge kept both sides).
- **CRUD API** — tenant-scoped `/api/v1/compute-targets` CRUD with
  Admin-only writes, tenant checks on every read/update/delete, 409 on
  workspace-referenced delete; ABAC resolver mappings present; exemption
  file contains no compute-target entries; all 20 static gates exit 0
  (including abac-route-registry at frozen count 50 and
  task-commit-attribution: the two non-merge product-surface commits in
  `fefde2946..HEAD` — `013a871a` checkpoint, `e5b481de` round-4 repair —
  are exactly the task frontmatter's `commits:` list).
- **Nix image build + `wl_image_hash`** — `docker/gyre-agent/flake.nix`
  builds `agent-image`/`agent-image-docker`/`agent-image-tar` from nixpkgs
  with fixed timestamps and image `User = 65534:65534`; spawn resolves the
  image digest BEFORE minting (`docker image inspect`) for container/SSH
  and mints via `mint_with_workload` (EdDSA round-trip test on the
  `wl_image_hash` claim); Kubernetes records the cluster-pulled `imageID`
  in the post-spawn workload attestation record. The spec sentence was
  amended (spec-lifecycle path) to state this backend split precisely.
- **flake.lock pin** — verified via GitHub API (only reachable host):
  rev `50ab7937…` exists, IS the head of `nixos-24.11`, and `lastModified`
  1751274312 matches the commit date. The narHash could not be recomputed
  here (codeload.github.com blocked by the sandbox; no nix binary) —
  recorded as a verifier check (`nix flake lock --check` /
  `nix build .#agent-image` where nix + full egress exist), a transport
  restriction, not a code defect.
- **UI** — WorkspaceSettings "Compute" tab with target dropdown;
  `npm ci` + vitest on `WorkspaceSettings.test.js` → 72 passed.

Focused suites (all exit 0, logs under `/tmp/stage/review-evidence/`):
`cargo test -p gyre-adapters compute --lib` (39), `api::spawn` (43),
`compute_target` (24), `api::admin` (35), `api::workspaces` (19), web
WorkspaceSettings vitest (72). Coverage matrix rows 22–26
(`specs/coverage/system/agent-runtime.md`) marked implemented with
evidence consistent with the code as verified above.

Minor observations, not approval-blocking:

- Two supplementary failure-path tests (`ssh_docker_spawn_without_ssh_errors`,
  `spawn_without_kubectl_errors`) no-op when the binary exists in the
  environment (conditional guard). They are extras, not the tests that pin
  the sections — the pinning is done by the stub-binary and argv tests, both
  mutation-verified here.
- `spawned_backends` is an inline `Arc<Mutex<HashMap>>` mirroring the
  pre-existing `process_registry`. Legitimately process-local runtime
  bookkeeping (both maps lose live handles together on restart; no
  acceptance-criterion endpoint reads it), consistent with the
  in-memory-state-stores gate's target (durable queryable records) and the
  established `process_registry` pattern.
- `web/dist` regenerated by cargo builds during probing was restored from
  git (assignment base state); post-restore test run used `SKIP_WEB_BUILD=1`.

Verdict: approved (`/tmp/stage/verdict.json`,
`approved: true`, findings `[]`).
