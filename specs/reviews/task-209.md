# Review — task-209 (Finish HSI §12 profile: notification filtering, judgment ledger, auth provider info)

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is" (row 53).
Candidate under review: `59a04b5172031aae1567cfe74a51c12acb986e19` (branch
`pipeline/task-209/73ab19e5344d4f13b68ebb8bbca7196e-1`), base `f7f5c59f`.
All main refs merged along the branch (f764bc00, cd58a166, 4ec1e526, ccfd4bc5)
are ancestors of the base, so the base→candidate diff is purely task-209 work.
Verdict: **needs-revision** — one blocking finding (F1: gyre-adapters test
target does not compile); everything else probed clean.

## Probes run

- `/tmp/stage/bin/cargo test -p gyre-adapters --lib` → **compile failure**,
  5× `error[E0061]: this function takes 10 arguments but 9 arguments were
  supplied` (evidence: `adapters-user-profile-test.log`). This is F1 below.
- `/tmp/stage/bin/cargo check -p gyre-server --lib` → clean (5m 48s,
  `server-check-lib.log`).
- `/tmp/stage/bin/cargo test -p gyre-domain` → 8 passed (`domain-test.log`).
- `cd web && npm test` → **2312 passed, 41 skipped, 0 failed** across 113
  files (`web-test.log`).
- `bash scripts/check-migration-versions.sh` → OK (the new
  `2026-10-07-000056_user_auth_provider_info` maps to a unique Diesel version
  `20261007000056`).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-notification-toggle-coverage.sh` → OK (41 variants,
  41 toggles, labels in lockstep).
- `bash scripts/check-tenant-filter.sh` → awk syntax error; **pre-exists on
  the base** (`git show f7f5c59f:scripts/check-tenant-filter.sh` fails
  identically) — environment awk incompatibility, not a task-209 defect.
- Diff inspection of all 56 non-`web/dist` files in base→candidate.

## Verified working (no findings)

- **Part 1 (spec amendment)** — HSI §12 now names the four concrete
  aggregation sources with attribution keys and workspace-attribution joins,
  documents the `?workspace_id=` filter semantics and the
  `approval|rejection|gate|trust|meta-spec` wire strings, and tightens the
  notification-preference contract (canonical names, 400 on unknown,
  badge/MCP parity, background-job exemption). Matches the implementation.
- **Part 3 (write paths)** — real `AuditEvent` writes with failure
  propagation (`?`), not fire-and-forget: trust transition in the
  `PUT /workspaces/:id` handler (only on an actual transition, after
  `apply_trust_transition` succeeds — tested incl. the no-change negative);
  gate override in `submit_review` (one event per FAILED gate result, only
  for human `user_id` approvals — agent-token negative tested, ledger
  surfacing via `?type=gate` tested); meta-spec publish on registry
  create/update (workspace attribution from `scope`/`scope_id`, Global/Tenant
  scopes deliberately unattributed — reasonable and documented).
- **Part 2 (aggregation)** — `list_for_user` unions spec_approvals (LEFT
  JOIN spec_ledger_entries; `?workspace_id=` now a real SQL filter) with
  audit-carried judgments filtered by `user_id` + `event_type IN (...)`,
  reads through the shared decompressing `AuditEventRow` materialization
  (compressed-detail test included), sorts reverse-chronologically with a
  deterministic tie-break, applies limit/offset after the merge, filters
  type per-source. Postgres and mem adapters in parity (mem shares the same
  underlying stores via `with_sources`). Rejection attribution to the
  declining human is implemented and tested end-to-end
  (`judgment_ledger_rejection_attributed_to_withdrawer`, plus the
  specs.rs `*_surfaces_in_judgment_ledger` writer/reader key-match tests).
- **Notification filtering** — `exclude_types` applied in-query before
  pagination in SQLite/Postgres/mem/SSE-decorator; inbox + badge + MCP inbox
  load real preferences; background dedup surfaces pass `&[]` per the port
  doc; PUT normalizes legacy parse aliases to canonical names (inert-toggle
  test included).
- **Auth provider info** — `users.oidc_issuer` migration (unique version,
  portable `ALTER TABLE`), `record_login` single-statement debounce with the
  issuer-change carve-out, JWT validation writer, read-only in
  `GET /users/me` (no PUT field), real-JWT test.
- **Frontend** — `NOTIF_TYPE_IDS` canonical list replaces the previous
  fabricated type names; prefs load/save use the real server shape; badge
  uses the server count; lockstep enforced by the new CI lint. Web suite
  green.

## Findings

### F1 (blocking, code): gyre-adapters test target does not compile — 5 stale-arity `list_for_user` call sites

The merge commit `0f41df8b` combined task-209's `exclude_types` parameter
with main/task-125's `repo_id` parameter, taking
`NotificationRepository::list_for_user` to 10 arguments. All production call
sites were updated, but five test-module call sites were not:

- `crates/gyre-adapters/src/sqlite/notification.rs:861`, `:878`, `:898` —
  pre-existing repo-filter tests, still on the base's 9-arg signature;
- `crates/gyre-adapters/src/sqlite/user_profile.rs:996`, `:1006` — task-209's
  own new `notifications_exclude_types_applies_to_list_and_count` test,
  written against the branch's pre-merge 9-arg signature (no `repo_id`) and
  never re-arithmetized after the merge.

Reproduce: `/tmp/stage/bin/cargo test -p gyre-adapters --lib` →
`error: could not compile gyre-adapters (lib test) due to 5 previous errors`.
Consequently the acceptance criterion "`cargo test --all` passes" fails, and
the task's own notification-filtering adapter test never executes — the task
file's "Verification" claim that the adapter suite is green post-remerge is
not reproducible. Fix: pass `None` (repo_id) / `&[]` (exclude_types) at the
five sites to match the 10-arg signature.

## Checks deferred to the verifier

- `cargo test --all` (after F1 is fixed) — server test compilation was
  deferred per the sandbox budget; `cargo check -p gyre-server --lib` is
  clean, but the server test modules (e.g. `notifications.rs` test helpers,
  `users.rs`/`workspaces.rs`/`merge_requests.rs`/`meta_specs.rs`/`specs.rs`
  test mods) were not compiled here and must be exercised remotely.
- Web bundle rebuild and generated-asset checks (owned by the verifier).
