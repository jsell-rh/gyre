# Gyre Development Guide

## Crate Structure (Hexagonal Architecture)

```
crates/
  gyre-common/     # Shared types, errors, Id - no external deps
  gyre-ports/      # Port traits (interfaces) - no infrastructure deps
  gyre-domain/     # Pure domain logic - depends ONLY on ports + common
  gyre-adapters/   # Adapter implementations (SQLite, etc.) - implements ports
  gyre-server/     # Binary: HTTP/WS server - wires domain + adapters
  gyre-cli/        # Binary: CLI + TUI - thin client
```

**Hexagonal boundary invariant:**
- `gyre-domain` MUST NOT import `gyre-adapters` or any infrastructure crate.
- Violation is caught by `scripts/check-arch.sh` and CI (will fail the build).

> **M33**: Project entity removed. Workspace is now the primary entity. All APIs use `workspace_id`.
> **M34**: `workspace_id`/`repo_id` non-optional on Task/Agent/MR. ABAC replaces RBAC middleware. Git URLs use workspace slug: `/git/{workspace_slug}/{repo_name}/...`

Dependency flow:
```
gyre-server --> gyre-domain --> gyre-ports --> gyre-common
gyre-server --> gyre-adapters --> gyre-ports --> gyre-common
gyre-cli    --> gyre-common
```

---

## Key Commands

Cargo uses each checkout's `target/` by default. To share build artifacts
across local worktrees, set `CARGO_TARGET_DIR` to an absolute path in your
shell; do not commit a machine-specific path to `.cargo/config.toml`.

```bash
# Build everything
cargo build --all

# Build release binaries
cargo build --release -p gyre-server -p gyre-cli

# Run all Rust tests
cargo test --all

# Run frontend component tests (vitest -- requires Node/npm)
cd web && npm test && cd ..

# Run Playwright E2E tests (M17.5 -- auto-starts gyre-server on port 2222)
cd web && npm run test:e2e && cd ..

# Format check
cargo fmt --all -- --check

# Lint (warnings are errors)
cargo clippy --all-targets --all-features -- -D warnings

# Architecture lint (enforces hexagonal boundaries)
bash scripts/check-arch.sh

# Decision-input parameter lint (flags parameters used only in log lines)
bash scripts/check-dead-parameters.sh

# Byte-slice truncation lint (flags &s[..N] panics on multibyte UTF-8)
bash scripts/check-byte-slice-truncation.sh

# Warn-continue creation lint (flags warn-and-continue on restriction policy creation)
bash scripts/check-warn-continue-creation.sh

# Inert enforcement lint (flags evaluate_/enforce_/verify_ calls whose results are discarded)
bash scripts/check-inert-enforcement.sh

# Ignored tool test lint (flags adapter fns shelling out with only #[ignore]d test coverage)
bash scripts/check-ignored-tool-tests.sh

# Id-from-sha lint (flags Id::new fed a commit SHA where another entity's id belongs)
bash scripts/check-id-from-sha.sh

# ABAC-exempt handler authorization lint (flags exempt-route handlers with no
# per-handler authorization decision, deferred enforcement, or unbacked
# mitigation claims)
bash scripts/check-abac-exempt-handlers.sh

# Unwritten adapter store-field lint (flags in-memory adapter collection
# fields read by getters but never populated by the store/save path)
bash scripts/check-unwritten-store-fields.sh

# Fail-open ref-resolution lint (flags resolve_ref() results defaulted with
# .unwrap_or_default()/.unwrap_or("") — a None there means the ref does not
# resolve; health signals must fail closed)
bash scripts/check-fail-open-ref-resolution.sh

# Task commit-attribution lint (flags task-labeled product-surface commits
# missing from their task's commits: frontmatter — they are invisible to
# review scoping; needs full git history)
bash scripts/check-task-commit-attribution.sh

# Auto-format
cargo fmt --all

# Watch mode (requires cargo-watch)
cargo watch -x "test --all"

# Run the E2E Ralph loop integration test (requires git on PATH)
cargo test -p gyre-server --test e2e_ralph_loop

# Run M17 integration test suites individually (all require git on PATH)
cargo test -p gyre-server --test api_integration      # 68 REST API contract tests
cargo test -p gyre-server --test auth_integration     # 21 auth + RBAC tests
cargo test -p gyre-server --test git_integration      # 12 git smart HTTP + merge queue tests
```

