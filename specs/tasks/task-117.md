---
title: "Implement compute target model"
spec_ref: "agent-runtime.md §3 Compute Target Model"
depends_on: []
progress: complete
coverage_sections:
  - "agent-runtime.md §3. Compute Target Model"
  - "agent-runtime.md §Abstraction"
  - "agent-runtime.md §Supported Backends"
  - "agent-runtime.md §Nix-Based Image Build"
  - "agent-runtime.md §Tenant and Workspace Configuration"
commits: ["71967943dd45cec83c6c0a2047948d774560efd7", "1ffe1397f4cf331544cdbc691fac2eeb2771f52b", "c6b520a1de330410641f3ac17386c2136d590676", "5482ae4f1cbbc58fb0c867a2cd45e9436e94b9b5", "e5b481de02a551664a7bece73d7423fc0be6df30"]
---

## Spec Excerpt

From `agent-runtime.md` §3:

**Abstraction:** Compute targets are pluggable backends for agent execution behind a single trait:

```rust
#[async_trait]
pub trait ComputeTarget: Send + Sync {
    fn name(&self) -> &str;
    fn target_type(&self) -> ComputeTargetType;   // Container, Ssh, Kubernetes
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;
    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;
    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool>;
}
```

**Supported Backends:**
- **Container** (Docker/Podman): `docker run` with security defaults (`--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`)
- **SSH**: SSH to remote host, `docker run` there
- **Kubernetes**: Create Pod with agent image

**Tenant and Workspace Configuration:**

```rust
pub struct ComputeTargetConfig {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub target_type: ComputeTargetType,
    pub config: serde_json::Value,
    pub is_default: bool,
    pub created_at: u64,
}
```

Workspace selects ONE compute target from tenant's list. Falls back to tenant default, then local container auto-detection.

**API:**
| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/compute-targets` | GET | List available targets |
| `POST /api/v1/compute-targets` | POST | Register new target (tenant admin) |
| `GET /api/v1/compute-targets/:id` | GET | Get target details |
| `PUT /api/v1/compute-targets/:id` | PUT | Update target config |
| `DELETE /api/v1/compute-targets/:id` | DELETE | Remove target |

## Implementation Plan

1. **Verify and extend domain types:**
   - Check if `ComputeTarget` trait already exists (it's referenced in the codebase)
   - Verify `ComputeTargetType` enum has Container, Ssh, Kubernetes variants
   - Verify `ComputeTargetConfig` entity in domain
   - Add `SpawnConfig` struct if missing: branch, repo_path, env_vars, agent_token, resource_limits

2. **Container backend:**
   - Verify/implement Docker/Podman backend with security defaults
   - `--network=none` by default (opt-in bridge via config)
   - Resource limits: `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`
   - Worktree mounted as volume

3. **SSH backend:**
   - Implement SSH compute target: connect via SSH, run `docker run` on remote host
   - SSH credentials stored in compute target config (host, port, key_path or key material)
   - Remote Docker invocation with same security defaults

4. **Kubernetes backend:**
   - Implement K8s compute target: create Pod with agent image
   - Pod spec: resource requests/limits, service account, namespace from config
   - `kill_process` → delete Pod
   - `is_alive` → check Pod phase

5. **Workspace configuration:**
   - Add `compute_target_id: Option<Id>` to Workspace entity if missing
   - Fallback chain: workspace config → tenant default → local container auto-detection
   - UI: Workspace Admin → Settings → Compute Target dropdown

6. **API endpoints:**
   - Verify existing compute-targets API exists and has full CRUD
   - Add workspace selection: `PUT /api/v1/workspaces/:id` with `compute_target_id`
   - Register routes and ABAC mappings

7. **Nix-based image build:**
   - Verify `docker/gyre-agent/flake.nix` exists or create it
   - Ensure reproducible build: `nix build .#agent-image`
   - Image hash recorded in JWT claims (`wl_image_hash`)

## Acceptance Criteria

