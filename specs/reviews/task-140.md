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

# R2 Review — candidate 39598b53 (base 110180fb)

Independent re-review at the exact assigned candidate. Static diff review plus focused
probes run at this tree (evidence: /tmp/stage/review-evidence/, logs + notes.md).

## Verified

- Persona seeding implementation unchanged in substance from R1 and still correct:
  `BUILTIN_PERSONA_DEFS` (four spec-table personas, prompts `include_str!`-embedded
  from `specs/personas/*.md`), `builtin_personas()` stamps Tenant scope / Approved /
  system approver / SHA-256 content hash; `seed_builtin_personas[_for_tenant]`
  check-before-insert via `find_by_slug_and_scope` (never overwrites); wired at
  main.rs:55, tenants.rs:80, admin.rs:517; CLI bootstrap single-sourced from the
  domain defs (private prompt copies deleted, pointer-equality test).
- Probes at 39598b53: `gyre-domain --lib builtin` 2 passed; `gyre-adapters --lib
  sqlite::workspace::persona` 2 passed; `gyre-cli --bin gyre bootstrap` 15 passed;
  check-arch.sh / check-task-commit-attribution.sh / check-mem-port-contracts.sh
  all exit 0. Task file matches the assigned contract; no scripts/, exemptions,
  or web/dist changes in the candidate diff.

## Finding R2-F1 (blocking) — test target does not compile

Commit 646684f4, the verification repair for durable finding 1aa47b6a (flaky
`rate_limit_exhaustion_returns_429`), adds
`rate_limit::tests::refills_to_full_rate_on_new_second`, which does not compile:

```rust
let tick = AtomicU64::new(1_700_000_000);            // rate_limit.rs:118
let rl = RateLimiter::with_clock(1, {
    Box::new(move || tick.load(Ordering::Relaxed))    // :120 — tick moved into closure
});
...
tick.store(1_700_000_001, Ordering::Relaxed);         // :127 — error[E0382]
```

`cargo test -p gyre-server --lib <any filter>` exits 101 at this candidate
(/tmp/stage/review-evidence/test-server-seed.log). Consequences:

- The whole gyre-server lib suite — including this task's own
  `seed_builtin_personas_*` and `create_tenant_seeds_builtin_personas` tests —
  cannot run at the candidate.
- CI `cargo test --all` fails → the task's "Tests pass" acceptance criterion is
  not met at the assigned candidate.
- The repair was shipped without being compiled: every green run cited in the
  shipped notes predates 646684f4 (latest cited trees 5f5ab82a / 6fd54cc0).

Fix: share the clock state between closure and test body (e.g.
`Arc<AtomicU64>` with a clone captured by the closure), then re-run
`cargo test -p gyre-server --lib rate_limit seed_builtin_personas create_tenant_seeds`
and the workspace suite at the repaired tree.

## Verdict

R2: NOT approved — one blocking code finding (R2-F1). The persona seeding work
itself is sound and its independently compilable suites pass at this candidate;
approval is withheld solely because the candidate's gyre-server test target does
not compile, so the task's own server-side acceptance tests cannot execute.
