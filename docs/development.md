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
within a checkout, set `CARGO_TARGET_DIR` to an absolute path in your shell;
do not commit a machine-specific path to `.cargo/config.toml`. Isolated baseline
and mutation worktrees need separate target directories. If a serialized gate
shares a target across worktrees, it must invalidate workspace packages first
(`python3 scripts/dev-cargo-clean.py`); otherwise Cargo can reuse the wrong
checkout's artifacts. The controller does this before every host Rust gate.

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
# Development reconcilers and cockpit

The development pipeline consists of independent discover/claim/run stages.
See [dev-pipeline.md](dev-pipeline.md) for state, claims, and execution semantics.

```bash
# Every stage can be triggered independently (including from cron).
python3 scripts/dev-pipeline.py discover triage
python3 scripts/dev-pipeline.py run triage
python3 scripts/dev-pipeline.py tick implement
python3 scripts/dev-pipeline.py tick review
python3 scripts/dev-pipeline.py tick verify
python3 scripts/dev-pipeline.py tick publish
python3 scripts/dev-pipeline.py tick cleanup

# Optional supervisor wakes all stages. Independent workers survive its restart.
python3 scripts/dev-pipeline.py slots 8
python3 scripts/dev-pipeline.py serve
python3 scripts/dev-pipeline.py serve --task task-210
python3 scripts/dev-pipeline.py status --json
python3 scripts/dev-pipeline.py retry task-210
python3 scripts/dev-pipeline.py retry-all

# Drain compute admission while existing workers finish and clean up.
python3 scripts/dev-pipeline.py slots 0
node scripts/loop-dashboard.mjs
```

State is private to `.gyre-pipeline/`: SQLite WAL, fenced claims, structured
findings, resources, and per-claim artifacts. `--state` or `GYRE_PIPELINE_STATE`
selects another absolute state directory. Stage concurrency is enforced by
claims, including when separate cron invocations race. Sandbox capacity includes
provisioning and pending deletion, rather than just live model processes.

The supervisor does not own task transitions. Each stage records its outcome;
subsequent discovery derives work from those outcomes and current upstream
state. Review and CI failures return actionable findings to implementation.
An infrastructure failure backs off on durable work. Checkpoints with complete
capture receipts can be reconstructed and published with an explicit Git lease.
Uncertain or corrupt capture receipts cannot authorize overwriting a branch.

The cockpit displays stage counts, pending retries, sandbox reservations,
coverage history, PR links, and live agent events. Streams retain collapse
controls, relative times, automatic following, and a return-to-bottom control.

Gateway setup remains `python3 scripts/dev-gateway.py --inference`. Authenticate
with `OPENSHELL_OIDC_CLIENT_SECRET`; the ignored gateway credential file can be
sourced into the shell running the supervisor. Inference setup reads
`secret-tool lookup service pricetag key api-token`. The attached providers are
`gyre-enmaas` and `gyre-github-rw`; the model is explicitly pinned to
`enmaas-glm-5-3/rits/zai-org/glm-5-3`. `python3 scripts/dev-inference.py --smoke-only`
checks inference without changing the configured providers.

Implementation and review each execute one bounded OMP assignment in an isolated
sandbox. Their prompts are `specs/prompts/pipeline-*.md`; repeatable deterministic
checks stay in scripts. Independent review cannot edit production code or
verifiers. Verification integrates the candidate with an exact main revision and
runs the maintained guards, full Rust tests, and frontend tests on the host.
Publication opens or updates a task PR, observes required checks, and requests an
exact-head merge without an administrator bypass. Delivery is recorded only
after observing the GitHub merge and checking its source tree.

To resume an existing unpublished task, seed its definition and exact source:

```bash
python3 scripts/dev-pipeline.py seed /absolute/path/task-210.md \
  --candidate FULL_COMMIT_SHA --base FULL_BASE_SHA
```

A seed is retained work, not an approval. Triage, independent review, verification,
and GitHub checks still apply. The old controller is not part of this pipeline.