### Integration Test Suites

Six integration test files in `crates/gyre-server/tests/` each start a live server on a random port:

| File | Tests | Coverage |
|---|---|---|
| `e2e_ralph_loop.rs` | 1 | Full Ralph loop end-to-end: spawn -> clone -> push -> MR -> merge |
| `api_integration.rs` | 66 | REST API contract tests for all endpoints (M17.2) |
| `auth_integration.rs` | 21 | Auth matrix: valid tokens, invalid tokens, ABAC role enforcement (M17.4) |
| `git_integration.rs` | 19 | Smart HTTP clone/push, push gates, merge queue, commit provenance (M17.3) |
| `graph_integration.rs` | 30 | Knowledge graph extraction, node/edge CRUD, spec linkage, push-triggered extraction (M30) |
| `m18_oidc_integration.rs` | 8 | OIDC discovery document, JWKS Ed25519 JWK, JWT spawn token, JWT auth, token-info claims, JWT revocation after complete (M18) |

All tests bind to `127.0.0.1:0` (random port) and run safely in parallel. Require `git` on `PATH`.

> **Note for CI / integration tests:** Always use `git push origin HEAD:main` (not `git push origin main`) when pushing to an empty repo. GitHub Actions runners default to `init.defaultBranch=master`, so the local unborn branch may be named `master` even if the remote expects `main`.

### E2E Integration Test (`e2e_ralph_loop`)

`crates/gyre-server/tests/e2e_ralph_loop.rs` proves the full Ralph loop works end-to-end via real HTTP and git operations:

1. Spawns a live `gyre-server` on a random port
2. Creates a project, repo, and task via REST API
3. Calls `POST /api/v1/agents/spawn` to get a per-agent token + worktree
4. Clones the repo over Smart HTTP (`/git/...`) using the agent token
5. Creates a commit and pushes it back via Smart HTTP
6. Calls `POST /api/v1/agents/{id}/complete` to open a MR and transition to review
7. Enqueues the MR and waits for the merge processor to auto-merge
8. Verifies the commit appears on the target branch

---

## Branching Convention

| Branch pattern | Purpose |
|---|---|
| `main` | Always green, deployable |
| `feat/<name>` | New features |
| `fix/<name>` | Bug fixes |
| `chore/<name>` | Maintenance, deps, tooling |
| `docs/<name>` | Documentation only |
| `ci/<name>` | CI/CD changes |

Rules:
- Branch from `main`.
- All work lands via PR.
- PRs require CI green before merge.
- No force-push to `main`.

---

## Commit Message Convention

Format: `<type>(<scope>): <description>`

| Type | When to use |
|---|---|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation changes only |
| `style` | Formatting, no logic change |
| `refactor` | Code change that is not a fix or feature |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `build` | Build system, Cargo.toml changes |
| `ci` | CI/CD pipeline changes |
| `chore` | Dependency updates, tooling |
| `revert` | Revert a previous commit |

Scope is optional but recommended. Use the crate name or subsystem.

Examples:
```
feat(server): add WebSocket endpoint for agent connections
fix(domain): correct task status transition from review to done
docs(agents): update AGENTS.md with new crate structure
ci: cache cargo target directory in GitHub Actions
build(gyre-ports): add async-trait dependency
```

Enforced by `scripts/check-commit-msg.sh` (pre-commit hook on commit-msg stage).

---

## Pre-Commit Hooks

Install once per clone:
```bash
pre-commit install
pre-commit install --hook-type commit-msg
```

Hooks run automatically on `git commit`. To run manually:
```bash
pre-commit run --all-files
```

Hook summary (see `.pre-commit-config.yaml` for the full list; ~60 local hooks):
- `cargo-fmt` / `cargo-clippy`: formatting and lint with denied warnings
- `arch-lint`: hexagonal boundary enforcement
- `abac-route-registry`: every registered `/api/v1/` route must have an ABAC `RouteResourceMapping` entry (`abac_middleware.rs`) — unregistered routes get NO policy evaluation; also fails on growth in duplicate resolver entries (first-match makes later entries dead code)
- `migration-versions`: fails on duplicate Diesel migration versions (one silently never runs)
- `byte-slice-truncation`: fails on `&s[..N]` string slices that panic on non-char-boundary UTF-8
- `fail-open-ref-resolution`: fails on `resolve_ref(...)` results defaulted with `.unwrap_or_default()`/`.unwrap_or("")` — health/safety signals must fail closed
- `task-commit-attribution`: fails on task-labeled product-surface commits missing from their task's `commits:` frontmatter (invisible to review scoping)
- `dead-message-kinds`: fails on `MessageKind` variants with no emitter (dead spec delivery link)
- `no-em-dash`: rejects Unicode em-dashes in source
- `conventional-commits`: commit message format (commit-msg stage)

