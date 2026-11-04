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

## R2 Review (independent, candidate eb0e6eb8 vs base 75ada1c4)

Assignment: candidate `eb0e6eb84222178399f00099c845749fb33bec0d`, base
`75ada1c4073ee9d56d3a1aebf5c780582d8867cb`. All five task-attributed commits
(`5f5ab82a`, `e27cc0da`, `151cf7d2`, `fcb712c5`, `f16e969c`) present on the branch.
Diff: 9 Rust files + Cargo.toml/lock + specs; no `scripts/`, no exemption files,
no `web/` changes (dist identical to base).

### Independent verification (fresh probes in this sandbox)

- `cargo test -p gyre-domain --lib builtin` — 2 passed (spec-table order,
  pre-approved tenant scope, independently recomputed SHA-256 hashes).
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona` — 2 passed
  (round-trip through real SQLite rows; slug+scope JSON lookup contract that
  idempotent reseed depends on, proven against real rows).
- `cargo test -p gyre-server --lib seed_builtin_personas` — 3 passed
  (creates four + idempotent restart, preserves a customized existing persona,
  scoped to own tenant).
- `cargo test -p gyre-server --lib create_tenant_seeds` — 1 passed (tenant-create
  hook through the real router, asserts the 4 slugs and `"Approved"` over HTTP).
- `cargo test -p gyre-cli --bin gyre bootstrap` — 15 passed (incl. pointer-equality
  of `BUILTIN_PERSONAS` with `gyre_domain::BUILTIN_PERSONA_DEFS` — single source
  of truth is mechanical).
- Mutation probe: temporarily flipping `builtin_personas` to `Pending` made
  `builtin_personas_are_pre_approved_tenant_scoped` FAIL (left: Pending,
  right: Approved) — the pre-approval assertion is live, not self-confirming.
  File restored; tree byte-clean at candidate afterwards.
- Gates: `check-arch.sh`, `check-task-commit-attribution.sh`,
  `check-scope-literal-defaults.sh`, `check-in-memory-state-stores.sh`,
  `check-rustfmt-diff.py 75ada1c4` (9 files, changed lines clean) — all exit 0.
- Evidence: `/tmp/stage/review-evidence/` (tree-sha.txt, test-*.log,
  mutation-domain.log, gate-*.log, clippy-domain.log).

### Spec conformance (platform-model.md §2)

- Exactly the four spec-table personas at `PersonaScope::Tenant`, in table order;
  `Approved` + `approved_by: "system"` + `approved_at` (pre-approved).
- Prompts embedded via `include_str!` from `specs/personas/<slug>.md` —
  compile-time failure if missing; `repo-orchestrator.md` present with
  Ralph-loop content matching the spec table's purpose column.
- Idempotent: check-before-insert keyed on `find_by_slug_and_scope(slug,
  Tenant(id))`; existing/customized personas never overwritten (§2 override
  path preserved). Every tenant-creation path is covered: startup enumeration
  (main.rs), `POST /api/v1/tenants` hook, admin demo-seed, CLI bootstrap.
- `content_hash` = SHA-256 of prompt+capabilities via `refresh_content_hash()`,
  independently recomputed in the domain test.

### Observations (not findings)

- Sandbox wrapper rejects workspace-wide cargo invocations, so
  `check-clippy-diff.py 75ada1c4` could not be run here (deferred to
  verification). Focused `cargo clippy -p gyre-domain --lib` shows zero
  diagnostics in the task's changed file (`workspace.rs`); the digit-grouping
  repair (`1_700_000_000`) is in place.
- That focused clippy run surfaces a pre-existing `never_loop` **error** at
  `crates/gyre-domain/src/rust_extractor.rs:1071` (introduced by `d7940e85`,
  present identically in base `75ada1c4`, untouched by task-140). Under this
  sandbox's clippy 0.1.99 (2026-09-28) the workspace-wide diff-gate would fail
  on it for base-identical reasons — environmental/pre-existing, not a candidate
  defect. Verifier should confirm with `python3 scripts/check-clippy-diff.py
  75ada1c4` on the CI toolchain.
- TCP listeners are unsupported in this sandbox (capabilities.json), so the live
  HTTP startup path was not exercised; it is covered by the main.rs wiring plus
  the router-level tests above.

### Verdict

Approved — no findings. The task contract is satisfied by real production code
with meaningful tests; all acceptance criteria independently re-verified.
