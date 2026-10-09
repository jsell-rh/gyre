---
title: "Seed built-in personas at tenant bootstrap"
spec_ref: "platform-model.md §2 Built-In Personas"
depends_on: []
progress: not-started
coverage_sections:
  - "platform-model.md §Built-In Personas"
commits: []
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

## Agent Instructions

- Read `crates/gyre-domain/src/workspace.rs` for the `Persona` struct and `PersonaScope` enum
- Read `crates/gyre-server/src/api/policy.rs` for the `builtin_policies()` pattern — follow the same seeding approach
- Read `crates/gyre-ports/src/persona.rs` for the `PersonaRepository` port trait
- Read existing persona spec files in `specs/personas/` for prompt content
- The hexagonal boundary invariant applies: domain logic in `gyre-domain`, infrastructure in `gyre-server`