- [ ] `ComputeTarget` trait with Container, SSH, and Kubernetes backends
- [ ] Container backend enforces security defaults
- [ ] SSH backend runs Docker on remote hosts
- [ ] Kubernetes backend creates/manages Pods
- [ ] `ComputeTargetConfig` persisted in database
- [ ] Workspace selects compute target with fallback chain
- [ ] Full CRUD API for compute targets
- [ ] Nix flake for reproducible agent image builds
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §3 (Compute Target Model) in its entirety. Existing compute target code: grep for `ComputeTarget` in `gyre-domain/src/compute_target.rs` and `gyre-server/src/`. Existing spawn code: `gyre-server/src/api/spawn.rs`. Container runtime: grep for `docker` or `podman` in the server. KV store for compute targets: grep for `compute_targets` in adapters. Route registration: `api/mod.rs`.

## Shipped

Implementation commits `02966511`...`2b26880` (merge repair `c79542d8`),
review-repair commits `1fc4470b`, `d404a439`, and `e5b481de`
(round 4), the `6a9739e`/`722179f0` base merges (`976b339a`,
`7e672231`), the task-092 base merge `b30c8546` (web/dist rebuilt
from merged sources), the round-4 base merge `a2bdf9a2`, the
round-5 base merge `d9e7785`, the round-6 base merge
`4ef962e48`, the round-7 base merge `37a8193cf`, the
round-8 clippy repair `5482ae4f1`, the round-9 SSH
name-validation repair `c6b520a1d`, and the round-10 base merge
`e28d79994`, and the round-11 base merge `79331c426` +
meta-spec test repair `1ffe1397f` carry the work
below. Evidence is saved
under `/tmp/stage/review-evidence/` (per-round subdirectories;
rounds 4–6 in `round4/`, `round5-merge-repair/`, and
`round6-merge-repair/`; round 7 in `round7-merge-repair/`; round 8
in `round8-clippy-repair/`; round 9 in
`round9-ssh-name-orphan/`; round 10 in
`round10-merge-repair/`; round 11 in
`round11-merge-repair/`).

### Behavior

- **Abstraction** — `ComputeTarget` port trait
  (`gyre-ports/src/compute.rs`: `name`, `target_type`, `spawn_process`,
  `kill_process`, `is_alive`) with `SpawnConfig`/`ProcessHandle`; four
  production backends in `gyre-adapters/src/compute/`.
- **Container backend** — `ContainerTarget` runs `docker|podman run
  --detach` with the spec security defaults enforced at spawn time:
  `--network=none` (opt-in bridge via config), `--memory=2g`,
  `--pids-limit=512`, `--user=65534:65534`; worktree mounted as a volume;
  runtime auto-detection (docker -> podman). Since round 4 the argv
  construction is factored into `ContainerTarget::docker_run_args`
  (same pattern as `SshDockerTarget`) and `spawn_process` executes
  exactly that vector, so the defaults are pinned by tests against
  both the built argv and the argv the runtime binary actually
  received (stub docker/podman spawn tests).
- **SSH backend** — `SshDockerTarget` (`ssh.rs`) shells out to `ssh` with
  batch mode + bounded connect timeout and runs `docker run` on the remote
  host with the same security defaults; every remote argument is
  single-quote shell-escaped (`shell_quote`). Credentials (user, host,
  port, identity file) come from the compute-target config JSON.
- **Kubernetes backend** — `KubernetesTarget` (`kubernetes.rs`) creates a
  Pod via `kubectl apply` from a generated manifest (resource
  requests/limits, service account, namespace, image pull policy from
  config; DNS-1035/253-char-safe pod names); `kill_process` ->
  `kubectl delete pod`; `is_alive` -> pod phase check. An explicit
  `kubectl_path` target-config option (pinned/cluster-specific CLIs,
  e.g. OpenShift `oc`) drives every kubectl invocation. The Pod's
  `workingDir` is `/workspace` — the agent image entrypoint clones its
  own checkout there; the server-local worktree path is never forwarded
  (round-3 fix, see below).
- **Persistence** — `ComputeTargetConfig` entity behind the
  `ComputeTargetRepository` port, implemented by SQLite, PostgreSQL, and
  mem adapters (tenant-scoped list, default flag, duplicate-name guard).