Several hooks have exemption files (`scripts/<check>-exemptions.txt`) seeded with pre-existing violations. Exemption lists are legacy debt: they should shrink, never grow.

### Adding a database migration

Diesel derives a migration's version from the directory name prefix (before the first `_`, dashes removed). Duplicate versions silently drop the second migration — its tables never get created on fresh databases. Always take the NEXT unused 6-digit sequence number:

```bash
ls crates/gyre-adapters/migrations/ | sort | tail -3
```

`scripts/check-migration-versions.sh` (pre-commit + CI) and the guard test `no_duplicate_migration_version_prefixes` both fail on duplicates.

---

## Architecture Decisions

Key specs to read before making changes:

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](../specs/development/architecture.md) |
| Design principles (invariants) | [specs/system/design-principles.md](../specs/system/design-principles.md) |
| Agent Gates & Spec Binding | [specs/system/agent-gates.md](../specs/system/agent-gates.md) |
| Spec Lifecycle Automation | [specs/system/spec-lifecycle.md](../specs/system/spec-lifecycle.md) |
| Platform model (ownership, orchestration, personas, governance) | [specs/system/platform-model.md](../specs/system/platform-model.md) |
| Spec Registry (manifest + ledger) | [specs/system/spec-registry.md](../specs/system/spec-registry.md) |
| Spec links (implements, supersedes, depends_on, conflicts_with, extends, references) | [specs/system/spec-links.md](../specs/system/spec-links.md) |
| Cross-repo dependency graph (auto-detect, breaking changes, cascade testing) | [specs/system/dependency-graph.md](../specs/system/dependency-graph.md) |
| Vision (7 principles: judgment not generation, right context, specs as artifact, feedback loop, challenge ceremony) | [specs/system/vision.md](../specs/system/vision.md) |
| Meta-Spec Reconciliation (safe iteration on personas, principles, standards) | [specs/system/meta-spec-reconciliation.md](../specs/system/meta-spec-reconciliation.md) |
| Realized Model (knowledge graph extracted from code, universal node types, architectural timeline) | [specs/system/realized-model.md](../specs/system/realized-model.md) |
| System Explorer UI (live architecture, moldable views, inline spec editing, preview modes) | [specs/system/system-explorer.md](../specs/system/system-explorer.md) |
| UI Journeys (Inbox/Briefing/Explorer/Meta-specs/Admin nav, journey-oriented navigation) | [specs/system/ui-journeys.md](../specs/system/ui-journeys.md) |
| Database & Migrations | [specs/development/database-migrations.md](../specs/development/database-migrations.md) |
| User management & notification system | [specs/system/user-management.md](../specs/system/user-management.md) |
| Full-text search (all entities, FTS5/tsvector, MCP tool) | [specs/system/search.md](../specs/system/search.md) |
| ABAC policy engine (attribute-based access, scope cascade, audit) | [specs/system/abac-policy-engine.md](../specs/system/abac-policy-engine.md) |
| Forge-native advantages | [specs/system/forge-advantages.md](../specs/system/forge-advantages.md) |
| Agent experience + legibility | [specs/development/agent-experience.md](../specs/development/agent-experience.md) |
| CI, docs, release | [specs/development/ci-docs-release.md](../specs/development/ci-docs-release.md) |

> `crates/gyre-server/build.rs` auto-runs `npm run build` when building gyre-server, so
> **Node.js ≥ 18 + npm are required** for a full build. Set `SKIP_WEB_BUILD=1` to skip the
> web rebuild and use the committed `web/dist/` instead (useful for Rust-only development
> or environments without Node). CI runs the web build in a separate `web-build` job and
> sets `SKIP_WEB_BUILD=1` is NOT set in the `test` job — ubuntu-latest runners have npm.
# Parallel sandbox controller and cockpit

`scripts/dev-controller.py` is the development loop. It imports task progress
from remote `main` and unfinished `worker/task-*` branches, including branches
left by the retired fleet. Local uncommitted files are not imported. The
controller rejects an obsolete loop process if one is still running.

