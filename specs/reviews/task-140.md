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

## R2 — Rebase round (independent review)

Comparison base: c5deca801a331e049f35b181f42d898df24e7e2b → candidate
f2299bfbc2af645b4c121be1860ea50fa3a7e700 (branch HEAD; includes merge 0a843e984
resolving main's task-105/rustfmt advance, conflict in `crates/gyre-cli/src/bootstrap.rs`).

Diff scope: 21 files — domain defs, SQLite adapter tests, CLI consolidation (deletes the
private `bootstrap/personas/*.md` embeds, re-exports `gyre_domain::BUILTIN_PERSONA_DEFS`),
server seeding + wiring (startup/tenant-create/admin-seed), rate-limit clock injection
(prior-round flake repair), coverage updates, task file. No `scripts/` changes, no
exemption growth, no deleted tests.

### Findings

None.

### Evidence (this sandbox, /tmp/stage/review-evidence/)

- Focused suites all green: `gyre-domain --lib builtin` 2 ok;
  `gyre-server --lib seed_builtin_personas` 3 ok; `create_tenant_seeds` 1 ok;
  `rate_limit` 18 ok (incl. deterministic `refills_to_full_rate_on_new_second`);
  `gyre-adapters --lib persona` 2 ok (SQLite round-trip + idempotent reseed);
  `gyre-cli --bin gyre bootstrap` 15 ok (incl. pointer-equality single-source test).
- Mechanical gates: `check-arch.sh` passes; `check-task-commit-attribution.sh` passes;
  `rustfmt --check` clean on all task-changed files; clippy 0 errors on changed crates.
- **Live startup probe** (`live-startup-probe.md`): built and ran the real binary against
  a fresh SQLite DB — startup seeded exactly 4 personas at `{"kind":"Tenant","id":"default"}`
  scope, all `Approved`/`approved_by=system`/`version=1`. Restart against the same DB:
  still 4 rows, same IDs, same `approved_at` (not overwritten). All four `content_hash`
  values independently recomputed with Python hashlib SHA-256 over
  `system_prompt + capabilities.join(",")` — all match.
- Merge resolution verified: `bootstrap.rs` keeps the candidate-side single-source
  re-export; main's conflicting hunk was rustfmt-only on the now-deleted private struct;
  compiles, tests pass, rustfmt clean.
- Startup ordering confirmed in `main.rs`: `bootstrap_default_tenant` →
  `seed_builtin_policies` (fail-closed) → `seed_builtin_meta_specs` →
  `seed_builtin_personas`, all before `TcpListener::bind` (main.rs:115).

### Notes (non-blocking)

- `r2d2: database is locked` ERROR lines appear during first-boot migrations (pre-existing
  SQLite pool contention, before persona code runs); startup completes normally.
- Seeding failures at tenant-create time are logged (warn), not transactional — a failed
  seed self-heals on next restart via idempotent startup seeding. Matches the task plan's
  check-then-insert shape and the `seed_builtin_meta_specs` convention.

## Verdict

R2: no findings — candidate approved against platform-model.md §2 Built-In Personas
and the task-140 acceptance criteria.