- **Workspace selection + fallback chain** — `Workspace.compute_target_id`;
  spawn resolution is request `compute_target_id` -> workspace assignment ->
  tenant default -> local container auto-detection. Cross-tenant targets
  are rejected at BOTH the workspace-assignment path (4xx) and the spawn
  path (404, indistinguishable from missing — no existence confirmation).
- **Backend-aware lifecycle** — each spawned agent records both its
  `ProcessHandle` and the `SpawnBackend` that owns it, so admin kill
  routes to `docker rm` / `kubectl delete` / remote `docker rm` instead of
  a pid-based local kill (a silent no-op for container/Pod handles).
- **CRUD API** — `GET/POST /api/v1/compute-targets`,
  `GET/PUT/DELETE /api/v1/compute-targets/:id` (plus the pre-existing
  admin variants) with ABAC resolver mappings; the two stale
  exemption-file entries were removed (frozen count lowered, then
  re-lowered to the merged baseline 50). `PUT /api/v1/workspaces/:id`
  accepts `compute_target_id` (string or null). UI: WorkspaceSettings
  "Compute" tab with a target dropdown.
- **Nix image build + `wl_image_hash`** — `docker/gyre-agent/flake.nix`
  builds the agent image from nixpkgs pinned by the committed
  `docker/gyre-agent/flake.lock` (nixos-24.11 head `50ab7937`, NAR hash
  byte-verified — see round 3 below), with `npm ci` from the committed
  package-lock.json and fixed timestamps (`agent-image`,
  `agent-image-docker`, `agent-image-tar`); image `User = 65534:65534`.
  The spawn flow resolves the image digest BEFORE minting the agent JWT
  (`docker image inspect`) for container and SSH backends and mints via
  `mint_with_workload`, so the `wl_image_hash` claim is populated in the
  initial token. Kubernetes records the cluster-pulled `imageID` in the
  post-spawn workload attestation record (`GET /api/v1/agents/:id/workload`)
  — the imageID only exists once the Pod is scheduled, and the JWT must
  pre-exist the Pod it is injected into. The spec sentence (§ Nix-Based
  Image Build) states this backend split precisely.

### Review-round repairs

Round 2 (`1fc4470b`): duplicate test block from a base merge (compile
failure), tenant-checking of request-supplied and workspace-assigned
`compute_target_id` (cross-tenant 404s), `wl_image_hash` minted into the
initial JWT, task-097 secret-delivery tests repaired onto the real
kubernetes dispatch via a stub `kubectl`, exemption-file housekeeping.

Round 3 (`d404a439`, findings from review `00e38774`):

- **Kubernetes Pods could not start in the standard flow** — the k8s
  dispatch arm forwarded the canonicalized SERVER-LOCAL worktree path as
  the Pod `workingDir`; the manifest carries no volumes and the agent
  image contains no such path, so under CRI the container failed to start
  (runc chdir ENOENT) and the agent never ran. Fixed by mirroring the
  SSH container arm: the k8s `SpawnConfig` overrides `work_dir` to
  `/workspace` (the entrypoint clones its own checkout there). Regression
  test `kubernetes_pod_working_dir_is_workspace_not_local_worktree`
  captures the applied manifest through the stub-kubectl spawn path and
  asserts `workingDir == "/workspace"` plus absence of server-local
  volumes; mutation-checked (fails without the fix).
- **`flake.lock` committed** — the flake previously left nixpkgs on the
  moving `nixos-24.11` branch ref with no lock file ("created on first
  run"), so two builds at different times resolved different nixpkgs.
  `docker/gyre-agent/flake.lock` now pins the branch head `50ab7937` with
  its NAR hash. No nix binary exists in this sandbox, so the lock node
  was constructed and then independently verified: a NAR serializer
  replicated from nix's own `archive.cc`/`serialise.cc` (byte format
  cross-checked against nix's committed binary fixture
  `tests/functional/duplicate.nar`) was validated against three
  nix-generated public lock nodes (numtide/flake-utils,
  nixpkgs@d02d88f8, nixpkgs@edf04b75 — all matched byte-for-byte) before
  hashing the locked revision; methodology and match transcripts in
  `/tmp/stage/review-evidence/flake-lock-verification.txt`. The flake.nix
  comment now states the lock is committed.