```bash
python3 scripts/dev-controller.py sync                 # inspect imported state
python3 scripts/dev-controller.py status               # inspect ledger and logs
python3 scripts/dev-controller.py run --slots 1         # dispatch cloud attempts
python3 scripts/dev-controller.py run --slots 50 --launch-burst 8  # ramp a larger pool
python3 scripts/dev-controller.py run --only-task task-151 --slots 1  # trace one task
python3 scripts/dev-controller.py retry task-099        # retry a failed task
python3 scripts/dev-controller.py retry-all             # retry every failed task
node scripts/loop-dashboard.mjs                         # cockpit: http://127.0.0.1:7690
```

It requires the OpenShell `gyre-gyre` gateway, `gyre-enmaas` and
`gyre-github-rw` providers, `docker/dev-worker/policy.yaml`, the worker image,
`OPENSHELL_OIDC_CLIENT_SECRET`, and the committed worker model configuration. Runtime state is kept in
`.gyre-dev-controller/state.sqlite3` (SQLite WAL); attempts and logs are in
`.gyre-dev-controller/attempts/`. Keep this directory when restarting the
controller. Only one controller process may run at a time. `--slots` is the
desired upper bound; `--launch-burst` limits starts per scheduling cycle.
Implementation workers run concurrently. Final integration uses one lane
(sandbox checks, host suites, upstream promotion), so candidates are checked
against successive main commits instead of invalidating concurrent checks.
With more than one admitted slot, one slot is reserved for integration;
with one slot, implementation and integration take turns. Candidates have
dispatch priority over new implementation work.
Gateway admission starts at one and increases by one whenever a sandbox
reaches Ready. Provisioning timeouts and gateway transport failures impose
durable, jittered exponential backoff (30 seconds to 15 minutes). Existing
sandboxes keep running; after the backoff, exactly one additional sandbox
probes gateway recovery. A Ready signal resumes the gradual ramp.
An explicit `ConfigurationInvalid` response pauses admission until the
configuration is repaired and its failed task is retried.
The cockpit reads
the ledger, shows task dependencies, attempt history and logs, and writes the
live `.gyre-dev-controller/slots` control. Set slots to `0` to drain; running
attempts finish. `--max-attempts` limits worker attempts per retry cycle.
The cockpit shows desired and admitted slots, gateway condition, next retry
time, and tasks waiting for infrastructure. Infrastructure failures do not
consume work-attempt budget; they retry automatically after backoff. The
Needs attention section offers Retry and Retry all for task or configuration
failures; an explicit retry grants a fresh attempt budget. A checker exiting
with a code failure, or a failed host suite, returns its candidate to an
implementation worker with a durable `attempts/<check-id>/repair.md` handoff.
It includes the candidate/base SHAs and the last 64 KiB of the failed gate's
log. The seed task is reopened as `needs-revision`; both implementation and
independent review receive the findings. Three automatic repair cycles are
allowed before operator attention is required. A manual retry retains the
findings and resets the budget. Existing checker failures are migrated into
this repair path at startup. Operational failures without evidence of a code
failure remain distinct and can require an explicit retry.
The cockpit distinguishes tasks already complete on upstream main from
recorded merges shipped by this controller. These counts measure different
things; imported completion is not evidence of controller throughput.
The overview shows current coverage from the coverage matrix and its
history from `specs/coverage/SUMMARY.md` commits. The task table and detail
drawer link GitHub PRs whose branch, title, or explicit body task reference
matches a task. PR lookup uses `gh` and refreshes once per minute.
The task drawer's Agent stream follows OMP text and tool events live through
the local attempt log; Raw log shows sandbox transport and bootstrap output.
Attempts started before streaming was enabled have only their raw logs until
their next agent round. Prompt text and private thinking events are omitted.
Transient source-fetch failures leave the controller alive and retry on the
next cycle. Worker agent rounds are bounded to 30 minutes by default
(`GYRE_DEV_ROUND_TIMEOUT` overrides this); the sandbox checkpoints and pushes
the branch after each round, including a timed-out round. A disconnected
sandbox exec or failed Cargo registry download is retried in the same sandbox
rather than provisioning another.
Branch pushes use the remote's exact current SHA as their lease and retry
transport failures in the same sandbox. If an attempt still exits with
unpushed edits, the driver saves `attempts/<id>/recovery.patch` locally before
deleting the sandbox.
Implementation and review rounds keep separate OMP sessions inside the sandbox
so a timed-out round can continue its prior inspection without contaminating
the independent review. When a session exceeds 400 KB, the next round archives
it and starts from a bounded handoff: the last three agent text messages,
current task, and branch diff summary. This keeps long tool transcripts out of
the next model request.

