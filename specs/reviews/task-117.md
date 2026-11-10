# Review: task-117 — Implement compute target model

**Candidate:** `447119c702cd51e53b59a985533180371b0a2e4e` (base `b5a872087899e8bc4a18092ab56990a26e9ca9af`)
**Round:** 12 (independent review)
**Verdict:** approved — no findings.

## Scope reviewed

Diff base..candidate (27 files, +3464/−429): `gyre-ports/src/compute.rs` trait,
four backends in `gyre-adapters/src/compute/` (container, ssh incl.
`SshDockerTarget`, kubernetes, local), spawn dispatch + backend-aware
lifecycle in `api/spawn.rs`, compute-target CRUD + workspace binding,
mem/SQLite/PostgreSQL persistence, `docker/gyre-agent/flake.nix` +
committed `flake.lock`, coverage rows 22–26, docs, exemption-file
housekeeping (frozen count 52→50).

## What was verified

- **Trait + backends.** `ComputeTarget` port (name/target_type/
  spawn_process/kill_process/is_alive); Container runs `docker|podman run
  --detach` with the four spec §3 security defaults built in
  `docker_run_args` and executed through it; SSH `SshDockerTarget` runs
  docker on the remote host with every argument single-quote
  shell-escaped and bounded `ConnectTimeout=10`; Kubernetes applies a
  generated Pod manifest via `kubectl apply -f -` (stdin, argv-separated,
  no shell), `workingDir=/workspace`, kill=delete pod, is_alive=phase.
- **Scope handling.** Cross-tenant guards exist at BOTH the
  workspace-assignment path (`update_workspace`, 400) and the spawn path
  (request-supplied and stale workspace-assigned ids, 404 with no
  existence confirmation). CRUD handlers enforce Admin role for
  mutations in-body and tenant scoping on every read.
- **Fallback chain.** request id → workspace assignment → tenant
  default → local container auto-detection → bare local process.
- **Persistence.** SQLite/PostgreSQL/mem adapters implement
  `ComputeTargetRepository`; mem `has_workspace_references` scans the
  workspace store shared with `MemWorkspaceRepository` (delete guard
  tested in mem mode too).
- **Nix.** `flake.nix` builds from nixpkgs pinned by committed
  `flake.lock` (valid lock v7, rev `50ab7937`), `npm ci` from committed
  `package-lock.json`, fixed timestamps, image `User=65534:65534`.
  `wl_image_hash` minted into the initial JWT via `mint_with_workload`
  for container/SSH; Kubernetes records the cluster-pulled `imageID`
  post-spawn (spec amended to state this split — technically sound: the
  JWT must pre-exist the Pod; imageID exists only post-scheduling).
- **Durable rebase finding resolved.** The recorded merge conflicts
  (spawn.rs, lib.rs merging `8b1246aa` into base `b5a872087`) were
  resolved by merge `79331c426` keeping both sides' additions
  (backend-aware `register_spawned_agent`/`spawned_backends` + base's
  task-106 `effective_work_dir` shape and task-116 test adaptation); no
  conflict markers anywhere in the tree.
- **Frontmatter integrity.** `33c4ee018` briefly introduced a corrupted
  full SHA for the round-11 repair commit into `commits:`; the candidate
  tip `447119c70` itself corrected it to the real
  `1ffe1397f4cf331544cdbc691fac2eeb2771f52b`. Final frontmatter matches
  the assignment (5 commits) and `check-task-commit-attribution.sh`
  exits 0.

## Probes (logs in /tmp/stage/review-evidence/round12/)

- `cargo test -p gyre-adapters compute --lib` → 39 passed, 0 failed
  (4 ignored, daemon-required).
- `cargo test -p gyre-server --lib compute_target` → 24 passed.
- `cargo test -p gyre-server --lib api::spawn` → 52 passed.
- `cargo test -p gyre-server --lib api::admin::` → 36 passed.
- `cargo test -p gyre-server --lib api::workspaces::` → 19 passed.
- Static gates exit 0: arch, abac-route-registry, abac-exempt-handlers,
  mem-port-contracts, in-memory-state-stores, migration-versions,
  task-commit-attribution, inert-enforcement, forged-scope-fields,
  scope-literal-defaults.
- **Independent mutation checks** (each reverted; tree verified
  byte-identical to the candidate afterward):
  1. Drop the tenant guard on request-supplied `compute_target_id` in
     `spawn_agent_core` → `spawn_rejects_cross_tenant_compute_target`
     FAILS (exit 101).
  2. Drop the tenant re-check on workspace-assigned targets →
     `spawn_rejects_stale_cross_tenant_workspace_assignment` FAILS.
  3. Drop the cross-tenant check in `update_workspace` →
     `update_workspace_rejects_cross_tenant_compute_target` FAILS.

## Notes (non-findings)

- The spec's trait sketch returns `ComputeTargetType` from
  `target_type()`; the port returns `&'static str` because the enum
  lives in `gyre-domain`, which depends on `gyre-ports`
  (hexagonal layering). The enum is used for the persisted entity; the
  semantic contract (three backend kinds behind one trait) is met.
- Prior rounds' evidence subdirectories referenced in the shipped notes
  are not present in this fresh sandbox; round-12 evidence is
  self-contained.
- `cargo test --all` / nextest / CI are owned by the verification stage.
