# Review: task-140 — Seed built-in personas at tenant bootstrap

Comparison base: be4bc65aea199f72efecb5602e13e912ed232926 → HEAD 543aad8989b568de8564a8bc6079ca24f6ea37f4.
Diff: 8 files, +522/−12, all additions to domain/server/adapter + task file. No exemption
changes, no deleted tests, no gate weakening (diff does not touch `scripts/`).

## R1 Findings

No material findings. Evidence reviewed:

### Spec conformance (platform-model.md §2 Built-In Personas)

- **Four personas at tenant level**: `BUILTIN_PERSONA_DEFS`
  (crates/gyre-domain/src/workspace.rs) defines exactly `workspace-orchestrator`,
  `repo-orchestrator`, `accountability`, `security` in spec-table order;
  `builtin_personas(tenant_id, now)` stamps `PersonaScope::Tenant(tenant_id)` on each.
  Domain test `builtin_persona_defs_cover_spec_table` asserts the exact slug list/order.
- **Pre-approved (ships with Gyre)**: every built-in is constructed with
  `approval_status: Approved`, `approved_by: Some("system")`, `approved_at: Some(now)`
  — same system-identity convention as the pre-existing `seed_builtin_meta_specs`.
  Asserted by `builtin_personas_are_pre_approved_tenant_scoped` and by the
  full-router test `create_tenant_seeds_builtin_personas` (checks `"Approved"` in the
  HTTP response body).
- **Prompt content**: embedded from the canonical `specs/personas/<slug>.md` via
  `include_str!` — compile-time failure if a file is missing, so the "file can be
  read/embedded" acceptance criterion is enforced by the build itself, not just a test.
  All four files exist with substantive content (94+ lines each).
  `specs/personas/repo-orchestrator.md` exists (created earlier by task-099's c903a80;
  the coverage row 13 "repo-orchestrator missing" note was already stale — the file
  is present and embedded now).
- **No overwrite / idempotency**: `seed_builtin_personas_for_tenant` (server lib.rs)
  checks `find_by_slug_and_scope(slug, Tenant(tenant_id))` before insert; existing
  personas are skipped, never updated. Covered by:
  - `seed_builtin_personas_creates_four_and_is_idempotent` (second run → still 4),
  - `seed_builtin_personas_preserves_existing` (custom security persona survives with
    its own id, prompt, and `Pending` status),
  - SQLite adapter test `slug_scope_lookup_supports_idempotent_reseed` — the
    production-storage contract (scope JSON written by `create` must match the JSON
    `find_by_slug_and_scope` filters on) is proven against real rows, which an
    in-memory-only test cannot catch.
- **Content hash SHA-256**: uses the pre-existing `Persona::refresh_content_hash()`
  (`SHA-256(system_prompt + capabilities.join(","))` — row 9's verified entity
  contract). Domain test independently recomputes the digest and compares. Consistent
  with every other persona in the system rather than introducing a second hash
  convention — correct choice.

### Wiring through real entry points

- Server startup: `main.rs:55` calls `seed_builtin_personas(&state)` after
  migrations/tenant bootstrap and **before** `TcpListener::bind` (main.rs:92), so
  startup seeding has no concurrency window with request handling.
- Tenant creation: `POST /api/v1/tenants` (tenants.rs:80) seeds immediately —
  verified end-to-end through the real router by `create_tenant_seeds_builtin_personas`
  (oneshot HTTP POST + persona list by scope).
- Admin demo seed (admin.rs:447) also seeds. These are the only three tenant-creation
  sites in gyre-server (grep: `Tenant::new` + `tenants.create` — test code excluded);
  the `gyre bootstrap` CLI path goes through `POST /api/v1/tenants`, so it is covered.
  JWT auto-provisioning does not create tenants.
- SQLite adapter symmetric scope serialization confirmed for both SQLite and Postgres
  adapters (`serde_json::to_string(&p.scope)` on write, same on lookup filter), and
  the `store!` macro wires `state.personas` to the DB backend when
  `GYRE_DATABASE_URL` is set — seeding is durable in production deployments.

### Cross-task interaction (task-099 CLI bootstrap)

The CLI bootstrap also registers the four personas (create + approve via API) with
prompts embedded from `crates/gyre-cli/src/bootstrap/personas/*.md`. Verified
byte-identical to `specs/personas/*.md` (`cmp` on all four). With server-side
seeding at tenant-create, the CLI's `find_persona_by_slug` finds the seeded persona
and skips — no duplicates, no content divergence between the two registration paths.

### Test quality

All 8 new tests assert observable behavior through real entry points (domain
constructor, seed functions against `mem::test_state`, real SQLite rows, and the
actual axum router). No mirrored-logic or assertionless tests. A missing seed, a
duplicate seed, an overwrite of a customized persona, or a scope-serialization
asymmetry each fail at least one test.

### Verification run (this review)

