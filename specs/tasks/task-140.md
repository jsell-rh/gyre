---
title: "Seed built-in personas at tenant bootstrap"
spec_ref: "platform-model.md §2 Built-In Personas"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Built-In Personas"
commits: ["1319006b9be7608ccf3d3b46aeeed8cd45c7c74b", "646684f41e5bba33b803ed70825971874ee4f318", "5f5ab82a4eb3906a5493bf1d51a851281fa79d8b", "e27cc0dab572a10d96ead8578f72bfa1dd71409d", "151cf7d2fef662f87f0306ba70b8f3197fb41802", "fcb712c51c1f27b597e4a0cb798e63d7f341f6e9", "f16e969ce52622e8f6133deb60550d77dd7ed47e"]
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

Built-in persona seeding implemented in `151cf7d2` (domain defs + adapters + server seeding), `fcb712c5` (tenant-create hook + `specs/personas/repo-orchestrator.md`), `f16e969c` (startup wiring), `e27cc0da` (single source of truth: CLI bootstrap re-exports `gyre_domain::BUILTIN_PERSONA_DEFS`), with verification repairs in `5f5ab82a` (rustfmt/digit-grouping on task-changed lines) and dist reverts in `63e2eeff`/`5945c0d5`.

### What ships

- Domain: `BUILTIN_PERSONA_DEFS` + `builtin_personas(tenant_id, now)` in `crates/gyre-domain/src/workspace.rs` — the four spec-table personas (`workspace-orchestrator`, `repo-orchestrator`, `accountability`, `security`), prompts embedded via `include_str!("../../../specs/personas/<slug>.md")`, `content_hash` = SHA-256 via `Persona::refresh_content_hash()`, `approval_status: Approved`, approved by the system user, `version: 1`.
- Server: `seed_builtin_personas` / `seed_builtin_personas_for_tenant` in `crates/gyre-server/src/lib.rs` — check-before-insert via `find_by_slug_and_scope`, so existing/customized personas are never overwritten and seeding is idempotent across restarts.
- Call sites: server startup (`crates/gyre-server/src/main.rs:55`, after migrations), tenant creation (`crates/gyre-server/src/api/tenants.rs`, every new tenant gets its own scoped copies), and the admin demo-seed path.
- CLI: `gyre bootstrap` registers the same four personas from the domain defs (pointer-equality test makes the single source of truth mechanical).

### Repairs on this branch

- **Finding 1aa47b6a** (`rate_limit_exhaustion_returns_429` returned 200): the 429 exhaustion test raced the wall clock across a second boundary, refilling the bucket mid-test. Fixed in `646684f4` by injecting a frozen clock (`RateLimiter::with_clock`) into `rate_limited_app`, plus a `refills_to_full_rate_on_new_second` test with a scripted clock exercising the refill deterministically.
- **Finding cdd23225** (review): that same `646684f4` checkpoint committed the refill test without compiling it — the `move` closure took ownership of the `AtomicU64` tick, so the later `tick.store(...)` was error E0382, blocking the entire gyre-server lib test target. Fixed in `1319006b`: the tick is shared via `Arc<AtomicU64>` (clone captured by the closure); the test compiles, is rustfmt-clean, and passes.
- **Finding 93ef2291** (verification gate): rustfmt/clippy failures on four task-changed lines — fixed in `5f5ab82a`, formatting only.
- **Finding 763d5ad1** (contract): a stray `## Verification Repair` ledger section in this task file was deleted; the task file matches the assigned contract (operational content lives in this Shipped section).

### Test evidence (tree `1319006b`, this sandbox)

- `cargo test -p gyre-server --lib` — **1260 passed, 0 failed** (full suite; proves the E0382 block is gone and no regression elsewhere).
- Includes: `rate_limit::tests::refills_to_full_rate_on_new_second` ok, `middleware::tests::rate_limit_exhaustion_returns_429` ok (finding 1aa47b6a), `seed_builtin_personas_*` 3 ok, `create_tenant_seeds_builtin_personas` ok.
- Prior rounds (documented in review history): `gyre-domain --lib builtin` 2 passed, `gyre-adapters --lib sqlite::workspace::persona` 2 passed, `gyre-cli --bin gyre bootstrap` 15 passed; mechanical gates (arch, attribution, mem-port-contracts, scope-literal, in-memory-state, forged-scope, relative-path, byte-slice, inert-enforcement) all exit 0.
- Evidence logs: `/tmp/stage/review-evidence/test-server-lib-full.log`, `test-server-rate-limit.log`, `test-server-seed.log`, `test-server-tenant-create.log`.

### Sandbox restrictions

TCP listeners are unsupported in this sandbox (errno 95, `/tmp/stage/capabilities.json`), so the live startup path is verified via the `main.rs:55` wiring plus the router-level tests; live HTTP verification belongs to host verification/CI. Heavy builds on this sandbox's slow disk also made the `tirith` pre-tool hook time out intermittently during this round — environmental, no code impact.

## Agent Instructions

- Read `crates/gyre-domain/src/workspace.rs` for the `Persona` struct and `PersonaScope` enum
- Read `crates/gyre-server/src/api/policy.rs` for the `builtin_policies()` pattern — follow the same seeding approach
- Read `crates/gyre-ports/src/persona.rs` for the `PersonaRepository` port trait
- Read existing persona spec files in `specs/personas/` for prompt content
- The hexagonal boundary invariant applies: domain logic in `gyre-domain`, infrastructure in `gyre-server`
