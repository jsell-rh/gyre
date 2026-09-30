# AGENTS.md - Gyre Agent Entry Point

Gyre is an autonomous software development platform built in Rust.
Humans design (specs), orchestrators decompose (tasks), agents implement (Ralph loops).
See [specs/system/agent-runtime.md](specs/system/agent-runtime.md) §1 for the canonical agent lifecycle definition.

**This file is the map. Follow the links below for detail.**

---

**Current goal:** [specs/GOAL.md](specs/GOAL.md) — close every spec gap with real production implementations, no fakery (no stubs, no audit-only where enforcement is specced, no test inflation). Read it before working on any task.

## Quick Start

```bash
# Build and run (requires Node.js ≥ 18 + npm — build.rs auto-builds the Svelte UI)
cargo build --all
cargo run -p gyre-server        # starts on port 3000, token: gyre-dev-token

# Rust-only build (skip web rebuild, uses committed web/dist/)
SKIP_WEB_BUILD=1 cargo build -p gyre-server

# Run tests
cargo test --all
cd web && npm test && cd ..     # frontend component tests (vitest)

# Access
open http://localhost:3000      # Svelte SPA dashboard
```

See [docs/server-config.md](docs/server-config.md) for all configuration options.

---

## Codebase Map

| Layer | Crate | Role |
|---|---|---|
| Shared types | `gyre-common` | Errors, Id, protocol types -- no external deps |
| Interfaces | `gyre-ports` | Port traits (interfaces) -- no infrastructure deps |
| Domain logic | `gyre-domain` | Pure business logic -- depends ONLY on ports + common |
| Adapters | `gyre-adapters` | SQLite, Diesel ORM implementations -- implements ports |
| Server | `gyre-server` | HTTP/WebSocket server -- wires domain + adapters |
| CLI | `gyre-cli` | Terminal client + TUI |

**Hexagonal boundary invariant:** `gyre-domain` MUST NOT import `gyre-adapters` or any infrastructure crate. Enforced by `scripts/check-arch.sh` and CI.

**Other invariants enforced mechanically (pre-commit + CI):**
- Every route registered in `api/mod.rs` MUST have an ABAC `RouteResourceMapping` entry in `abac_middleware.rs` (`scripts/check-abac-route-registry.sh`) — unregistered routes get NO policy evaluation. The registry exemption file is FROZEN at its committed baseline count: adding a route there instead of the resolver converts a CI failure into a silent grandfather (`scripts/check-abac-route-registry.sh` fails on any growth). Duplicate resolver entries are likewise frozen at baseline: `resolve()` is first-match, so a later duplicate is dead code whose resource mapping silently never runs.
- Every route that runs without middleware ABAC (resolver-exempt OR listed in the frozen exemption file) MUST enforce per-handler authorization in the handler body: load the scoped entity, compare tenant/workspace against the caller, return Forbidden on mismatch. `auth.agent_id` bookkeeping and permissive-default `check_*_abac` helpers are NOT containment (`scripts/check-abac-exempt-handlers.sh`).
- Every MCP tool whose handler performs repository writes MUST be in the `needs_write` RBAC gate in `mcp.rs` — gate membership is derived from handler effects, never hand-maintained beside the dispatch (`scripts/check-mcp-write-tools.sh`).
- Every new migration MUST use the next unused 6-digit sequence number (`ls crates/gyre-adapters/migrations/ | sort | tail -3`); duplicate versions silently never run (`scripts/check-migration-versions.sh`).
- Every `MessageKind` variant MUST have an emitter — a kind that is never constructed is a dead spec delivery link (`scripts/check-dead-message-kinds.sh`).
- Never slice strings at fixed byte indexes (`&s[..N]`) — panics on non-char-boundary UTF-8 (`scripts/check-byte-slice-truncation.sh`).
- Never store or pass relative `"./..."` path literals that a child process or filesystem consumer will resolve — canonicalize at rest or at the call site (`scripts/check-relative-path-defaults.sh`); relative defaults break default deployments while absolute-tempdir tests stay green.
- Never fail-open a `resolve_ref(...)` result with `.unwrap_or_default()`/`.unwrap_or("")` — a `None` means the ref does not resolve; there is no valid empty-string SHA, and the default converts "cannot determine state" into "state is fine" on health/safety decision signals (`scripts/check-fail-open-ref-resolution.sh`).
- Every task-labeled product-surface commit MUST be recorded in its task's `commits:` frontmatter — the verifier scopes each review round to that list; an unlisted commit is invisible to review scoping (`scripts/check-task-commit-attribution.sh`, needs full git history).
- Every port-trait failure constraint (e.g. "Fails if ... already exists") MUST be enforced by EVERY adapter implementing the port — the mem adapter has no schema, so it guards in code; SQLite-only test suites cannot catch a contract one adapter enforces (`scripts/check-mem-port-contracts.sh`).
- Never fabricate a tenant/workspace scope identity via literal `"default"` fallback on lookup failure — the scope cannot be determined, so skip the operation and log (see `emit_reconciliation_completed`); a fabricated identity silently re-targets the operation and leaks data if a real scope is literally named "default" (`scripts/check-fabricated-scope-defaults.sh`).
- Never pass byte-oriented security material (secret values, keys, credentials) through `String::from_utf8_lossy` — binary secrets are silently mangled with U+FFFD and the consumer uses a corrupted credential; skip the item and log a warning naming it (`scripts/check-lossy-secret-conversion.sh`).
- Never fabricate a tenant/workspace scope identity at construction time — a struct-literal `tenant_id: "default"` (or `workspace_id`/`ws_id` equivalent) creates an entity no operator scoped; an entity whose spec-mandatory scope is missing silently rides whatever identity a downstream consumer fabricates, leaking data if a real scope is literally named "default" (`scripts/check-scope-literal-defaults.sh`; construction-time sibling of the lookup-fallback check).
- Never discard a `validate_*` result at statement position — a validator whose return value is not bound, branched on, or propagated gates nothing, exactly like the evaluate_/enforce_/verify_ classes; a void validator is audit-only theater (`scripts/check-inert-enforcement.sh`, extended by the task-099 process revision).
- Never default a path via `PathBuf::from(<bare name>)` in an `unwrap_or` fallback — a dynamic path default is a relative path default resolved against the process cwd, the same failure class as literal `"./..."` defaults but invisible to literal-pattern review (`scripts/check-relative-path-defaults.sh` Check 2).

