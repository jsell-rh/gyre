---
title: "Retire Dockerfile and complete backend security defaults"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / agent-image-single-definition / 4a3f940495a8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per agent-runtime.md §3 Nix-Based Image Build ('One definition, all targets. The Dockerfile is replaced by the Nix flake.') and Supported Backends ('Container security defaults (all backends)'). (1) Delete docker/gyre-agent/Dockerfile and every reference that builds or instructs building it: the spawn-failure guidance in crates/gyre-server/src/api/spawn.rs must direct operators to the Nix targets (nix build .#agent-image-docker then docker load, or #agent-image-tar for air-gapped transfer) instead of 'docker build -t gyre-agent:latest docker/gyre-agent/'; CI/scripts/docs referencing the Dockerfile are updated. Nothing in the production tree may build or direct operators to a non-reproducible agent image that can diverge from the attested one. (2) Container security defaults on all backends: the Kubernetes backend must enforce the defaults it currently lacks — a PID limit equivalent to pids-limit=512 expressed in the created Pod objects via the supported PID-limiting mechanism, and default network isolation equivalent to --network=none (e.g. a default-deny NetworkPolicy the platform creates alongside the Pod, with opt-in to bridged networking via compute target config) — while Container and SSH backends retain the full set (--network=none, --memory=2g, --pids-limit=512, --user=65534:65534, worktree volume). For the worktree volume mount on remote backends, the task must either provision the worktree content as a volume on the remote target or, if a default is demonstrably unsatisfiable on a remote backend, record the concrete technical evidence and process the amendment through the spec lifecycle — a code-comment deviation with no authorization is not an acceptable closure. The image-digest attestation flow (wl_image_hash pre-spawn, pod imageID post-spawn) is preserved.

## Acceptance Criteria

- [ ] docker/gyre-agent/Dockerfile does not exist; grep across the repo for agent-image build guidance returns only Nix-based instructions; nix build of the three flake targets still succeeds (flake.nix/flake.lock unchanged in substance).
- [ ] A Pod created by the Kubernetes backend carries a 512-equivalent PID limit and default-deny network isolation in the created API objects, with network access beyond that only when the compute target config opts in.
- [ ] Container and SSH backends still spawn with --network=none, --memory=2g, --pids-limit=512, --user=65534:65534 and the worktree volume (Container) — regression-tested, not just asserted.

## Verification intent

Trigger a container spawn with the agent image missing and read the server log, and list docker/gyre-agent/ — at the base the log instructs 'docker build -t gyre-agent:latest docker/gyre-agent/' (api/spawn.rs:1771) and the Dockerfile exists, building a materially different unpinned node:22-alpine image from the bit-reproducible flake image, so a default deployment following the server's own guidance runs agents on a non-reproducible image that diverges from the attested wl_image_hash; after the change both the file and the guidance must be gone. Additionally, inspect a Pod created by the Kubernetes backend — at the base it carries no PID limit and allows DNS egress with no platform-created network policy (kubernetes.rs:137-189 documents the deferrals in comments only), so the '(all backends)' security-defaults assertions fail.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "ebd16b0ebf8506b9c84eef25",
    "decision": "gap",
    "requirement_quote": "**Container security defaults** (all backends):\n- `--network=none` (opt-in to bridge via compute target config)\n- `--memory=2g`, `--pids-limit=512`\n- `--user=65534:65534` (non-root)\n- Worktree mounted as volume",
    "detail": "All three backends are real: ContainerTarget runs docker/podman with the full default set including the worktree volume mount; SshDockerTarget runs docker on the remote host with the same four flags (--network=none, --memory=2g, --pids-limit=512, --user=65534:65534); KubernetesTarget creates a Pod with memory 2Gi and runAsUser/runAsGroup 65534, runAsNonRoot, no privilege escalation. Gap against the '(all backends)' claim: (1) the Kubernetes backend sets no pids-limit (the code defers to kubelet-level cgroup limits the manifest cannot express) and has no --network=none equivalent (DNS egress is allowed; isolation is deferred to cluster-side network policies the platform does not create); (2) the worktree volume mount exists only on the local ContainerTarget — SshDockerTarget and KubernetesTarget mount no volume (the remote worktree cannot be mounted, and the agent image clones its own checkout). So two of the five specced defaults are absent on the Kubernetes backend and the volume mount is absent on two of three backends, with no spec amendment authorizing the deviations — the code comments document them as platform constraints, which per the audit rules is not a spec conflict without concrete evidence the contract cannot be satisfied.",
    "evidence": [
      {
        "file": "crates/gyre-adapters/src/compute/container.rs",
        "quote": "format!(\"--volume={}:{}\", config.work_dir, config.work_dir));",
        "detail": "The local ContainerTarget mounts the worktree as a volume and applies all four docker security defaults — the reference implementation of the specced default set."
      },
      {
        "file": "crates/gyre-adapters/src/compute/ssh.rs",
        "quote": "// Spec §3 security defaults (same as ContainerTarget).",
        "detail": "The SSH-docker backend carries the same four docker flags (network=none, memory=2g, pids-limit=512, user=65534:65534) — but its docker_run_args pushes no worktree volume mount, so 'Worktree mounted as volume' does not hold on this backend."
      },
      {
        "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
        "quote": "Kubernetes has no direct pids-limit",
        "detail": "The Kubernetes backend's manifest sets no per-Pod pids-limit — the code defers to kubelet-derived cgroup limits it does not configure — so one of the specced '(all backends)' security defaults is absent on this backend."
      },
      {
        "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
        "quote": "// Spec: --network=none equivalent — no egress except what the",
        "detail": "The Pod spec has no network isolation: DNS egress remains available and network-policy enforcement is deferred to the cluster admin, so the --network=none default does not hold on the Kubernetes backend."
      },
      {
        "file": "crates/gyre-adapters/src/compute/kubernetes.rs",
        "quote": "\"runAsUser\": 65534,",
        "detail": "Positive wiring: the Pod manifest does enforce the non-root user (65534:65534, runAsNonRoot, no privilege escalation) and the 2Gi memory limit — the defaults that can be expressed per-Pod are set."
      }
    ]
  },
  {
    "id": "3ee23b90e9236c59b010822f",
    "decision": "gap",
    "requirement_quote": "One definition, all targets. The `Dockerfile` is replaced by the Nix flake.",
    "detail": "The Nix side is fully implemented: docker/gyre-agent/flake.nix defines agent-image (buildLayeredImage, default), agent-image-docker (docker-loadable) and agent-image-tar (gzipped, air-gapped transfer) with pinned nixpkgs and a committed lockfile; the spawn flow resolves the image digest before minting the agent JWT (docker image inspect / remote inspect) into the wl_image_hash claim, and the Kubernetes backend records the actually-pulled pod imageID in the post-spawn workload attestation served by GET /api/v1/agents/:id/workload. GAP: the Dockerfile is NOT replaced — docker/gyre-agent/Dockerfile still exists and builds a materially different image (FROM node:22-alpine with unpinned packages, versus the bit-reproducible pinned flake), and the production spawn failure path actively directs operators to build it ('If the image is missing, build it first: docker build -t gyre-agent:latest docker/gyre-agent/'). A default deployment following the server's own error guidance therefore runs agents on a non-reproducible image that can diverge from the attested one — undermining both 'The Dockerfile is replaced by the Nix flake' and 'The image is the unit of consistency'. The spec already provides the no-Nix deployment path (nix build .#agent-image-tar for air-gapped transfer), so a live Dockerfile fallback is not required to satisfy the build-command set. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "`docker build -t gyre-agent:latest docker/gyre-agent/`.",
        "detail": "The production container-spawn failure path directs operators to build the legacy Dockerfile — the Dockerfile is a live deployment path, not vestigial, contradicting 'The Dockerfile is replaced by the Nix flake'."
      },
      {
        "file": "docker/gyre-agent/Dockerfile",
        "quote": "FROM node:22-alpine",
        "detail": "The legacy Dockerfile still exists and builds a materially different, non-pinned image than the Nix flake — two image definitions ship, so there is no single 'unit of consistency'."
      },
      {
        "file": "docker/gyre-agent/flake.nix",
        "quote": "#   nix build .#agent-image-tar      # gzipped tarball for air-gapped transfer",
        "detail": "Positive wiring: the flake defines all three specced targets (agent-image, agent-image-docker, agent-image-tar) with pinned dependencies — and the tar target already covers no-Nix environments, so the Dockerfile fallback is not needed for that case."
      },
      {
        "file": "crates/gyre-server/src/auth.rs",
        "quote": "pub wl_image_hash: Option<String>,",
        "detail": "Positive wiring: the wl_image_hash JWT claim carries the pre-resolved image digest for container/SSH backends; the Kubernetes backend records the pulled pod imageID in the post-spawn workload attestation."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": ".route(\"/api/v1/agents/:id/workload\", get(workload::get_workload))",
        "detail": "The post-spawn workload attestation record is served at the specced GET /api/v1/agents/:id/workload endpoint."
      }
    ]
  }
]