New worker candidates require a successful review round before completion
can be nominated. Integration additionally runs the upstream static verifier
against the candidate code, followed by the candidate's verifier, and rejects
new entries in frozen exemption files. Candidate scripts are restored before
the integration reviewer sees the merge tree. Full Rust and frontend suites
still run on the exact merge SHA on the host before any upstream push.

The controller hashes task requirements and cited specs, retaining desired and
observed generations. Changed requirements reopen stale completion; a candidate
from the previous generation cannot ship. Missing dependencies and cycles are
reported explicitly. Frozen attempt bundles contain the scripts, prompts,
policy and model configuration, with a persisted SHA256 manifest.

Admission requires a complete recent remote inventory. Owned pending objects,
orphans, live attempts and deletion-pending objects all consume the slot budget.
Deletion runs in a bounded asynchronous queue; a delete response does not free
capacity until inventory confirms absence. Create timeouts retain an existing
pending sandbox while the autoscaler catches up, with increasing polling delays
and a 30-minute overall deadline (`GYRE_DEV_READY_TIMEOUT`).

`GYRE_DEV_MAX_CANDIDATES` (default 8) caps the integration backlog. The cockpit
reports queue age, gate pass rate, repair count and recorded deliveries/hour.
A failed cloud gate is reproduced on main in the same sandbox; a failing host
suite triggers the same full suites on its main base. Proven
baseline defects get a separately scoped prerequisite task; host infrastructure
outages retry the verified tree locally with backoff and no new sandbox.

When runnable implementation work drains, bounded fidelity auditors inspect up
to four sections at a time, at most two audit tasks concurrently. They record
production entry points and replayable acceptance probes, correct hollow claims
and reopen/create scoped implementation tasks. New task IDs are reserved in the
ledger. An auditor leases its coverage matrix and referenced task files; ordinary
implementation remains concurrent. The integrator regenerates the coverage
summary. Audit delivery is counted separately from product delivery; an empty
queue alone never proves GOAL is met. See [the controller audit](dev-loop-audit.md)
for the live verification limits.

All agent roles pin `enmaas-glm-5-3/rits/zai-org/glm-5-3` explicitly against
`https://api.enmaas.devshift.net/v1` using OpenAI completions. The model entry
is committed in `docker/dev-worker/models.yml`, including its 262144-token
context, 65536-token output limit, and disabled developer role. No model
discovery request is required. `GYRE_DEV_MODEL` overrides all roles;
`GYRE_DEV_IMPLEMENTATION_MODEL` overrides implementation only. The worker
stages the committed `config.yml` beside the model entry (`MODELS_YML` and
`CONFIG_YML` can override the files). No repetition or frequency penalty is
enabled.

Before starting the loop with this provider, run in your desktop session:

```bash
source .gyre-dev-controller/gateway.env
python3 scripts/dev-gateway.py --inference
```

`dev-gateway.py` replaces the local `gyre-gyre` registration with the current
spoke gateway and OIDC client, authenticates, and checks the service account
subject. It reads `OPENSHELL_OIDC_CLIENT_SECRET`, falling back to the ignored
`OPENSHELL-GOAL.md` note. The ignored `gateway.env` file supplies the credential
to subsequent controller runs in the same shell. `--inference` also runs the
GitHub and EnMaaS setup below. GitHub credentials come from `GITHUB_TOKEN`,
`GH_TOKEN`, or `gh auth token`. The committed `github.yaml` profile permits
Git clone/fetch/push, including POST to `git-receive-pack`. To repair only this
provider on an authenticated gateway, run
`python3 scripts/dev-inference.py --github-only`. To update only inference, use
`python3 scripts/dev-inference.py --smoke`.

Before dispatching new sandboxes, the controller checks that both required
providers exist. Missing providers pause admission as a shared controller
condition; they do not exhaust individual task budgets. Prior failed attempts
whose logs report a required provider missing return to the retry queue on
restart. Admission resumes automatically when the providers are available.

