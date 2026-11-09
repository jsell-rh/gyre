---
title: "Seed built-in personas at tenant bootstrap"
spec_ref: "platform-model.md §2 Built-In Personas"
depends_on: []
progress: complete
coverage_sections:
  - "platform-model.md §Built-In Personas"
commits: ["b0f24a748880ec65b275b6f7eb2905bfb44d9695"]
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
- Call sites: server startup (`crates/gyre-server/src/main.rs:59`, after migrations and default-tenant bootstrap), tenant creation (`crates/gyre-server/src/api/tenants.rs`, every new tenant gets its own scoped copies), and the admin demo-seed path.
- CLI: `gyre bootstrap` registers the same four personas from the domain defs (pointer-equality test makes the single source of truth mechanical).

### Repairs on this branch

- **Finding 1aa47b6a** (`rate_limit_exhaustion_returns_429` returned 200): the 429 exhaustion test raced the wall clock across a second boundary, refilling the bucket mid-test. Fixed in `646684f4` by injecting a frozen clock (`RateLimiter::with_clock`) into `rate_limited_app`, plus a `refills_to_full_rate_on_new_second` test with a scripted clock exercising the refill deterministically.
- **Finding cdd23225** (review): that same `646684f4` checkpoint committed the refill test without compiling it — the `move` closure took ownership of the `AtomicU64` tick, so the later `tick.store(...)` was error E0382, blocking the entire gyre-server lib test target. Fixed in `1319006b`: the tick is shared via `Arc<AtomicU64>` (clone captured by the closure); the test compiles, is rustfmt-clean, and passes.
- **Finding 93ef2291** (verification gate): rustfmt/clippy failures on four task-changed lines — fixed in `5f5ab82a`, formatting only.
- **Finding 763d5ad1** (contract): a stray `## Verification Repair` ledger section in this task file was deleted; the task file matches the assigned contract (operational content lives in this Shipped section).
- **Finding 59dd2c91** (rebase): main advanced under this branch, conflicting in `crates/gyre-server/src/lib.rs` and `main.rs`. Resolved by merge commit `63809a09` as a clean union — main's task-156 additions (`pub mod reconciliation`, `meta_spec_conformance_sweep` job spawn, `approved_content_hash` in `seed_builtin_meta_specs`) came in while all persona seeding survived intact (verified by direct diff of both conflicted files against candidate `b0f24a74` and base `4871a825`; startup ordering `bootstrap_default_tenant → seed_builtin_policies → seed_builtin_meta_specs → seed_builtin_personas` confirmed in `main.rs`).
- **Finding dda9c1fb** (checkpoint): worker lease expired mid-round; source recovered at `b0f24a74` and re-verified on the merged tree — working tree clean, no unresolved conflicts, persona seeding present at every call site.
- **Finding 2789ae92** (rebase): main advanced to `c5deca80` (task-105 log compression + rustfmt sweeps), leaving one content conflict in `crates/gyre-cli/src/bootstrap.rs`. Resolved by merge commit `0a843e984` keeping the candidate side — `pub use gyre_domain::BUILTIN_PERSONA_DEFS as BUILTIN_PERSONAS` — because main's conflicting hunk was rustfmt-only on the pre-consolidation private `BuiltinPersona` struct (main never touched the persona prompt files or referenced the private struct outside `bootstrap.rs`), while the auto-merged `main.rs` and `bootstrap.rs` tests already use `system_prompt` from the domain def. Taking main's side would have reintroduced the second, drift-prone persona embed the task-140 consolidation removed. All other paths auto-merged; persona seeding verified present at every call site on the merged tree.
- **Finding 3bcd1437** (CI, candidate head `c369e1ff`, e2e run 42555036473): the `e2e` job failed in "Install Playwright browsers" — GitHub's `ubuntu-latest` now resolves to ubuntu 26.04, and the locked `playwright-core@1.58.2` registry has zero browser-download entries for `ubuntu26.*`, so `npx playwright install --with-deps chromium` throws before any test runs. Not a candidate defect (base→candidate diff never touched `.github/` or `web/package*.json`); verified against the locked package source in this sandbox (`ubuntu24.04-x64` has full chromium/headless-shell/firefox entries, the failing throw is `registry/index.js:1152`). Fixed by pinning the `e2e` job to `runs-on: ubuntu-24.04` in `.github/workflows/e2e.yml` (with a comment naming the lockfile constraint); `ci.yml` jobs stay on `ubuntu-latest` since they passed on 26.04. Live re-run belongs to remote verification. Evidence: `/tmp/stage/review-evidence/ci-3bcd1437.md`.

### Test evidence

- Prior round (tree `1319006b`): `cargo test -p gyre-server --lib` — **1260 passed, 0 failed**, including `rate_limit::tests::refills_to_full_rate_on_new_second`, `middleware::tests::rate_limit_exhaustion_returns_429` (finding 1aa47b6a), `seed_builtin_personas_*` 3 ok, `create_tenant_seeds_builtin_personas` ok; plus `gyre-domain --lib builtin` 2 passed, `gyre-adapters --lib sqlite::workspace::persona` 2 passed, `gyre-cli --bin gyre bootstrap` 15 passed.
- Resume round (merged tree `63809a09`): full mechanical gate sweep — 89 `scripts/check-*.sh` gates, failure set byte-identical to the base commit `4871a825` (35 pre-existing/environmental failures on both trees, 54 passes on both). The branch and merge introduce zero new gate failures.
- Rebase round (merged tree `0a843e984`, this sandbox — crate cache now sufficient, compile works): `cargo check -p gyre-cli` clean; `rustfmt --check` on the resolved `bootstrap.rs` clean; `check-arch.sh` and `check-task-commit-attribution.sh` pass; focused suites all green — `gyre-cli --bin gyre bootstrap` **15 passed**, `gyre-domain --lib builtin` **2 passed**, `gyre-server --lib seed_builtin_personas` **3 passed**, `gyre-server --lib rate_limit` **18 passed**, `gyre-server --lib create_tenant_seeds` **1 passed**, `gyre-adapters --lib persona` **2 passed**. Full-log evidence: `/tmp/stage/review-evidence/rebase-2789ae92.md`.
- CI repair round (finding 3bcd1437, this sandbox): workflow-only change, no Rust/web source touched. Probes: `npm ci` in `web/` installs the locked `playwright-core@1.58.2`; its registry contains 10 `ubuntu24.04` download-URL entries and zero `ubuntu26`/`ubuntu25` entries; edited `e2e.yml` parses as valid YAML with `jobs.e2e.runs-on == ubuntu-24.04`; `check-task-commit-attribution.sh` passes (ci commit touches no product surface). Full-log evidence: `/tmp/stage/review-evidence/ci-3bcd1437.md`.

### Sandbox restrictions

TCP listeners are unsupported in this sandbox (errno 95, `/tmp/stage/capabilities.json`), so the live startup path is verified via the `main.rs` wiring plus router-level tests; live HTTP belongs to host verification/CI. No code impact.

## Agent Instructions

- Read `crates/gyre-domain/src/workspace.rs` for the `Persona` struct and `PersonaScope` enum
- Read `crates/gyre-server/src/api/policy.rs` for the `builtin_policies()` pattern — follow the same seeding approach
- Read `crates/gyre-ports/src/persona.rs` for the `PersonaRepository` port trait
- Read existing persona spec files in `specs/personas/` for prompt content
- The hexagonal boundary invariant applies: domain logic in `gyre-domain`, infrastructure in `gyre-server`
