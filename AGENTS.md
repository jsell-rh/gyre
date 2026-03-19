# AGENTS.md - Gyre Agent Entry Point

Gyre is an autonomous software development platform built in Rust.
It orchestrates AI agents through a structured Ralph loop: spec, implement, review, merge.

For full specs: [specs/index.md](specs/index.md)

---

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

Dependency flow:
```
gyre-server ──> gyre-domain ──> gyre-ports ──> gyre-common
gyre-server ──> gyre-adapters ──> gyre-ports ──> gyre-common
gyre-cli    ──> gyre-common
```

---

## Key Commands

```bash
# Build everything
cargo build --all

# Build release binaries
cargo build --release -p gyre-server -p gyre-cli

# Run all tests
cargo test --all

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
```

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

## Architecture Decisions

Key specs to read before making changes:

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](specs/development/architecture.md) |
| Design principles (invariants) | [specs/system/design-principles.md](specs/system/design-principles.md) |
| M0 milestone deliverables | [specs/milestones/m0-walking-skeleton.md](specs/milestones/m0-walking-skeleton.md) |
| Agent experience + legibility | [specs/development/agent-experience.md](specs/development/agent-experience.md) |
| CI, docs, release | [specs/development/ci-docs-release.md](specs/development/ci-docs-release.md) |

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