These checks have exemption files (`scripts/*-exemptions.txt`) for pre-existing violations; never add new entries.

---

## Documentation Index

**Before writing any code, read the docs relevant to your task.** This file is the entry point only — endpoint signatures, auth requirements, env vars, and protocol details all live in the docs/ files below.

| What you need | Where to look |
|---|---|
| **All API endpoints** (REST, git HTTP, WebSocket, MCP, A2A) — auth, roles, request/response shapes | [docs/api-reference.md](docs/api-reference.md) |
| **Running the server**, env vars, OIDC, database, WireGuard config | [docs/server-config.md](docs/server-config.md) |
| **Agent spawn/complete**, container env vars, gate protocol, spec lifecycle automation | [docs/agent-protocol.md](docs/agent-protocol.md) |
| **Building, testing**, branching, commit conventions, pre-commit hooks, architecture decisions | [docs/development.md](docs/development.md) |
| **Dashboard UI** -- workspace home, repo mode tabs, keyboard shortcuts, components | [docs/ui.md](docs/ui.md) |
| **CLI usage** -- init, clone, push, tasks, MRs, diagnostics | [docs/cli.md](docs/cli.md) |
| **Product specs** (vision, architecture, milestones) | [specs/index.md](specs/index.md) |

---

## Key Architecture Specs

| Topic | Spec |
|---|---|
| Tech stack + hexagonal invariants | [specs/development/architecture.md](specs/development/architecture.md) |
| Agent Runtime (lifecycle, signal chain, compute targets, budget, prompts) | [specs/system/agent-runtime.md](specs/system/agent-runtime.md) |
| Platform model (tenant/workspace/repo hierarchy, personas, orchestration) | [specs/system/platform-model.md](specs/system/platform-model.md) |
| Vision (7 principles: judgment not generation, right context, specs as artifact...) | [specs/system/vision.md](specs/system/vision.md) |
| Realized Model (knowledge graph extracted from code) | [specs/system/realized-model.md](specs/system/realized-model.md) |
| Meta-Spec Reconciliation (safe iteration on personas, principles, standards) | [specs/system/meta-spec-reconciliation.md](specs/system/meta-spec-reconciliation.md) |
| UI Navigation (workspace home, repo mode, no-sidebar model) | [specs/system/ui-navigation.md](specs/system/ui-navigation.md) |
| Agent Gates & Spec Binding | [specs/system/agent-gates.md](specs/system/agent-gates.md) |
| Spec Lifecycle Automation | [specs/system/spec-lifecycle.md](specs/system/spec-lifecycle.md) |
| ABAC policy engine | [specs/system/abac-policy-engine.md](specs/system/abac-policy-engine.md) |

---

## Milestone Status

M0 through M35 and HSI are all **Done**. See [specs/index.md](specs/index.md) for the full milestone table.

Current work: **Authorization Provenance** (`authorization-provenance.md`) -- cryptographic work authorization chain. See `specs/tasks/` for decomposed tasks.
