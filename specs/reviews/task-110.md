# Review: task-110 — Implement tenant & workspace invitation flow

**Candidate:** `7dbf8908f4b69b5014ab5a956f4f120db2341123`
**Base:** `f764bc005c5ff8f442f58556a655fb9902049aa9`
**Round:** rebase-repair review (prior F1: candidate sat on merge-base
33cd786c with Diesel migration version collisions against main)

**Verdict: PASS (approved).** Evidence: `/tmp/stage/review-evidence/task-110-review.md`
(this round's full log — probe commands, exits, outputs).

## What this round examined

The prior round verified the task-110 implementation clean on its own
lineage but rejected integration (F1): wrong merge-base plus duplicate
Diesel migration versions (`20261010000058` vs main's
`budget_enforcement_action`, `20261108000061` vs main's `spec_links`).
This round reviewed the repair and the merged tree.

## Repair verification (prior F1 closed)

- **Merge onto the assigned base:** merge commit `46f49834` brings
  9a68f84a onto f764bc00. The base→candidate diff is additive — nothing
  of main's content is deleted (deletions limited to the auth.rs
  `find_or_create_user` → `find_or_create_user_tenant` refactor,
  sqlite/mod.rs migration-partition test update, and an
  exemption-file comment rewrite).
- **Migration renumbering:** task migrations now
  `2026-11-13-000063_invitations`, `-000064_invitation_pending_unique`,
  `-000065_user_tenant_scope`; `000064` sorts after `000063` (the tables
  it indexes). `ls migrations | sed | sort | uniq -d` → empty;
  `check-migration-versions.sh` exit 0; `check-migration-sql-portability.sh`
  exit 0. The adapter test targeting the renumbered version
  (`migration_000064_dedupes_raced_pending_invitations_before_index`)
  passes against real SQLite.
- **Six conflict files** (postgres/mod.rs, schema.rs, gyre-domain/lib.rs,
  api/mod.rs, docs/api-reference.md, specs/coverage/system/user-management.md)
  resolved as unions without duplicates or lost content — all 19
  relevant mechanical invariant scripts exit 0 on the merged tree,
  including `check-abac-route-registry.sh` (route↔mapping parity) and
  `check-arch.sh`.

## Independent behavior verification

- `cargo test -p gyre-domain invitation` → 4 passed.
- `cargo test -p gyre-adapters invitation` → 5 passed (real SQLite:
  roundtrips, wrong-source transition rejection, DB-enforced
  pending-scope unique index, renumbered-migration dedupe).
- `cargo check -p gyre-server --lib --tests` → exit 0 (compiles all 26
  `api::invitations` tests). gyre-server test execution exceeds the
  sandbox's 4 GiB budget — **verifier must run `cargo test --all`**.
- Source review of the full diff (+7419/−33): spec-exact domain
  entities; port contracts with atomic single-use `transition_status`;
  SQLite conditional UPDATE + partial-index race backstop; mem guards
  under lock; TenantAdmin-only guards; CSPRNG tokens (ring
  SystemRandom, 32 bytes) stored only as SHA-256; token routes pre-auth
  with token-as-auth-factor, invitee-email guard on presented bearer
  identities; expired/revoked/reused → 409; cross-tenant containment on
  tenant and workspace surfaces; per-tenant `InvitationPolicy`
  (fail-closed on corrupt rows); SSO account linking only with
  `email_verified=true` via sanctioned `claim_external_id`; background
  expiry job spawned at startup with configurable interval (zero falls
  back to default); no token material in list responses.

`check-crypto-verify.sh` exits 2 with an awk syntax error — verified
failing identically on the base commit (pre-existing script/environment
issue, not this diff).

## Non-blocking observations (cleanup, not defects)

1. Stale comments in `crates/gyre-adapters/src/sqlite/invitation.rs`
   (lines 187, 414, 794) still say "migration 000061" — post-renumber
   it is `2026-11-13-000064`. Comment-only.
2. `specs/tasks/task-110.md` Shipped text says merge commit `46f49834`
   is "recorded in `commits:` above", but the executor's attribution
   tool (`dev-attribution.py`, structurally `--no-merges`) removed it
   in candidate commit `7dbf8908`. The mechanically enforced invariant
   (`check-task-commit-attribution.sh`, also `--no-merges`) passes
   against the candidate tree; the merge resolution was examined in
   this review via the base→candidate diff.
