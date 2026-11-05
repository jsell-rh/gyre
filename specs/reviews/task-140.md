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

## R2 — Independent review (candidate acbdab6e, base d81b0fe4)

Range includes the two durable-finding repairs: 646684f4 (frozen-clock fix for
1aa47b6a) and 1319006b (E0382 fix for cdd23225). Full diff reviewed: domain defs,
server seeding + wiring, SQLite adapter tests, CLI single-sourcing, rate-limit
clock injection, coverage/task-file updates. No exemption files touched, no
scripts/ changes, no deleted tests.

### Findings

None material.

### Independent evidence (this sandbox, tree acbdab6e)

- `cargo test -p gyre-server --lib rate_limit` → **18 passed**, including
  `rate_limit::tests::refills_to_full_rate_on_new_second` and
  `middleware::tests::rate_limit_exhaustion_returns_429` — the gyre-server lib
  test target **compiles** at the candidate (durable finding cdd23225's E0382 is
  fixed via `Arc<AtomicU64>` shared tick) and both prior failures now pass
  deterministically under the frozen clock (durable finding 1aa47b6a).
  (r2-server-rate-limit.log)
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed;
  `create_tenant_seeds` → 1 passed. (r2-server-seed.log, r2-server-tenant-create.log)
- `cargo test -p gyre-domain --lib builtin` → 2 passed.
  `cargo test -p gyre-adapters --lib sqlite::workspace::persona` → 2 passed.
  `cargo test -p gyre-cli --bin gyre bootstrap` → 15 passed.
- Mechanical gates: check-arch.sh, check-task-commit-attribution.sh,
  check-mem-port-contracts.sh → all exit 0. All 7 frontmatter commits are in the
d81b0fe4..acbdab6e range.
- **Mutation probe** (r2-mutation-probe.md): replacing the tenants.rs production
  hook `seed_builtin_personas_for_tenant(...)` with a no-op makes
  `create_tenant_seeds_builtin_personas` fail (`left: []` vs the four expected
  slugs) — the router-level test kills the bug class, so it is not
  self-confirming. File restored byte-identical; tree clean at acbdab6e.

### Notes (non-blocking, concur with R1)

- Idempotency is check-then-insert in code (no UNIQUE(slug, scope) constraint) —
  same pattern as `seed_builtin_meta_specs`; startup seeding finishes before
  `TcpListener::bind`, and the tenant-create hook cannot race itself.
- The domain hash test recomputes SHA-256 over prompt+capabilities directly with
  `Sha256::digest` (not by calling `hash_content`), so a wrong digest algorithm
  or a constant hash fails it; the SQLite round-trip test additionally proves
  the hash persists through real rows.
- Live HTTP startup path untestable here (no TCP listeners in sandbox); verified
  via main.rs:53 wiring (before listener bind) + router-level tests, consistent
  with R1. Belongs to host verification/CI.

## Verdict

R1: no findings — task meets platform-model.md §2 Built-In Personas. Progress set to
complete.

R2: no findings — candidate acbdab6e independently re-verified, including both
durable-finding repairs; approved.
