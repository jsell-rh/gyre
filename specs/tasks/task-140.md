---
title: "Seed built-in personas at tenant bootstrap"
spec_ref: "platform-model.md §2 Built-In Personas"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Built-In Personas"
commits: ["e27cc0dab572a10d96ead8578f72bfa1dd71409d", "151cf7d2fef662f87f0306ba70b8f3197fb41802", "fcb712c51c1f27b597e4a0cb798e63d7f341f6e9", "f16e969ce52622e8f6133deb60550d77dd7ed47e"]
---

## Spec Excerpt

From `platform-model.md` §2 Built-In Personas:

Gyre ships with built-in personas that exist at the tenant level:

| Persona | Purpose | Approval |
|---|---|---|
| `workspace-orchestrator` | Cross-repo coordination, priority management, escalation | Pre-approved (ships with Gyre) |
| `repo-orchestrator` | Ralph loop management, task decomposition, agent dispatch | Pre-approved |
| `accountability` | Spec integrity, drift detection | Pre-approved |
| `security` | Vulnerability scanning, OWASP review | Pre-approved |

Built-in personas can be overridden at workspace or repo scope (with human approval).

## Implementation Plan

1. **Read existing persona spec files** — `specs/personas/workspace-orchestrator.md`, `specs/personas/accountability.md`, `specs/personas/security.md` exist in the repo. `specs/personas/repo-orchestrator.md` is missing per the coverage audit. Create it with appropriate content for Ralph loop management.

2. **Create a `seed_builtin_personas()` function** in `gyre-server` (alongside the existing `builtin_policies()` seeding in `policy.rs`):
   - Called at server startup, after the default tenant is created
   - For each of the 4 built-in personas, check if a persona with that name and `PersonaScope::Tenant` already exists
   - If not, create the Persona entity with:
     - `name`: the persona name (e.g., `"workspace-orchestrator"`)
     - `scope`: `PersonaScope::Tenant` (or the equivalent `Global` variant used in code)
     - `prompt`: content read from the corresponding `specs/personas/*.md` file (embedded at compile time or read from disk)
     - `version`: 1
     - `content_hash`: SHA-256 of the prompt content
     - `approval_status`: `Approved` (pre-approved)
     - `approved_by`: system user ID
     - `approved_at`: current timestamp
   - If the persona already exists, do not overwrite (user may have customized it)

3. **Wire into server startup** — call `seed_builtin_personas()` in the server initialization path, after database migrations and tenant bootstrap.

4. **Tests:**
   - Unit test: `seed_builtin_personas` creates 4 personas with correct names, scope, and pre-approved status
   - Unit test: calling `seed_builtin_personas` twice is idempotent (no duplicates)
   - Unit test: verify all 4 persona spec files can be read/embedded

## Acceptance Criteria

- [ ] Server startup creates 4 built-in personas at the tenant level: `workspace-orchestrator`, `repo-orchestrator`, `accountability`, `security`
- [ ] All built-in personas have `approval_status: Approved` (pre-approved)
- [ ] `specs/personas/repo-orchestrator.md` file exists with appropriate persona definition
- [ ] Seeding is idempotent — restarting the server does not duplicate personas
- [ ] Content hash is computed from the prompt content via SHA-256
- [ ] Tests pass

## Shipped

Implemented in `151cf7d2` (domain defs + adapters + server seeding), `fcb712c5` (tenant-create hook), `f16e969c` (startup wiring), with the dual-source-of-truth defect repaired in `e27cc0da` (CLI bootstrap personas re-exported from `gyre_domain::BUILTIN_PERSONA_DEFS`; the CLI's four private prompt copies deleted — there is now exactly one definition of the built-in personas, embedded from `specs/personas/*.md`, shared by server seeding and `gyre bootstrap`).

**Where things live:**
- Domain: `BUILTIN_PERSONA_DEFS` + `builtin_personas(tenant_id, now)` in `crates/gyre-domain/src/workspace.rs` — four personas in spec-table order, prompts embedded via `include_str!("../../../specs/personas/<slug>.md")`, `content_hash` computed via `Persona::refresh_content_hash()` (SHA-256), `approval_status: Approved`, approved by the system user.
- Server: `seed_builtin_personas` / `seed_builtin_personas_for_tenant` in `crates/gyre-server/src/lib.rs` — check-before-insert via `find_by_slug_and_scope` so existing/customized personas are never overwritten; idempotent across restarts.
- Call sites: server startup (`crates/gyre-server/src/main.rs:55`, after migrations, before the listener bind), tenant creation (`crates/gyre-server/src/api/tenants.rs:80`, so every new tenant gets its own scoped copies), and the admin demo-seed path (caller-scoped).
- CLI: `gyre bootstrap` registers the same four personas by re-exporting the domain defs (`crates/gyre-cli/src/bootstrap.rs`).
- `specs/personas/repo-orchestrator.md` exists (created by `fcb712c5`; required by the spec table).

**Verification (focused probes, all green at `e27cc0da`):**
- `cargo test -p gyre-domain --lib builtin` — 2 passed (spec-table coverage, pre-approved tenant scope)
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona` — 2 passed (round-trip through SQLite, slug+scope lookup for idempotent reseed)
- `cargo test -p gyre-server --lib seed_builtin_personas` — 3 passed (creates four + idempotent, scoped to own tenant, preserves existing)
- `cargo test -p gyre-server --lib create_tenant_seeds` — 1 passed (tenant-create hook seeds)
- `cargo test -p gyre-cli --bin gyre bootstrap` — 15 passed (incl. `builtin_personas_are_the_domain_seed_definitions`: pointer-equality with `gyre_domain::BUILTIN_PERSONA_DEFS`, making the single source of truth mechanical)

**Sandbox restriction:** TCP listeners are unsupported in this sandbox (errno 95), so the live startup path could not be exercised over HTTP; it is verified via the `main.rs:55` wiring plus the router-level tests above. Live HTTP verification belongs to host verification/CI.

## Agent Instructions

- Read `crates/gyre-domain/src/workspace.rs` for the `Persona` struct and `PersonaScope` enum
- Read `crates/gyre-server/src/api/policy.rs` for the `builtin_policies()` pattern — follow the same seeding approach
- Read `crates/gyre-ports/src/persona.rs` for the `PersonaRepository` port trait
- Read existing persona spec files in `specs/personas/` for prompt content
- The hexagonal boundary invariant applies: domain logic in `gyre-domain`, infrastructure in `gyre-server`
