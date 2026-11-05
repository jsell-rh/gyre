# Review — task-117 (Implement compute target model)

Spec: `specs/system/agent-runtime.md` §3 Compute Target Model (Abstraction, Supported
Backends, Nix-Based Image Build, Tenant and Workspace Configuration).
Candidate under review: `1810d83f` (base `d81b0fe4`; includes implementation commits
`02966511`…`2b26880`, merge repairs `c79542d8`/`7e672231`/`b30c8546`, review repairs
`1fc4470b` (round 2) and `d404a439` (round 3)). This is the fourth review round; the
round-1/2/3 durable findings (duplicate test block, cross-tenant spawn resolution,
`wl_image_hash` JWT claim, k8s Pod workingDir, missing `flake.lock`, api-reference
table breakage) were each re-checked at this candidate.

Verdict: **approved**.

## Probes (evidence: `/tmp/stage/review-evidence/`)

All probes ran at the exact candidate (`1810d83f`, working tree clean, sha recorded):

- `cargo test -p gyre-server --lib api::spawn` → **39 passed, 0 failed** — includes
  `kubernetes_pod_working_dir_is_workspace_not_local_worktree` (round-3 regression
  test), `spawn_rejects_cross_tenant_compute_target`,
  `spawn_rejects_stale_cross_tenant_workspace_assignment`, and the 5 secret-delivery
  tests repaired onto the real kubernetes dispatch.
- `cargo test -p gyre-adapters compute --lib` → **41 passed, 0 failed** (4 ignored:
  daemon-required Docker/SSH integration tests; no docker/podman/kubectl/nix binary
  in this sandbox).
- `cargo test -p gyre-server --lib compute_target` → **24 passed, 0 failed**
  (workspace bind + cross-tenant rejection + CRUD + delete-reference-guard).
- `cargo test -p gyre-server --lib mint_with_workload` → **1 passed**
  (`wl_image_hash`/`wl_pid`/`wl_container_id` EdDSA claim round-trip; absent when
  not passed).
- Web: `npx vitest run src/__tests__/WorkspaceSettings.test.js` → **50 passed,
  0 failed** (locked node_modules already installed). `web/dist` contains the
  rebuilt compute-target UI (`index-BgPtEtKd.js`).
- All 21 enforced static gates exit 0 at this candidate (arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, migration-versions, byte-slice-truncation,
  dead-message-kinds, mem-port-contracts, in-memory-state-stores,
  migration-sql-portability, relative-path-defaults, fail-open-ref-resolution,
  fabricated-scope-defaults, lossy-secret-conversion, scope-literal-defaults,
  inert-enforcement, forged-scope-fields, forwarded-header-trust,
  unbounded-external-http, sandbox-sweep-artifacts, task-commit-attribution).
  Exemption counts moved in the permitted direction only (ABAC exemptions 52→50 —
  the two compute-targets entries moved into the resolver; one stale
  relative-path prose-comment exemption dropped).

### Mutation checks (test-strength proof, source restored sha-verified after each)

1. **k8s workingDir fix**: reverted the `k8s_spawn_config` work_dir override so the
   dispatch passes the raw server-local `spawn_config` →
   `kubernetes_pod_working_dir_is_workspace_not_local_worktree` **FAILS**. The test
   kills the bug it claims to guard.
2. **Cross-tenant spawn guard**: replaced `Some(e) if e.tenant_id == spawn_tenant_id`
   with `Some(e)` in the request-supplied `compute_target_id` resolution →
   `spawn_rejects_cross_tenant_compute_target` **FAILS**. Real enforcement, real test.

Both edits reverted via `git checkout`; `sha256sum -c` confirms byte-identical
restoration; `git status` clean.

## Verification of prior durable findings (all resolved)

- **F: duplicate test block / E0428 compile failure (round 1)** — the full
  `gyre-server` lib test binary compiles and runs (filtered suites compile the whole
  module set); no duplicate definitions remain.