- `cargo test -p gyre-domain --lib builtin` → 2 passed.
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona_tests` → 2 passed.
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed;
  `create_tenant_seeds` → 1 passed.
- `cargo build -p gyre-server --bin gyre-server` → compiles (startup wiring).
- `scripts/check-arch.sh` → passes (domain additions are static data + pure fn;
  `include_str!` is compile-time, no I/O).
- `check-task-commit-attribution.sh`, `check-mem-port-contracts.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-byte-slice-truncation.sh`, `check-dead-message-kinds.sh`,
  `check-in-memory-state-stores.sh`, `check-forged-scope-fields.sh`,
  `check-inert-enforcement.sh` → all pass.
- Commit attribution: all 3 product commits (f16e969, fcb712c, 151cf7d) are recorded
  in the task frontmatter and fall inside the review range; the remaining range
  commits are process-only (task-file edits, verified per-commit).

### Notes (non-blocking)

- N1: The `personas` table has no UNIQUE(slug, scope) constraint; idempotency is
  check-then-insert in code. Same approach as `seed_builtin_meta_specs`. The only
  theoretical race (admin-seed endpoint concurrent with tenant-create) is
  Admin-gated; startup seeding completes before the listener binds.
- N2: A tenant admin who hard-deletes a built-in persona gets it re-seeded on next
  server restart. Defensible reading of "ships with Gyre" (built-ins are platform
  furniture, not user data); documented behavior of the seed loop.

## Verdict

R1: no findings — task meets platform-model.md §2 Built-In Personas. Progress set to
complete.

---

## R4 Review (independent, assigned candidate 2992e44c8e5b1cb5886e1ed32687a54ee4111bb9, base fefde2946f3a0af1f212588f54591516fee0f7b2)

Full base→candidate diff reviewed (22 files, +750/−580). No material findings.

### Diff scope audit

- Persona seeding: domain `BUILTIN_PERSONA_DEFS` + `builtin_personas()`
  (workspace.rs), server `seed_builtin_personas[_for_tenant]` (lib.rs:1572/1588),
  startup wiring (main.rs:62, after `bootstrap_default_tenant` and
  `seed_builtin_meta_specs`), tenant-create hook (tenants.rs:80), admin demo-seed
  hook (admin.rs:561), SQLite adapter round-trip tests, CLI bootstrap
  consolidation to the domain defs (re-export; the deleted private
  `bootstrap/personas/*.md` copies verified byte-identical to `specs/personas/*.md`
  at base, so the consolidation is prompt-content-neutral).
- Out-of-task but documented repairs, each verified benign: rate-limit clock
  injection (production `RateLimiter::new` still uses the real wall clock;
  `try_acquire` semantics unchanged; only tests inject clocks) and the e2e.yml
  runner pin to ubuntu-24.04 (workflow-only; the locked playwright-core@1.58.2
  registry — installed via npm ci in this sandbox — has ubuntu24.04 entries and
  zero ubuntu26 entries, matching the recorded CI failure 42555036473).
- No scripts/, exemptions, web/dist, or unrelated-task changes in the diff.

### Spec conformance (platform-model.md §2)

- Exactly the four spec-table personas, Tenant scope, `Approved` +
  approved_by "system" + approved_at, version 1, prompts embedded via
  `include_str!` from `specs/personas/<slug>.md` (missing file = compile
  failure). `repo-orchestrator.md` present.
- content_hash via the pre-existing `Persona::refresh_content_hash()`
  (SHA-256 of system_prompt + capabilities) — the entity's established contract
  (coverage row 9), consistent with `update_persona`; correct choice over a
  prompt-only hash convention.
- Idempotency/no-overwrite: check-before-insert keyed on
  `find_by_slug_and_scope(slug, Tenant(tenant_id))`; proven against real SQLite
  rows (`slug_scope_lookup_supports_idempotent_reseed`), which the mem-only
  tests cannot catch.
- All tenant-creation sites seed: tenants.rs:76→80, admin.rs:529→561, and the
  startup default-tenant bootstrap is covered by the startup
  enumerate-all-tenants seeding. No route additions, so no ABAC registry impact.

### Probes run (this sandbox, /tmp/stage/bin/cargo)

- `cargo test -p gyre-domain --lib builtin` → 2 passed.
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed;
  `create_tenant_seeds` → 1 passed (real-router POST /tenants + persona list,
  asserts "Approved" in HTTP JSON); `rate_limit` → 18 passed.
- `cargo test -p gyre-adapters --lib persona` → 2 passed (real SQLite rows).
- `cargo test -p gyre-cli --bin gyre bootstrap` → 15 passed (incl. the
  pointer-equality single-source-of-truth test).
- `rustfmt --check` on all six changed Rust files → clean.
- Gates: check-arch, check-task-commit-attribution, check-mem-port-contracts,
  check-in-memory-state-stores, check-scope-literal-defaults,
  check-fabricated-scope-defaults → all pass.
- Evidence logs under /tmp/stage/review-evidence/ (this attempt's sandbox).

Test-strength check: every new test asserts observable post-conditions through
real entry points (domain constructor, seed fns, real SQLite rows, the axum
router). Removing the seed call, the approval stamp, the no-overwrite check, or
the scope stamp each fails at least one test — no self-confirming tests found.

### Non-material notes

- The coverage row 13 line references drifted slightly after the main rebase
  (main.rs:55 vs actual 62, admin.rs:507 vs actual 561); substantive claims all
  verified against the candidate tree.
- The task file's Shipped narrative names commits (151cf7d2, fcb712c5, …) that
  are dangling after checkpoint recovery — the product surface actually landed
  via b0f24a748, which is the frontmatter's recorded commit, and the
  attribution gate passes. Narrative-only drift, no review-scoping impact.

## R4 Verdict

Approved — no findings. All acceptance criteria met with real implementations.