- **`docs/api-reference.md` table repair** — the tenant-scoped
  compute-targets block was inserted mid-table, breaking the tail of the
  admin endpoint table (bcp/scim rows rendered as paragraph text). The
  block now sits below the admin table; stray trailing `|` dropped.
- **Task-092 base merge resolved** (`b30c8546`) — conflicts were confined
  to generated `web/dist` artifacts; both branches changed `web/src`, so
  dist was rebuilt with vite from the merged sources (`npm ci` + build;
  full web vitest: 65 files, 1624 passed / 41 skipped) rather than taking
  either side's stale bundle.

Round 4 (`e5b481de`, finding from review of `536c76ad`):

- **Container security defaults were unpinned by executing tests** —
  the `container.rs` `security_default_*` tests restated the
  `unwrap_or(<default>)` fallbacks against struct fields (mirrored
  logic), so the reviewer's mutation — replacing the four unconditional
  security args in `spawn_process` with `if let Some(...)` guarded
  versions, letting default-config containers spawn as root with host
  networking — left every suite green. Fixed by (a) factoring the
  `docker run` argv construction into `ContainerTarget::docker_run_args`
  and spawning through it, (b) replacing the six mirrored tests with
  real-vector assertions (defaults, structural flags, and overrides
  with no-linger-on-default checks), and (c) two end-to-end tests that
  drive the real `spawn_process` through stub `docker` and `podman`
  binaries (PATH-scoped, serialized via a `PATH_LOCK`) and assert the
  four flags on the argv the stub actually executed. Mutation-checked:
  the reviewer's exact mutation now fails
  `docker_run_args_enforce_security_defaults` and both stub tests
  (transcript in `round4/mutation-check.log`, exit 101).

Round 5 (`d9e7785`, base merge `72ad2a128` — task-096/task-077
upstream):

- **Rebase repair only, no behavior change.** Conflicts resolved by
  keeping BOTH sides' independent test suites and doc text:
  `spawn.rs` keeps the backend-aware lifecycle tests and base's
  task-077 F6 fail-closed interrogation-policy test (merged test
  imports); `workspaces.rs` keeps the compute-target binding tests
  and base's task-077 trust-transition tests; the
  `docs/api-reference.md` workspaces/{id} row combines the
  `compute_target_id` and `trust_level` descriptions.
  `check-abac-route-registry.sh` takes the tighter frozen count on
  each axis (exemptions 50 = HEAD/merged file count; duplicates 17 =
  base/merged resolver count). One auto-merge semantic artifact fixed
  in `mem.rs`: `test_state_failing_policy_creates`/`_lists` (base
  additions) were updated to the merged 6-arg `test_state_inner` with
  a paired `ws_store`, like every other test-state builder.

Round 6 (`4ef962e48`, base merge `c5deca801` — task-105 log
compression upstream):

- **Rebase repair only, no behavior change.** The two content
  conflicts (`AppState` in `lib.rs`, `test_state_inner` in `mem.rs`)
  were both the same shape: HEAD added `spawned_backends` beside the
  old `agent_logs: Arc<Mutex<HashMap<String, Vec<String>>>>` while
  the base replaced `agent_logs` with the zstd-compressing
  `log_compression::AgentLogBuffer` (task-105). Resolution keeps
  both independent additions: `spawned_backends` (task-117 backend-
  aware kill/liveness routing, doc comment preserved) with the base's
  `AgentLogBuffer`-typed `agent_logs`. All other files auto-merged;
  `cargo check -p gyre-server --all-targets` confirms the merged
  usage sites compile. Focused suites on both merged features pass
  (see Test evidence).

Round 7 (`37a8193cf`, base merge `fefde2946` — task-089 HSI
Explorer Generated Views + task-098 secrets + agent-audit/rollout
upstream):

