---
title: "Seed built-in personas at tenant bootstrap"
spec_ref: "platform-model.md §2 Built-In Personas"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Built-In Personas"
commits: ["151cf7d2fef662f87f0306ba70b8f3197fb41802", "fcb712c51c1f27b597e4a0cb798e63d7f341f6e9", "f16e969ce52622e8f6133deb60550d77dd7ed47e"]
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

- [x] Server startup creates 4 built-in personas at the tenant level: `workspace-orchestrator`, `repo-orchestrator`, `accountability`, `security`
- [x] All built-in personas have `approval_status: Approved` (pre-approved)
- [x] `specs/personas/repo-orchestrator.md` file exists with appropriate persona definition
- [x] Seeding is idempotent — restarting the server does not duplicate personas
- [x] Content hash is computed from the prompt content via SHA-256
- [x] Tests pass

## Shipped

Real seeding of the four built-in personas (platform-model.md §2), shipped across
three product commits (f16e969, fcb712c, 151cf7d — all ancestors of this branch
head) and re-verified at the current head in this session:

- **Domain** (`crates/gyre-domain/src/workspace.rs`): `BUILTIN_PERSONA_DEFS` — the
  four personas in spec-table order, prompts embedded byte-for-byte from
  `specs/personas/<slug>.md` via `include_str!` (a missing file is a compile
  error, so embeddability is enforced by the build). `builtin_personas(tenant_id,
  now)` returns tenant-scoped, pre-approved `Persona` entities: `Approved`,
  `approved_by: Some("system")`, `approved_at: Some(now)`, content hash via the
  existing `Persona::refresh_content_hash()` (SHA-256 over system_prompt +
  capabilities).
- **Server** (`crates/gyre-server/src/lib.rs`): `seed_builtin_personas(state)`
  runs at startup after migrations and tenant bootstrap (main.rs:55, before the
  listener binds); `seed_builtin_personas_for_tenant` is the shared idempotent
  tail, also called from `POST /api/v1/tenants` (tenants.rs:80) and the admin
  demo seed (admin.rs:507) so every tenant gets the §2 table at creation.
  Idempotency key is `find_by_slug_and_scope(slug, Tenant(tenant_id))` —
  existing personas are never overwritten (user customizations survive
  restarts); a deleted built-in is re-seeded on restart ("ships with Gyre").
- **Tests** (all re-run green at this head this session):
  - `cargo test -p gyre-domain --lib builtin` → 2 passed (defs cover spec table
    exactly; pre-approved tenant-scoped; hash independently recomputed).
  - `cargo test -p gyre-adapters --lib sqlite::workspace::persona` → 2 passed
    (SQLite round-trip of scope/approval/hash/prompt through real rows;
    check-then-insert reseed does not duplicate).
  - `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed (four +
    idempotent, preserves customized persona, tenant isolation).
  - `cargo test -p gyre-server --lib create_tenant_seeds` → 1 passed (real
    router: `POST /api/v1/tenants` seeds the four, response contains "Approved").

Environment note: crates.io network access was intermittent this session; all
above tests compiled and ran from the local cargo cache (domain/adapter/server
built via `SKIP_WEB_BUILD=1` at this head). Full workspace suites, arch checks,
all-target Clippy, and GitHub CI are owned by verification/publication.

## Agent Instructions

- Read `crates/gyre-domain/src/workspace.rs` for the `Persona` struct and `PersonaScope` enum
- Read `crates/gyre-server/src/api/policy.rs` for the `builtin_policies()` pattern — follow the same seeding approach
- Read `crates/gyre-ports/src/persona.rs` for the `PersonaRepository` port trait
- Read existing persona spec files in `specs/personas/` for prompt content
- The hexagonal boundary invariant applies: domain logic in `gyre-domain`, infrastructure in `gyre-server`
