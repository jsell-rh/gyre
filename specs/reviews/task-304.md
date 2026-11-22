# Review — task-304 (Retire Dockerfile and complete backend security defaults)

Spec: `specs/system/agent-runtime.md` §3 (Nix-Based Image Build — "One definition, all targets. The Dockerfile is replaced by the Nix flake."; Supported Backends — "Container security defaults (all backends)").
Candidate under review: `f756b5ed` (branch `pipeline/task-304/c941d106e0204a9a8c8ec4e67ff86d07-2`, base `2b5189b9`; implementation commit `95ab1dcb`, rustfmt `7e27cbdf`, process/docs `26bdd7b3`/`f756b5ed`).
Verdict: **approved** — no blocking findings. All three acceptance criteria verified by inspection plus passing focused probes; the two probes this sandbox cannot run (server spawn-suite execution, `nix build` of the flake targets) are recorded below for the verifier.

Working-tree note: the checkout carries the executor's protected-task-text restoration of `specs/tasks/task-304.md` to its base content as an unstaged change; it is not part of the product diff and was not touched.

## Round 1

Probes run (evidence under `/tmp/stage/review-evidence/`):

- `/tmp/stage/bin/cargo test -p gyre-adapters compute` → **53 passed, 0 failed, 4 ignored** (live-runtime/SSH-daemon tests), exit 0 (`adapters-compute-tests.txt`).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (type-checks the new spawn.rs test code without linking; `server-check-tests.txt`).
- `/tmp/stage/bin/cargo fmt --check -p gyre-adapters -p gyre-server` → exit 0 (`fmt-check.txt`).
- `bash scripts/check-arch.sh` → exit 0 (`arch-check.txt`).
- Grep sweep for `docker build`/`Dockerfile` across README, docs/, docker/, crates/, scripts/, .github/ (`docker-build-grep.txt`) plus the full base→candidate diff read (12 files).

### AC 1 — Dockerfile retired, Nix-only guidance (verified)

- `docker/gyre-agent/Dockerfile` deleted (commit `95ab1dcb`); `docker/gyre-agent/` now contains only flake/lock/runner/entrypoint/proxy files.
- Spawn-failure guidance in `crates/gyre-server/src/api/spawn.rs` (`dispatch_agent_launch`, ~:1768) now directs to `nix build .#agent-image-docker` + `docker load < result`, or `.#agent-image-tar` for air-gapped transfer — exactly the required replacement; the image-digest attestation flow around it (`wl_image_hash` pre-mint, pod `imageID` post-spawn) is untouched by the diff.
- Repo-wide sweep: the only remaining `docker build` instruction builds the unrelated `gyre-worker` CI image (`docs/dev-pipeline.md:643`); every other Dockerfile mention is either a retirement/historical note (m24/m25 milestone docs, README M24 row) or the server/dev-worker Dockerfiles — none instruct building the agent image. `specs/coverage/system/lsp-call-graph.md:19` still names the old path inside an open task-072 row's historical audit note — not build guidance, and not this task's matrix (minor, non-blocking).
- `docker/gyre-agent/flake.nix` changes are comments only; `flake.lock` not in the diff — "unchanged in substance" holds, so the committed lockfile still applies. `nix build` of the three targets cannot run here (no Nix binary in the sandbox) — deferred to the verifier.

### AC 2 — Kubernetes PID limit + default-deny network isolation (verified)