- **Rebase repair only, no behavior change to task-117 semantics.**
  Three content conflicts resolved by keeping BOTH sides'
  additions: `abac_middleware.rs` keeps the tenant-scoped
  compute-target resolver mappings beside base's secrets mappings;
  `admin_kill_agent` merges HEAD's backend-aware kill
  (`spawned_backends` routing) with base's `killed_pid` capture
  (handle PID recorded before registry consumption, `attested_pid`
  fallback, passed to `emit_killed`); `mem.rs` keeps the 3-tuple
  `shared_workspace_policy_pair`/`ws_store` plumbing (round-5
  shape) and updates every test-state builder to base's 7-arg
  `test_state_inner` (new `kv` parameter). `web/dist` kept as
  auto-merged from base; locally regenerated artifacts from the
  cargo-check build were discarded (executor restores dist from
  the assignment base). Focused suites on both merged features and
  the shared test-state builders pass (see Test evidence).

Round 8 (`5482ae4f1`, verification finding at `d13ea54a` —
`checks.sh` exit 1):

- **Clippy-diff gate failed on two task-117 changed lines.**
  `check-clippy-diff.py` reported (a) `clippy::double_must_use` at
  `gyre-ports/src/compute.rs` — the `ComputeTarget` trait was the
  only async port trait missing the `#[allow]` every sibling
  carries (async_trait generates a `#[must_use]` future per
  method), and (b) `clippy::type_complexity` at
  `gyre-server/src/mem.rs` — the round-5 merge shape of
  `shared_workspace_policy_pair` returned a raw 3-tuple. Fixed by
  adding the established `#[allow(clippy::double_must_use)]`
  attribute (same comment as the other ports) and factoring the
  tuple into a named `SharedWorkspacePolicyPair` type alias.
  Compile-only change, no behavior change. The verification log's
  non-fatal `WARN: stale exemptions` naming the compute-targets
  routes is not reproducible at HEAD: the exemption file at
  `d13ea54a` contains no compute-target entries and
  `check-abac-route-registry.sh` exits 0 with no WARN both before
  and after this round.

Round 9 (`c6b520a1d`, review finding at `5c8fbcd79` — SSH
agent-name validation orphaned state):

- **The SSH dispatch arm's agent-name charset/length validation
  returned 400 AFTER the agent record, agent token, git worktree +
  DB record, and task assignment were persisted**, leaving an
  Active agent, the task InProgress and assigned to that agent,
  and a worktree record behind on every rejected spawn — violating
  the handler's own compensating-rollback invariant (the
  worktree-creation-failure and interrogation-policy-failure paths
  both delete the just-created agent). The base had this late
  error only for opt-in `container_mode:true`; task-117's flip of
  the container_mode default to true put it on every default
  SSH-target spawn with a non-`[a-zA-Z0-9._-]` or >63-char name.
  Fixed by hoisting the check to the top of `spawn_agent_core`,
  beside the duplicate-name check and scoped to the same condition
  it applied under (resolved SSH compute target), before any
  write; the now-dead late check is deleted. Regression test
  `spawn_rejects_unsafe_agent_name_on_ssh_target_without_orphans`
  asserts the 400 plus absence of agent/task-assignment/worktree
  orphans; mutation-checked against the candidate's late-check
  shape (fails on "rejected spawn must not create an agent",
  transcript in `round9-ssh-name-orphan/mutation-check.log`).

Round 10 (`e28d79994`, rebase repair — base merge `774737638`,
task-177 rendering + task-099/140 process/spec-approval upstream):

- **Rebase repair only, no behavior change to task-117 semantics.**
  The single content conflict (`api/spawn.rs`) was the recurring
  monitor-loop shape: HEAD routes every spawn arm through
  `register_spawned_agent` (task-117 backend-aware lifecycle),
  while the base still carried per-arm inline registry inserts +
  exit monitors whose only semantic addition was
  `revoke_agent_task_secrets` on process exit (Platform Model §7,
  task-098). Resolution keeps HEAD's `register_spawned_agent`
  calls in all three conflicted arms (container, kubernetes,
  auto-detected container) and folds the base's secret revocation
  into the shared `monitor_spawned_agent_exit` Active→Idle block,
  so exit bookkeeping now covers every backend uniformly
  (the base only did it for container/SSH/local arms). All other
  files auto-merged; `web/dist` kept as auto-merged (executor
  restores dist from the assignment base).