- **F: cross-tenant compute_target_id (round 1/2)** — `spawn.rs` now derives
  `spawn_tenant_id` from the workspace (falling back to the caller's auth tenant) and
  404s on tenant mismatch for BOTH the request-supplied id and a stale/tampered
  workspace assignment; two killing tests, mutation-verified by this review.
- **F: k8s Pod workingDir (round 3)** — k8s arm overrides `work_dir` to `/workspace`
  (mirroring the SSH container arm); regression test captures the applied manifest
  through the real spawn path with a stub kubectl and asserts `workingDir ==
  "/workspace"` plus no server-local volumes; mutation-killed by this review.
- **F: flake.lock not committed (round 3)** — `docker/gyre-agent/flake.lock` is
  committed, pinning nixpkgs `50ab793786d9de88ee30ec4e4c24fb4236fc2674` with a NAR
  hash. Independent remote check: the commit exists on NixOS/nixpkgs (GitHub API,
  HTTP 200) and its committer date `2025-06-30T09:05:12Z` equals the lock's
  `lastModified` 1751274312 exactly; the narHash decodes to 32 bytes. The NAR hash
  itself cannot be recomputed in this sandbox (no nix binary); note this is
  fail-closed — an incorrect narHash makes `nix build` fail hard at
  fetch-validation rather than silently degrading reproducibility. `flake.nix`
  comment now states the lock is committed.
- **F: `wl_image_hash` JWT claim (round 2/3)** — the spawn flow resolves the image
  digest before minting (`docker image inspect` locally, remote daemon digest for
  SSH container mode) and mints via `mint_with_workload`; claim round-trip tested.
  Kubernetes records the cluster-pulled `imageID` post-scheduling; the spec sentence
  (§3 Nix-Based Image Build) was amended via this task to state the backend split
  precisely — spec-amendment path used, as the review round required.
- **F: api-reference.md table breakage (round 3)** — the tenant-scoped
  compute-targets block now sits below the admin table with a blank line and its own
  header/delimiter pair; the bcp/scim rows are intact table rows again.

## Contract check against acceptance criteria

- `ComputeTarget` port trait (`gyre-ports/src/compute.rs`) with `name`,
  `target_type`, `spawn_process`, `kill_process`, `is_alive` and four production
  backends (container.rs, ssh.rs, kubernetes.rs, local.rs). The trait's
  `target_type()` returns `&'static str` rather than the spec sketch's
  `ComputeTargetType` enum — the enum exists and is used on the persisted entity;
  the string return is an equivalent realization of the same contract, not a
  behavior gap.
- Container backend enforces the spec security defaults at spawn
  (`--network=none` default with bridge opt-in, `--memory=2g`, `--pids-limit=512`,
  `--user=65534:65534`, worktree volume mount, docker→podman auto-detect).
- SSH backend runs `docker run` on the remote host with the same defaults; every
  remote argument single-quote shell-escaped; ssh batch mode + bounded
  ConnectTimeout; agent names validated to `[a-zA-Z0-9._-]{1,63}` before container
  use.
- Kubernetes backend: Pod via `kubectl apply` from a generated manifest (resource
  requests/limits, serviceAccountName, namespace, imagePullPolicy, DNS-1035/253-char
  pod names), kill → `kubectl delete pod`, is_alive → phase check, `kubectl_path`
  config for pinned CLIs. All kubectl args passed via `Command::arg` (no shell).
- `ComputeTargetEntity` persisted behind `ComputeTargetRepository` in SQLite,
  PostgreSQL, and mem adapters (tenant-scoped list, default flag, duplicate-name
  guard, workspace-reference delete guard — the mem adapter enforces in code per
  the port contract; mem-port-contracts gate green).
- Workspace selection + fallback chain: request id → workspace assignment → tenant
  default → local container auto-detection → bare local process only when no
  container runtime exists. Cross-tenant rejected at assignment (4xx) and spawn
  (404, indistinguishable from missing).
- Full CRUD at `/api/v1/compute-targets` with in-handler Admin-role checks for
  mutations, tenant scoping on every read/write (404 on foreign tenant), 409 on
  duplicate names and on delete-while-referenced; routes registered in the ABAC
  resolver (exemption entries removed, frozen count lowered). `PUT
  /api/v1/workspaces/:id` accepts `compute_target_id` (string or JSON null, kept
  distinct from absent via `deserialize_present`). UI: WorkspaceSettings compute
  dropdown bound to `compute_target_id`.
- Backend-aware lifecycle: `register_spawned_agent` records handle + owning
  `SpawnBackend`; admin kill routes to `docker rm` / `kubectl delete` / remote
  `docker rm` with LocalTarget pid-kill only for pre-task-117 records.
- `cargo test --all` is owned by the verification stage; every focused suite at
  this candidate is green and the full lib test binary compiles.

## Notes for the verifier (not defects)

- The flake.lock NAR hash should be confirmed on first real `nix build` in an
  environment with nix (fail-closed if wrong; implementer's serializer-based
  verification methodology is documented under their evidence directory).
- `cargo test --all` / nextest / GitHub CI remain to be run by verification.
