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

## R2 Review (this round)

Comparison base: `1a00087c5bd7dcd8703c7506086a1c5f089f3b13` → candidate
`5369d1e0f6c22cbd801e992205c3399e44f5708f` (merge of base into the task branch).
Diff: 19 files — domain defs + seeding (gyre-domain/workspace.rs, gyre-server/lib.rs,
main.rs, api/tenants.rs, api/admin.rs), SQLite persona contract tests, CLI
single-source-of-truth consolidation (e27cc0da: CLI's four private prompt copies
deleted, `BUILTIN_PERSONAS` re-exported from `gyre_domain::BUILTIN_PERSONA_DEFS`),
task file (allowed sections only: progress, commits, Shipped), coverage row 13 →
implemented. No changes to scripts/, exemptions, or unrelated tasks.

### Findings

None. All acceptance criteria independently verified with live evidence.

### Live startup probe (new — supersedes the R1-era "no TCP listener" restriction)

`/tmp/stage/capabilities.json` in this round's sandbox reports
`tcp_listener_probe: supported`, so the real startup path was exercised end to end
(evidence: `/tmp/stage/review-evidence/live-startup-probe.log`,
script `live-probe.sh`):

1. Booted the real `gyre-server` debug binary (built at candidate `5369d1e0`)
   against a fresh SQLite DB (`GYRE_DATABASE_URL=sqlite://<tmp>/gyre.db`).
2. `POST /api/v1/tenants` (Bearer gyre-dev-token) → HTTP 201; tenant-create hook
   seeded exactly the four personas at `scope=tenant&scope_id=<new tenant>`:
   `accountability`, `repo-orchestrator`, `security`, `workspace-orchestrator`,
   each `approval_status=Approved`, `approved_by=system`, `version=1`,
   64-hex-char content hash.
3. Killed the server, restarted against the same DB (exercises the
   `main.rs:53` startup seeding over an existing tenant) → still exactly 4
   personas, same slugs — restart idempotency against real durable storage.
4. Independently recomputed each persona's `content_hash` in Python from
   `specs/personas/<slug>.md` + capabilities join → all four match the API-served
   hashes (SHA-256, byte-identical prompts to the canonical spec files).

Note: the task's Shipped section still claims TCP listeners are unsupported
(errno 95) — that was true of an earlier sandbox, not this one; the live probe
above supersedes it. Documentation staleness only, no code defect.

### Focused tests (re-run at candidate `5369d1e0`, all exit 0)

- `cargo test -p gyre-domain --lib builtin` → 2 passed.
- `cargo test -p gyre-server --lib seed_builtin_personas` → 3 passed
  (creates-four + idempotent, preserves customized existing, tenant-scoped).
- `cargo test -p gyre-server --lib create_tenant_seeds` → 1 passed (real router).
- `cargo test -p gyre-adapters --lib sqlite::workspace::persona_tests` → 2 passed
  (SQLite round-trip; slug+scope lookup contract for idempotent reseed).
- `cargo test -p gyre-cli --bin gyre bootstrap` → 15 passed (incl. pointer-equality
  of `BUILTIN_PERSONAS` with `gyre_domain::BUILTIN_PERSONA_DEFS`).
- Gates: `check-arch.sh`, `check-task-commit-attribution.sh`,
  `check-mem-port-contracts.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh`, `check-in-memory-state-stores.sh` —
  all exit 0.

### Diff-quality notes

- All four attributed commits (e27cc0da, 151cf7d2, fcb712c5, f16e969c) are in the
  candidate's history and in the task frontmatter (attribution gate passes).
- The single-source-of-truth repair (e27cc0da) is real: the CLI's
  `bootstrap/personas/*.md` private copies are deleted in the diff;
  `BUILTIN_PERSONAS` is a `pub use` re-export, so drift is now impossible.
- No test inflation: each new test kills a real failure mode (missing seed,
  duplicate seed, overwrite of customized persona, SQLite scope-JSON asymmetry,
  CLI/server definition drift).
- web/dist regenerated as a side effect of building the server binary for the
  probe was restored; working tree byte-identical to the candidate at review end.

### Verdict

R2: no findings — task meets platform-model.md §2 Built-In Personas and its
acceptance criteria with live, durable-storage evidence.