Round 11 (`79331c426` + `1ffe1397f`, rebase repair — base merge
`b5a872087`, task-108 admin impersonation + task-116 meta-spec
prompt delivery + meta-spec repository split upstream):

- **Rebase repair plus one base-test adaptation.** The two content
  conflicts were both additive and resolved by keeping BOTH sides'
  additions: `lib.rs` keeps HEAD's `mem_workspace_store`
  (compute-target delete guard) beside base's
  `mem_meta_spec_binding_store`; `api/spawn.rs`'s `effective_work_dir`
  takes the base's task-106 shape (prefer the agent's jj workspace
  dir, else the plain-git worktree — HEAD's canonicalize-the-worktree
  behavior is the base's else-if branch) with a combined comment; the
  test-module imports are the union of both sides'. One auto-merge
  semantic artifact fixed in `mem.rs` (the recurring round-5 shape):
  base's new `test_state_with_jj_ops` /
  `test_state_with_jj_and_git_ops` builders updated to the merged
  3-tuple `shared_workspace_policy_pair` / 7-arg `test_state_inner`
  with a paired `ws_store`.
- **Base's task-116 meta-spec prompt-delivery test adapted to the
  real kubernetes dispatch** (`1ffe1397f`): the test drove the spawn
  through a Kubernetes-typed compute target whose `config.command`
  ran a local dump script — relying on the pre-task-117 dispatch
  where Kubernetes-typed targets fell through to the local
  LocalTarget arm. Task-117's real Kubernetes backend dispatches
  those targets to kubectl, so the local script never ran. Repaired
  with the same process-boundary double the task-097 tests use
  (round-2 precedent): a stub kubectl captures the Pod manifest that
  production `build_pod_manifest` applies, and the assertions read
  `GYRE_META_SPEC_SET_SHA` (must equal the response's
  `meta_spec_set_sha`) and `GYRE_META_SPEC_PROMPT` (must contain the
  seeded persona section) from `spec.containers[0].env`. Same
  assertion strength, observed against real dispatch code.

Round 12 (`a17041706`, requirements_changed re-integration — base
`dbf9edaea`, the published task-117 merge on main plus upstream
task-108/109/115/116/174 work, merged with the retained source
checkpoint `447119c70`):

- **Re-integration verification, no code repair required.** The
current task contract (`specs/tasks/task-117.md` at the new base)
differs from the retained checkpoint only in bookkeeping
(`progress: complete`, the verified-asset commit `71967943d` added
to the `commits:` frontmatter); the acceptance criteria and coverage
sections are unchanged. The base's task-117 surface —
`gyre-ports/src/compute.rs`, `gyre-adapters/src/compute/`, spawn /
compute-target / workspace handlers, `docker/gyre-agent/` — is
byte-identical to the retained checkpoint (the base's publication
descends from this candidate lineage), so the merge keeps it
unchanged while adding the upstream features. All base differences
are purely additive (spec-lifecycle config port/adapters/API,
signal_chain orchestrator registry, explorer view updates); the two
additive `mem.rs` blocks (`MemSpecLifecycleRepository`, the
`spec_lifecycle_configs` + `orchestrator_registry` test-state fields)
auto-merge cleanly beside task-117's `spawned_backends` state — no
content conflicts, no semantic artifacts to repair (the recurring
round-5/11 `test_state_inner` shape did not recur: the base's
changes are new struct fields, not signature changes). Evidence in
`round12-requirements-changed-merge/`.

### Test evidence

All logs with exit codes under `/tmp/stage/review-evidence/`
(rounds 2–3 at source `d404a439`, round 4 in `round4/` at source
`e5b481de`):

- Round 4 (`e5b481de`): `cargo test -p gyre-adapters compute --lib` ->
  **39 passed, 0 failed** (4 ignored: daemon-required integration
  tests; the six mirrored security tests were replaced by four
  mutation-killing argv/stub tests); `cargo test -p gyre-server --lib
  api::spawn` -> **39 passed, 0 failed**;
  `check-rustfmt-diff.py`/`check-clippy-diff.py` vs `HEAD^1` -> clean.