This reads `secret-tool lookup service pricetag key api-token`, imports or
updates the committed OpenShell profile, and creates or updates `gyre-enmaas`
with that credential. Authenticate `gyre-gyre` first if its login has expired.
The setup command never writes the API key to disk or puts it in command
arguments. The gateway injects Bearer authentication for EnMaaS; sandbox model
configuration resolves `ENMAAS_API_KEY` from the attached provider. `--smoke`
tests the pinned model using the worker's OMP configuration. `--smoke-only`
checks inference without changing the gateway. Existing sandboxes continue
with their staged configuration; new attempts use EnMaaS.

Bootstrap files are staged as one retryable bundle. Sandbox Cargo commands use
`cc`/`lld` and `/tmp/gyre-target`, overriding the checkout's `clang`/`mold`
linker settings.
The pinned worker image is built from `docker/dev-worker/Dockerfile`; it includes
`rustfmt`, Clippy, and `jj`.
OpenShell cannot accept loopback sockets, including those used by library
tests, so the sandbox checker typechecks every Rust target with Clippy and runs
the static and frontend build gates there. Performance tests also need stable
host resources. Before promotion, the controller runs `cargo test --all` and the
full frontend suite in an isolated host worktree at the exact verified merge SHA.
Its result is recorded in `attempts/<id>/host-tests.log`; a failure blocks PR
publication and merging. The host gate runs as a separate recorded process, so the
controller continues scheduling, reaping, and deleting sandboxes while it runs.

Every finished sandbox is deleted, including failed attempts. The controller
periodically retries deletion if a driver crashes or gateway deletion fails.
A Git clone/bootstrap failure retries within its existing sandbox and then
marks the task failed for explicit retry; it does not provision another
sandbox automatically. Prior failed attempts with clear provisioning or
gateway transport evidence migrate to automatic waiting on controller start.
Set slots to `0` to drain dispatch.

Each worker gets a unique `devloop/task-NNN/attempt-N` branch and may resume
from an old worker branch or a previous attempt. It checkpoints after each
agent round. Completion only nominates a candidate. A separate sandbox merges
that exact candidate with the current remote `main`, checks changed-line Rust
format and Clippy diagnostics, runs static architecture gates and the frontend
build, then reviews the integration tree. Long Clippy and web build
gates have a 30-minute timeout each (`GYRE_DEV_GATE_TIMEOUT` overrides it), so a
hung build cannot retain a checker sandbox indefinitely. The sandbox publishes
a verified merge commit after those gates and review pass. Its GitHub commit
page shows the task title, spec, candidate SHA, and the reviewed `## Shipped`
summary from the task file (or existing implementation notes for older tasks).
The cockpit task drawer links the shipped commit directly to GitHub.
The controller then
runs the full Rust and frontend suites on that exact merge SHA on the host and
blocks promotion if either fails. It creates a GitHub PR for the verified ref,
with the shipped summary, exact head/base, gates and bundle digest, and confirms
GitHub has that exact head. The cockpit links it immediately. `gh` must be
installed and authenticated in the controller environment. Default publication waits for GitHub checks on that exact head and uses
`gh pr merge --merge --match-head-commit` without an administrator bypass.
The GitHub merge commit retains the shipped description; its parents include
the verified integration tree. `--publication pr` still reconciles failed CI,
but leaves passing PRs open for human review.

Pending checks and review/rule requirements hold publication. API outages
back off on the same tree. Cancelled/timed-out Actions jobs are rerun at most
three times without allocating another sandbox. Failed checks preserve logs
and queue implementation repair. When the exact upstream base failed the
same workflow with the same named E2E failures, a scoped prerequisite repair
is proposed instead; additional or unclassified failures return to the feature
worker. Repair/rechecking updates the task's existing PR with a force-with-lease
on its controller-owned branch; the newly verified branch remains unchanged.
The cockpit reports PR conditions and any `--only-task` dispatch restriction.
Publication retries preserve the verified commit and use exponential backoff.
The controller checks the candidate
and base SHAs again before requesting the GitHub merge; if `main` moved, it
checks again on the new base. An attempt whose process exits while the
controller is down is recovered from its recorded exit status and remote branch
on restart.

The new round prompts in `specs/prompts/dev-*.md` are short and task specific.
The repeatable verification commands live in `scripts/dev-check.sh`, which the
independent checker runs against the integrated commit. When a new mechanical
failure class is found, add a check or focused test rather than appending it
to an ever growing agent prompt.
