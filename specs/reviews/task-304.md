# Review — task-304 (Retire Dockerfile and complete backend security defaults)

Spec: `specs/system/agent-runtime.md` §3 Nix-Based Image Build ("One definition, all targets. The Dockerfile is replaced by the Nix flake.") + Supported Backends ("Container security defaults (all backends)").
Candidate under review: `591615d8` (branch `pipeline/task-304/2f374330ca4249b1aa8497d488cec05b-1`, base `88202d67`; product commits `95ab1dcb` + rustfmt `7e27cbdf`, rebase-repair merge `3c8e4bec` onto main `88202d67`, process `591615d8`).
Verdict: **approved** — no findings. All three acceptance criteria verified by inspection plus focused probes; the new Kubernetes tests were mutation-killed to prove they detect disabled behavior.

## Probes

Run at candidate `591615d8` (full log: `/tmp/stage/review-evidence/probes.txt`):

- `/tmp/stage/bin/cargo test -p gyre-adapters compute` → exit 0, **53 passed, 0 failed, 4 ignored** (live runtime/SSH-daemon tests). Includes the Container regressions (`docker_run_args_enforce_security_defaults`, `spawn_executes_security_defaults_via_docker_stub`/`podman_stub`), the SSH regression (`ssh_docker_run_args_enforce_security_defaults`), and all new k8s tests (pids default/override, default-deny policy scoping, RFC 1123 policy naming, apply-stream composition, stub-kubectl end-to-end stream, bridge opt-in, kill deleting both objects).
- **Mutation kill test** (restored afterwards; working tree clean): replacing `applies_network_isolation()` with `false` and deleting the `"pids"` manifest line made **4 tests fail** — including `spawn_uses_configured_kubectl_path`, which observes the real `Command`/stdin stream through the stub kubectl, not just the manifest builder. The tests are not self-confirming.
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (5m36s). Test-target codegen is blocked by the sandbox build wrapper (4 GiB) — deferred to the verifier (see below).
- `cargo fmt --check -p gyre-adapters -p gyre-server` → clean.
- Dockerfile-retirement sweep: `docker/gyre-agent/Dockerfile` gone; repo-wide `docker build` grep returns only the unrelated gyre-worker CI image (docker/dev-worker) and historical retirement notes in milestone docs; spawn.rs failure guidance now points at `nix build .#agent-image-docker` + `docker load` / `.#agent-image-tar`; `flake.lock` byte-identical, `flake.nix` comments-only, all three targets intact.

## Verified per acceptance criterion

- **AC1 (single image definition).** Dockerfile deleted; every remaining agent-image build instruction is Nix-based. The root `./Dockerfile` and `docker/dev-worker/Dockerfile` are the server and CI-worker images — different images, untouched, out of scope. `wl_image_hash` pre-spawn mint and the k8s post-spawn `imageID` attestation flow are untouched by the diff (auth.rs / workload.rs not in the diffstat).
- **AC2 (Kubernetes defaults).** `build_pod_manifest` sets `resources.limits.pids: "512"` (the supported per-Pod PID-limiting mechanism; overridable via `pids_limit` config, and the override REPLACES the default). Unless the config opts in (`"network": "bridge"`), `spawn_process` applies a platform-created default-deny NetworkPolicy in the same `kubectl apply -f -` YAML stream: both policyTypes, empty `ingress`/`egress` rule lists (deny-all semantics), `podSelector` scoped via the Pod's per-agent `gyre-agent` label — verified the label exists in the Pod manifest (`metadata.labels.gyre-agent` = pod name), so the policy selects exactly the spawned Pod. `kill_process` deletes both objects (best-effort). The bridge opt-in applies the Pod alone — regression-tested at the executed-invocation level (`spawn_bridge_opt_in_applies_pod_only`), not just the builder.
- **AC3 (Container/SSH retention).** `container.rs` and `ssh.rs` are byte-identical to the base (empty diff) and their pre-existing regression tests pass in this run — the full flag set (`--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`) plus the Container worktree volume are exercised through stub docker/podman/ssh invocations.
- **Remote worktree volume — spec amendment.** The blanket wording was amended in `agent-runtime.md` §3 by an attributed "Amendment (task-304)" paragraph carrying the concrete technical evidence (a git worktree's `.git` is a gitdir pointer into the server-local main repository, so a copy on a remote docker host / kubelet node has no object database and fails `git status`/`commit`/`push`; remote backends provision the working copy via the entrypoint's authenticated clone). This follows the repo's established amendment pattern (cf. the 2026-11-05 backend-split amendment recorded by task-117 in the same spec) and the goal's rule 4; it is an authorized spec change, not a code-comment deviation.
- **Rebase repair.** The conflicted file (`docs/agent-protocol.md`) preserves both sides: task-304's Nix/single-definition wording and task-310's `GYRE_WORKSPACE_ID` row + WebSocket-primary-transport section. `spawn.rs` auto-merge verified to carry both the Nix failure guidance and task-310's env injection. The auto-registered `gyre-agent-default` target still sets `"network": "bridge"` (lib.rs:2513), so the M25 zero-config flow keeps working under the none-default.
- **Coverage rows.** Rows 24 and 25 of `specs/coverage/system/agent-runtime.md` were extended with task-304 evidence matching the actual code — no unrelated rows touched, no verification claims beyond what the probes support.

Minor (non-blocking, recorded for completeness):

- `specs/coverage/system/lsp-call-graph.md` row 11 still cites `docker/gyre-agent/Dockerfile` as an existing path when describing agent-container toolchains — a stale cross-reference (not build guidance; it neither builds nor instructs building the image), and that row belongs to task-072/074 scope. A later sweep of that matrix may update the path reference.
- `kill_process` unconditionally attempts `kubectl delete networkpolicy <name>` even for bridge-opt-in targets where no policy was created — a best-effort delete that logs a warn on NotFound; harmless namespace hygiene, no correctness impact.
- `pids_limit` config parsing casts `u64 as u32`; an absurd value (>4.29e9) would wrap. Admin-controlled setting, no security impact.

## Verifier must run

- `cargo test -p gyre-server spawn` — in particular `kubernetes_spawn_applies_default_deny_network_policy_and_pids_limit` (observes the applied stream end-to-end through the real dispatch + stub kubectl). Server test-target codegen exceeds this sandbox's 4 GiB budget; the wrapper rejects `--all-targets`.
- `nix build .#agent-image .#agent-image-docker .#agent-image-tar` — no Nix in this sandbox; flake inputs unchanged in substance (flake.lock byte-identical, flake.nix comments only).
- Full workspace suite + Clippy per standard verification.