- Round 5 (`d9e7785`, merge repair): full `cargo test -p
  gyre-server --lib` -> **1413 passed, 0 failed**; `cargo test -p
  gyre-adapters compute --lib` -> **39 passed, 0 failed** (4 ignored);
  web vitest `WorkspaceSettings.test.js` -> **71 passed**;
  `cargo fmt --check` and `cargo clippy -p gyre-server --lib
  --all-targets` -> clean; abac-route-registry, arch,
  abac-exempt-handlers, mcp-write-tools, migration-versions,
  task-commit-attribution gates -> exit 0 (logs in
  `round5-merge-repair/`).
- Round 6 (`4ef962e48`, merge repair): `cargo test -p gyre-server
  --lib api::spawn` -> **41 passed, 0 failed** (task-117 lifecycle +
  base's task-077 F6 test); `log_compression` -> **11 passed**;
  `api::agent_logs` -> **2 passed**; `cargo test -p gyre-adapters
  compute --lib` -> **39 passed, 0 failed** (4 ignored);
  `cargo test -p gyre-server --lib compute_target` -> **24 passed,
  0 failed**; abac-route-registry, migration-versions, arch,
  task-commit-attribution gates -> exit 0 (log in
  `round6-merge-repair/evidence.log`).
- Round 7 (`37a8193cf`, merge repair): `cargo check -p gyre-server
  --all-targets` -> exit 0; `cargo test -p gyre-server --lib
  api::spawn` -> **43 passed, 0 failed**; `compute_target` -> **24
  passed, 0 failed**; `api::admin` -> **35 passed, 0 failed**
  (backend-aware kill + `killed_pid` audit); `api::agents` -> **9
  passed, 0 failed**; `api::workspaces` -> **19 passed, 0 failed**;
  `reconciliation::` -> **33 passed, 0 failed** (`kv`-store
  builders); `api::secrets` -> **6 passed, 0 failed**;
  `cargo test -p gyre-adapters compute --lib` -> **39 passed,
  0 failed** (4 ignored); `cargo fmt --check` -> exit 0; clippy -> no
  errors (remaining warnings pre-existing in base's
  `view_query_resolver.rs`); abac-route-registry,
  abac-exempt-handlers, forged-scope-fields, inert-enforcement,
  in-memory-state-stores, scope-literal-defaults,
  migration-versions, arch, task-commit-attribution gates -> exit 0
  (log in `round7-merge-repair/evidence.log`).
- Earlier rounds (`d404a439`): `cargo test -p gyre-server --lib
  compute_target` -> **24 passed**; web vitest on the merged sources
  -> **1624 passed, 41 skipped, 0 failed** (65 files); enforced static
  gates all exit 0 (arch, hierarchy, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, migration-versions,
  byte-slice-truncation, dead-message-kinds, mem-port-contracts,
  in-memory-state-stores, migration-sql-portability,
  relative-path-defaults, fail-open-ref-resolution,
  fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, unbounded-external-http,
  sandbox-sweep-artifacts); no new exemption entries.
- Round 8 (`5482ae4f1`, clippy repair): `check-clippy-diff.py HEAD^1`
  -> "changed lines clean" exit 0, and against the task assignment
  base `fefde2946` (all 17 task-117-changed Rust files) -> "changed
  lines clean" exit 0; full `cargo clippy --workspace --all-targets
  --all-features` -> exit 0 with no `double_must_use` or
  `type_complexity` on compute.rs/mem.rs changed lines (remaining
  warnings pre-existing outside the diff, e.g. base's
  `view_query_resolver.rs` and task-098's `MemSecretRepository`
  store field); full `cargo test -p gyre-server --lib` -> **1494
  passed, 0 failed**; `cargo test -p gyre-adapters compute --lib` ->
  **39 passed, 0 failed** (4 ignored); `check-rustfmt-diff.py HEAD^1`
  -> clean; arch, abac-route-registry, migration-versions,
  mem-port-contracts, in-memory-state-stores,
  task-commit-attribution gates -> exit 0 (logs in
  `round8-clippy-repair/`).
- Round 9 (`c6b520a1d`, SSH name-validation repair): `cargo test -p
  gyre-server --lib api::spawn` -> **44 passed, 0 failed**
  (43 previous + the new orphan regression test); mutation check
  (candidate's late-check shape restored) -> test FAILS on
  "rejected spawn must not create an agent" (exit 101, log in
  `round9-ssh-name-orphan/mutation-check.log`);
  `check-rustfmt-diff.py HEAD^1` / `check-clippy-diff.py HEAD^1` ->
  "changed lines clean"; `cargo test -p gyre-server --lib
  compute_target` -> **24 passed, 0 failed**.
- Round 10 (`e28d79994`, merge repair): `cargo check -p gyre-server
  --all-targets` -> exit 0; `cargo test -p gyre-server --lib` ->
  **1537 passed, 0 failed** (includes base's task-099/140 additions;
  `api::spawn` -> **47 passed** with the merged secret-revocation
  tests); `compute_target` -> **24 passed, 0 failed**;
  `api::admin` -> **36 passed, 0 failed**; `secret` -> **36 passed,
  0 failed**; `cargo test -p gyre-adapters compute --lib` -> **39
  passed, 0 failed** (4 ignored); web vitest `WorkspaceSettings.test.js`
  -> **72 passed**; `cargo fmt --check` -> exit 0;
  `check-clippy-diff.py` / `check-rustfmt-diff.py` against the task
  assignment base `774737638` -> "changed lines clean" (17 Rust
  files); arch, abac-route-registry, abac-exempt-handlers,
  forged-scope-fields, inert-enforcement, scope-literal-defaults,
  fabricated-scope-defaults, mem-port-contracts,
  in-memory-state-stores, migration-versions,
  task-commit-attribution gates -> exit 0 (log in
  `round10-merge-repair/evidence.log`).
- Round 11 (`79331c426` + `1ffe1397f`, merge repair): `cargo check -p
  gyre-server --all-targets` -> exit 0 (also `gyre-cli`); full
  `cargo test -p gyre-server --lib` -> **1627 passed, 0 failed**
  (includes base's task-108/116 additions; `api::spawn` -> **52
  passed** with the adapted meta-spec delivery test);
  `cargo test -p gyre-adapters compute --lib` -> **39 passed,
  0 failed** (4 ignored); web vitest `WorkspaceSettings.test.js` ->
  **72 passed**; `cargo fmt --check` -> exit 0;
  `check-clippy-diff.py` / `check-rustfmt-diff.py` against `HEAD^1`
  -> "changed lines clean" (58 Rust files); arch,
  abac-route-registry, abac-exempt-handlers, forged-scope-fields,
  inert-enforcement, scope-literal-defaults,
  fabricated-scope-defaults, mem-port-contracts,
  in-memory-state-stores, migration-versions,
  task-commit-attribution gates -> exit 0 (logs in
  `round11-merge-repair/`).
- Round 12 (`a17041706`, requirements_changed re-integration):
  `cargo check -p gyre-server --all-targets` -> exit 0; full
  `cargo test -p gyre-server --lib` -> **1659 passed, 0 failed**
  (includes upstream signal_chain/spec-lifecycle additions);
  `api::spawn` -> **52 passed, 0 failed**; `compute_target` ->
  **24 passed, 0 failed**; `cargo test -p gyre-adapters compute
  --lib` -> **39 passed, 0 failed** (4 ignored); arch,
  abac-route-registry, migration-versions, mem-port-contracts,
  in-memory-state-stores, task-commit-attribution gates -> exit 0;
  coverage rows 22–26 all `implemented`, no `not-started` rows in
  `specs/coverage/system/agent-runtime.md`; working tree clean after
  all runs (log and transcripts in
  `round12-requirements-changed-merge/`).
- `cargo test --all` / nextest / GitHub CI are owned by the verification
  stage; not re-run here per assignment scope.