- **PID limit:** `build_pod_manifest` emits `spec.containers[0].resources.limits.pids: "512"` — the supported per-container PID-limiting mechanism (kubelet maps the `pids` resource to the cgroup pids limit); overridable via `pids_limit` config key (`kubernetes_target_from_config`), override replaces (not rides alongside) the default. Tests: `manifest_has_security_defaults` (asserts `"512"`), `manifest_pids_limit_override_replaces_default`, and the stub-kubectl spawn test asserts the *applied* stream carries `"512"` end-to-end.
- **Network isolation:** `build_network_policy_manifest` produces a `networking.k8s.io/v1` NetworkPolicy with `policyTypes: [Ingress, Egress]` and **empty** `ingress`/`egress` rule lists (deny-all of both types), `podSelector.matchLabels` on the spawned Pod's `gyre-agent` label — which `build_pod_manifest` sets (pre-existing label, base kubernetes.rs:182), so the policy really selects the created Pod. It is applied in the same `kubectl apply -f -` stdin stream as the Pod (`build_apply_stream`, `---`-separated JSON docs — valid YAML stream), so a policy that fails validation fails the whole apply (fails closed). `kill_process` deletes both objects (tested via argv-recording stub: `delete pod <id>` and `delete networkpolicy <id>-net-isolate`).
- **Opt-in:** `"network": "bridge"` in the compute-target config skips the policy — the Pod alone is applied (`spawn_bridge_opt_in_applies_pod_only` asserts the captured stream is one document). `applies_network_isolation` treats every value other than exactly `"bridge"` as isolated (fail-safe against typos). Same opt-in surface as the container backend; the auto-registered `gyre-agent-default` container target sets `"network": "bridge"` (lib.rs:2513), consistent with the README.
- **End-to-end:** `kubernetes_spawn_applies_default_deny_network_policy_and_pids_limit` drives the real `POST /api/v1/agents/spawn` route with a Kubernetes-typed target whose `kubectl_path` is a stdin-capturing stub, then asserts the applied Pod carries pids=512 and the applied policy is default-deny and scoped to the spawned Pod's name. Type-checked here (`cargo check --tests`); execution deferred to the verifier (test-binary linking exceeds the sandbox build budget — see below).

### AC 3 — Container/SSH regression (verified)

- `docker_run_args_enforce_security_defaults` and `spawn_executes_security_defaults_via_docker_stub`/`podman_stub` assert `--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534` and the worktree `--volume=<worktree>:<worktree>` on the container backend; `ssh_docker_run_args_enforce_security_defaults` asserts the same four flags on the SSH backend. All pass in the 53-test adapters compute run. No assertion weakened; the pre-existing override tests confirm the defaults are genuine defaults.

### Worktree volume on remote backends — spec amendment (verified)

The task's authorized closure path was taken: `specs/system/agent-runtime.md` §3 now carries an explicit "Amendment (task-304)" paragraph recording the concrete technical evidence (a git worktree's `.git` is a file pointing into the server-local main repository at an absolute path, so a copy shipped to a remote docker host or kubelet node has a dangling `.git` linkage and no object database — `git status`/`commit`/`push` fail from it), with remote backends provisioning a content-identical, self-contained checkout via the entrypoint's authenticated clone. The evidence is technically accurate, recorded in the spec (not a code comment), and follows repo precedent for §3 amendments (the 2026-11-05 imageID backend-split amendment was a direct spec edit, coverage row 25). The spec-lifecycle post-receive machinery (auto drift-review task) fires on merge to the default branch — outside a task branch's scope.

### Coverage matrix

Rows 24/25 of `specs/coverage/system/agent-runtime.md` updated with concrete source evidence scoped to this task (pids limit mechanism and location, policy composition, Dockerfile deletion, guidance change); no unrelated rows touched.

Test-inflation check: none found. The new k8s tests assert the actual serialized manifests/streams captured from the production spawn paths (manifest builder output, kubectl stdin, kubectl argv) — each fails if the corresponding production behavior is removed; the kill test asserts both deletions from recorded argv. No conditional-guard or assertionless tests added (`spawn_without_kubectl_errors`' environment guard is pre-existing).

Minor (non-blocking, recorded for completeness):

- The `gyre-agent` label value is the full pod name (up to 253 chars); Kubernetes label values max at 63 chars, so an agent name longer than 63 chars makes the API server reject the apply — the whole spawn fails loudly (not a silent isolation loss), and the Pod label pattern is pre-existing at base (kubernetes.rs:182). The NetworkPolicy's selector repeats the same name, so no new failure class is introduced. Future hardening candidate, out of this task's scope.
- Pod scheduling can begin before the NetworkPolicy is admitted (both objects are in one apply stream, applied sequentially Pod-first) — a transient race inherent to object-based isolation, bounded by the single-stream apply; not a spec deviation.

## Verifier must run (cannot run in this sandbox)

1. `cargo test -p gyre-server spawn` — full spawn suite, in particular `kubernetes_spawn_applies_default_deny_network_policy_and_pids_limit` (type-checked here; test-binary linking exceeds the sandbox 4 GiB budget).
2. `cd docker/gyre-agent && nix build .#agent-image .#agent-image-docker .#agent-image-tar` — no Nix binary in this sandbox; flake.nix/flake.lock unchanged in substance (comment-only diff), so the committed lockfile applies.
3. Standard full-suite + mechanical checks on the merged tree.
