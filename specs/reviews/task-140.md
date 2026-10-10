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
## R2 (independent re-review — retry of infrastructure-failed review)

Comparison base: 8c2d177505852b3e39cd77f4f782fb355de245aa → HEAD 65e32bd29b6226ddc7a1403b174798f940b04452.
Diff over the assigned base: 23 files, +748/−651. Product surface is the same code R1
approved plus `e27cc0da` (CLI bootstrap personas single-sourced from
`gyre_domain::BUILTIN_PERSONA_DEFS`; the CLI's four private prompt copies under
`crates/gyre-cli/src/bootstrap/personas/` deleted). Task contract text (Spec Excerpt,
Implementation Plan, Acceptance Criteria) is byte-identical to base — only progress
metadata and the Shipped section were added, resolving durable finding 1beb150a
("implementation changed the assigned requirements"): no requirement was changed.

### R2 verification (all probes re-run at this head; evidence in
/tmp/stage/review-evidence/task-140/evidence.txt)

- `cargo test -p gyre-domain --lib builtin` → 2 passed.
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona` → 2 passed.
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed.
- `cargo test -p gyre-server --lib create_tenant_seeds` → 1 passed.
- `cargo test -p gyre-cli --bin gyre bootstrap` → 15 passed.
- **Mutation probe**: replacing the `seed_builtin_personas_for_tenant` call at
  tenants.rs:80 with a comment makes `create_tenant_seeds_builtin_personas` FAIL —
  the acceptance tests are not self-confirming. File restored (sha256-verified, tree
  clean at 65e32bd2).
- Invariant gates: check-arch, check-mem-port-contracts, check-in-memory-state-stores,
  check-migration-versions, check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-byte-slice-truncation, check-inert-enforcement, check-lossy-secret-conversion →
  all exit 0.

### Attribution

All four product commits (e27cc0da, 151cf7d2, fcb712c5, f16e969c) are in the task
frontmatter and inside the review range. The remaining range commits are process-only,
pipeline web/dist build artifacts, or merge commits of upstream fixes. The
check-task-commit-attribution.sh failure on task-210's a781ede2 is pre-existing:
that commit is an ancestor of the assigned base 8c2d1775, not introduced by this
candidate. Not a finding against this task.

### R2 notes (non-blocking)

- N3: `specs/personas/repo-orchestrator.md` pre-existed the base (added by task-099's
  c903a80) — coverage row 13's "repo-orchestrator missing" was already stale. The
  acceptance criterion is the file existing with an appropriate definition, which
  holds. The Shipped section's "created by fcb712c5" attribution is inaccurate but
  non-substantive (fcb712c5 only renamed the adapters test module).
- N4: sandbox cannot accept TCP connections (errno 95, capabilities.json), so the live
  startup path was not exercised over HTTP. The exact host-verification checks are
  recorded in the evidence file: boot with a SQLite GYRE_DATABASE_URL, GET
  /api/v1/personas expecting the four built-ins Approved at Tenant scope, restart,
  re-GET expecting the same four (no duplicates).

## Verdict

R2: approved — no findings. The durable R1-followup contract finding is resolved; the
e27cc0da single-source-of-truth repair closes the CLI/server prompt-drift risk with a
pointer-equality test making it mechanical.
