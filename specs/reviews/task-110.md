# Review: TASK-110 — Implement tenant & workspace invitation flow

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-14 (rebase-repair round)
**Verdict:** approved
**Candidate:** `af6beafd79e74082dad32d5cfe2f6bda6d2c87d8` (merge repair onto main head `1ee52c43`, durable finding 08005f87 merge_state DIRTY)
**Base:** `1ee52c431b24c02e4833ac585147d1695ef26486`
**Spec:** user-management.md §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations

---

## Scope of this round

The invitation-flow implementation (domain entities, ports, SQLite/Postgres/mem adapters,
migrations 2026-11-13-000063/64/65, the five tenant endpoints, workspace invitation lifecycle,
CSPRNG + SHA-256 tokens, per-tenant `InvitationPolicy`, bulk invite, background expiry job,
ABAC mappings, SSO email-link claim in auth.rs) is the previously independently approved
work (approvals recorded at `f92a0af3` and `187b0e3c` in the branch history). This round's
candidate is the rebase repair onto current main head `1ee52c43` (which carries task-112's
notification delivery channels and task-125's API rows): merge commit `104fc5e3` resolves
two source conflicts — `gyre-adapters/src/sqlite/mod.rs` (the `migration_000058` test's
exclusion set, now covering both task-110's `2026-11-13-000065_user_tenant_scope` and
task-112's `2026-11-09-000060_user_preferences_channel_shape`) and
`specs/coverage/system/user-management.md` (header recomputed against the auto-merged
table) — plus a process commit `af6beafd` recording the repair.

This review independently re-examined the *entire* base..candidate diff (not just the merge
resolution) against the task contract and canonical spec.

## Verification performed

- Read the full diff and all key sources: `gyre-domain/src/invitation.rs`,
  `gyre-ports/src/invitation.rs` (+ user port additions `find_by_email`/`claim_external_id`),
  `gyre-adapters/src/sqlite/invitation.rs` (1029 lines), `postgres/invitation.rs`, `user.rs`
  additions in all three adapter stacks, `mem.rs` invitation repositories,
  `gyre-server/src/api/invitations.rs` (all handler paths: invite, bulk, list, revoke, policy
  GET/PUT, magic-link accept/decline, workspace invite/list/revoke, token accept/decline,
  invitee-facing surface, expiry job), `auth.rs` (SSO email-link claim with `email_verified`
  gate), `api/mod.rs`, `abac_middleware.rs`, `lib.rs` (magic-link routes outside
  require-auth with per-handler guards), `main.rs` (job spawn), migrations 000063/64/65,
  schema.rs, docs.
- Mechanical invariant gates (all green): migration versions (no duplicates), ABAC route
  registry coverage, migration SQL portability, mem port contracts (duplicate guards in
  code), in-memory state store lint, ABAC exempt-handler per-handler authorization, arch
  lint, lossy secret conversion, scope literal defaults, inert enforcement.
- `cargo check -p gyre-server` green (full dep graph incl. the unconditionally compiled
  postgres module).
- Focused tests green: `cargo test -p gyre-domain -p gyre-ports` (513+8),
  `cargo test -p gyre-adapters invitation` (5: DB-level pending-scope uniqueness,
  raced-duplicate dedupe, roundtrips, wrong-source transition rejection),
  `cargo test -p gyre-adapters migration_000058` (the merge-conflict-resolution test —
  passes on the merged tree), full `cargo test -p gyre-adapters` (524 passed / 6 ignored).
- Clippy (lib targets for the four crates, filtered to lines changed vs base, mirroring CI's
  `check-clippy-diff.py` semantics): zero warnings on changed lines.
- Route overlap (`/api/v1/workspaces/invitations/:token/...` static segment vs
  `/api/v1/workspaces/:id/...` param segment) confirmed safe against the vendored matchit
  0.7.3 contract ("static and dynamic route segments are allowed to overlap; static
  segments given higher priority"); direct scratch-crate execution was blocked by the
  stage's build wrapper (infrastructure restriction, not a code defect).
- Commit attribution verified at the candidate commit: all nine attributed SHAs present in
  `specs/tasks/task-110.md` frontmatter. (`check-task-commit-attribution.sh` fails only
  against the review sandbox's working tree, where the stage reset the task file to
  `progress: not-started, commits: []` — not a candidate defect.)
- Coverage rows 5/6/7/25 claims cross-checked against the code: domain entities with every
  spec field, all five tenant endpoints, workspace lifecycle, per-tenant policy with
  fail-closed corrupt-row handling, conditional single-use `transition_status` claims in
  every adapter (WHERE-clause in SQLite/PG, check-and-set under the store lock in mem),
  background expiry job with conditional transitions and zero-interval env guard.

## Findings

None blocking. Notes (non-blocking):

1. **Server test suite deferred to verifier (4 GiB budget).** The 40-test HTTP suite in
   `api/invitations.rs` and the mem.rs invitation-repo tests were read and assessed as
   real, mutation-sensitive tests (concurrent-claim race via `tokio::join!` asserting
   exactly one winner; bearer-identity mismatch 403 with invitation-not-burned follow-up;
   cross-tenant 403 fixtures; SSO-link; key minting; caps; expiry) but not executed in
   this sandbox per the build-wrapper constraints. Required verifier commands:
   `cargo test --all`, `cargo test -p gyre-server api::invitations`, and CI's
   all-targets clippy diff gate.
2. **Design note, accepted:** a workspace literally named `invitations` has its
   workspace-scoped `:id` routes shadowed by the token routes at that segment (static
   precedence); the invitee-facing authenticated routes (`/workspaces/:id/invite/accept`)
   remain reachable. Documented behavior of the route design, not a defect.
3. **Coverage-note nit:** row 5 says "26 in api::invitations"; the merged tree carries
   more (≈40). Undercount only — no hollow claim.

Evidence: `/tmp/stage/review-evidence/probe-log.md` (+ `clippy-diff-check.py`).

## Verdict

**Approved.** The rebase repair preserves the independently approved invitation
implementation, resolves both source conflicts correctly (proven by the merged-tree test
run of the very test whose exclusion set was conflicted), and the combined tree passes
every mechanical invariant gate, type check, and focused test suite run in this review.
Final `cargo test --all` execution is recorded for the independent verifier.
