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

## R2 — Independent re-review (retry after review-model infrastructure failure)

Assignment: base `73a31e0b` → candidate `cbf89cb0` (merged tree `6fd54cc0` plus a
process-only commit touching only `specs/tasks/task-140.md`). Working tree clean at
`cbf89cb0` for every probe; all mutations restored (`git status --porcelain` empty).

**Probes run (evidence: `/tmp/stage/review-evidence/probe-log.txt`):**

- `cargo test -p gyre-domain --lib builtin` → 2 passed. Spec-table order/slugs, tenant
  scope, `Approved`/`approved_by=system`/`approved_at`, version 1, and an independently
  recomputed SHA-256 over prompt+capabilities.
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona` → 2 passed. Real
  SQLite rows: round-trip of scope/approval/hash/prompt, and the check-then-insert
  reseed loop finds existing rows (no restart duplication) — the production-storage
  contract the mem adapter cannot prove.
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed (four seeded +
  idempotent, customized persona preserved, tenant isolation).
- `cargo test -p gyre-server --lib create_tenant_seeds` → 1 passed, through the real
  axum router (`POST /api/v1/tenants` then persona list by scope).
- `cargo test -p gyre-cli --bin gyre bootstrap` → 15 passed, including
  `builtin_personas_are_the_domain_seed_definitions` (pointer-equality with
  `gyre_domain::BUILTIN_PERSONA_DEFS` — the e27cc0da single-source-of-truth repair
  is mechanically enforced).
- `cargo check -p gyre-server --bins` → clean; startup wiring at `main.rs:53-56`
  (after `build_state`/migrations-era seeds, before `TcpListener::bind`).

**Mutation probes (all caught, then reverted):**

1. Removing the `find_by_slug_and_scope` existing-check (unconditional insert) →
   `seed_builtin_personas_creates_four_and_is_idempotent` and
   `seed_builtin_personas_preserves_existing` both FAIL (duplicate security persona).
2. Dropping pre-approval (`Pending`, `approved_by=None`) →
   `builtin_personas_are_pre_approved_tenant_scoped` FAILS.
3. Changing the SHA-256 input (prompt only, no capabilities) → hash assertion FAILS
   against the independently recomputed digest.

So the tests genuinely guard the acceptance criteria; none are self-confirming.

**Mechanical gates at candidate:** arch, task-commit-attribution, mem-port-contracts,
scope-literal-defaults, fabricated-scope-defaults, in-memory-state-stores,
forged-scope-fields, relative-path-defaults, byte-slice-truncation,
inert-enforcement, dead-message-kinds — all exit 0.

**Source-level cross-checks:** `build_state` wires `state.personas` to
Pg/SqliteStorage when `GYRE_DATABASE_URL` is set (lib.rs:1003) — seeding is durable in
DB deployments; scope-JSON symmetry between `create` and `find_by_slug_and_scope`
verified in both SQLite and Postgres adapters; all tenant-creation sites in the server
route through `seed_builtin_personas_for_tenant`; `specs/personas/repo-orchestrator.md`
exists (94 lines) and is embedded via `include_str!` (compile-time failure if missing).

**Sandbox restriction (not a code defect):** TCP listeners unsupported (errno 95,
`/tmp/stage/capabilities.json`), so the live HTTP startup path was verified via the
binary compile + wiring read + router-level tests. Host verification: start the server
with `GYRE_DATABASE_URL=sqlite://<tmp>`, `GET /api/v1/personas?scope=tenant&scope_id=<id>`
expecting the four slugs `Approved`; restart; expect still four (no duplicates).

**R2 verdict: approved, no findings.** Candidate `cbf89cb0` satisfies
platform-model.md §2 Built-In Personas and the task's acceptance criteria with real
implementations and failure-capable tests.
