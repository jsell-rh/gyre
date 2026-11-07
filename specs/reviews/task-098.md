# Review: task-098 — Platform Model Secrets Admin API + MCP + CLI

**Candidate:** `d44fee6d7b21b95ceb46339975f02d86ab703643` (branch `pipeline/task-098/40cb043621934c40b9c30ab4729dce38-26`, base `9bef21a4`)
**Reviewer:** independent review stage
**Verdict:** **Approved** — no findings. Evidence under `/tmp/stage/review-evidence/`.

## What was reviewed

Full diff `9bef21a4..d44fee6d` (36 files, +3692/−103): `api/secrets.rs` (new, 886 ln),
`abac_middleware.rs`, `mcp.rs` (secrets:// resources), `secret_rotation.rs` (new),
`jobs.rs`/`main.rs`/`lib.rs`/`mem.rs`, ports/adapters `find_by_id`, CLI
(`main.rs`, `client.rs`), UI (`SecretsPanel.svelte` + 3 settings surfaces + tests),
docs, coverage matrix, committed `web/dist`.

## Independent evidence (all on the exact candidate tree)

| Probe | Result |
|---|---|
| `cargo test -p gyre-server --lib -- mcp secret` | **102 passed / 0 failed** (incl. all 6 `api::secrets` tests, `abac_middleware::tests::secrets_routes_are_admin_only`, 3 `secret_rotation` tests, 4 mcp secrets-resource tests, 4 mem secret-contract tests) — `cargo-server-mcp-secret.log` |
| `cargo test -p gyre-adapters secret` | **17/17** — `cargo-adapters-secret.log` |
| `cargo test -p gyre-cli --bin gyre secret` | **6/6** — `cargo-cli-secret.log` |
| `npm test` (full, after `npm ci`) | **64 files, 1635 passed / 41 skipped, exit 0** — `vitest-full.log`; focused settings suites 152/152 — `vitest-settings-suites.log` |
| Mechanical gates (19 scripts) | all exit 0 except `check-unnamed-tuple-carriers` — 3 hits, **identical set on base** (verified via temporary worktree at `9bef21a4`: same 3 pre-existing carriers, no new ones; check not CI-wired) — `mechanical-gates.log` |
| Mutation probe 1: `builtin-secret-admin-only` policy `enabled: false` | `secrets_routes_are_admin_only` **FAILS** (Developer gets 200 on GET /api/v1/secrets) — `mutation-disable-admin-enforcement.log` |
| Mutation probe 2: in-handler `require_admin` short-circuited | `non_admin_jwt_is_forbidden` **FAILS** (developer JWT gets 201 on POST) — same log |
| Post-mutation restore | `git checkout` restored both files; `git status` clean at `d44fee6d`; full focused suite re-run green (102/102) |

Both enforcement layers (ABAC policy + in-handler role check) are therefore backed
by tests that fail when the enforcement is disabled — not audit-only theater.

## Findings on the contract

- **Admin API (§7 table):** all four endpoints live, registered in `api/mod.rs`,
  ABAC `RouteResourceMapping` for each, admin-only at both policy and handler
  layers (proven by mutation). Create is create-or-update on
  `(scope, scope_id, name)` per the spec's "Create/update". `tenant_id` always
  derived from auth context; caller-supplied `scope_id` validated against the
  caller's tenant for all four scopes (cross-tenant → 403, unknown → 404; tested).
- **Values never returned:** structural guarantee — `gyre_common::Secret` has no
  value field; `SecretResponse` is metadata-only; values live encrypted
  (AES-256-GCM, task-097) and are only re-supplied from the caller's request on
  rewrite. API tests assert the plaintext never appears in create/list/rotate
  responses and round-trip the stored value through `get_value`; the UI test
  simulates a value-leaking server and asserts the panel still renders only
  `••••••••`.
- **MCP resources:** `secrets://names` / `secrets://status` registered in
  `resources/list`, scope resolved from agent JWT claims with agent-record/task
  fallback, tenant from the governing workspace record (no fabricated
  "default"). Names-only cascade mirrors `resolve_for_agent` scoping without any
  decryption path. Tests cover cascade + nearest-scope-wins, cross-workspace
  invisibility, expiry statuses (active/expiring/expired), no-agent-scope error.
- **CLI:** `gyre secret set/list/delete/rotate` with workspace/repo/tenant scope
  resolution (git-remote inference reuse), name→id resolution for delete/rotate;
  6 parse tests including unknown-scope and missing-value rejection.
- **UI:** SecretsPanel tab in Workspace/Repo/Tenant settings with create/delete/
  rotate actions, write-only value field; real tests on all three surfaces
  (scope wiring asserted via `api.secretsList` call args).
- **Rotation job:** `secret_rotation_check` registered in the job registry
  (300 s interval) and spawned in `main.rs` via the same `spawn_job` path as
  `retention_cleanup`; `run_once` tests cover expiring+expired notification,
  admin/owner-only targeting, dedup on rerun, healthy-secret silence.
- **Coverage matrix:** rows 45/46 honestly moved to "Landed on branch …
  ready-for-review" — not marked verified; prior Hollow notes preserved.
- **Mechanical invariants:** exemptions file held at 16 entries (7 line-number
  re-points only); no new tuple carriers, scope-literal defaults, forged scope
  fields, or inert enforcement introduced.

## Non-blocking observations (recorded, not defects)

1. The task's Shipped prose says CLI flags `--type`/`--expires-in`; the code
   (and `docs/cli.md`, which matches the code) implements `--secret-type`/
   `--expires-at`. Prose-only inaccuracy in the task notes; spec imposes no
   flag names.
2. `secret_rotation::run_once` scans tenant/workspace/repo scopes but not
   task-scoped secrets. Consistent with the spec's "Task secrets (one-time,
   per-task)" lifecycle (revoked at teardown, nothing for an admin to rotate)
   and the task plan's notification-job definition; noted because the Shipped
   prose says "scans all scopes".

Full workspace `cargo test --all`, all-target Clippy, and CI are left to the
verification stage per assignment scope.
