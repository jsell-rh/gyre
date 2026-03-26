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

Hook summary:
- `cargo-fmt`: formatting check
- `cargo-clippy`: lint with denied warnings
- `arch-lint`: hexagonal boundary enforcement
- `no-em-dash`: rejects Unicode em-dashes in source
- `conventional-commits`: commit message format (commit-msg stage)

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

> `web/dist/` is committed so the server can serve the SPA without requiring `npm` at build
> time. Agents and CI do not need Node installed to build or run `gyre-server`.
