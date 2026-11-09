# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation
Flow, §Invitation Expiry, §Tenant Invitations.
Candidate: `6f9c799f` (base `fb1f82e3`); task commits `8f66d5a4`, `0f4fea9a`,
`8c469c53`, `c1de316b`, `797cf52f` plus process/merge commits. Round 10 of
review (rounds 1–9 findings all repaired and re-verified this round).
Verdict: **needs-revision** (one docs finding; product code passes).

Evidence log: `/tmp/stage/review-evidence/task-110-round10.md`.

## What was independently verified (all PASS)

- **Acceptance criteria 1–9** verified by code reading plus live probes:
  domain entities (`gyre-domain/src/invitation.rs`, all spec fields,
  `InvitationPolicy::default()` = 7/7/50/true), port traits with the
  single-use `transition_status(from, to)` contract, SQLite/Postgres
  adapters + migrations (`2026-10-10-000058_invitations`,
  `2026-11-08-000059_invitation_pending_unique`,
  `2026-11-08-000060_user_tenant_scope`), all five tenant endpoints, the
  full workspace invitation lifecycle (invite → Pending → accept/decline/
  expire/revoke on both the token surface and the authenticated invitee
  surface), CSPRNG tokens stored only as SHA-256 hashes (known-answer test),
  bulk invite, expiry background job wired in `main.rs` with the
  zero-interval env guard, routes registered with ABAC resolver mappings
  and per-handler enforcement.
- **Test suites** (focused + the two crates holding all changes, full):
  `api::invitations` 26/0, `auth` 69/0, adapters `invitation` 5/0,
  adapters `user` 24/0, full `gyre-server --lib` **1564/0**, full
  `gyre-adapters` **400/0 (+12 ignored)** — matching the implementer's
  round-9 claims exactly.
- **Mutation probes (3/3 killed)** — the round-9 repair tests are
  load-bearing, not self-confirming:
  1. TenantAdmin guard disabled → `non_admin_cannot_invite_via_http` FAILS.
  2. Tenant containment + Owner/Admin guard removed from
     `invite_to_workspace` → `cross_tenant_guards_reject_foreign_callers`
     and `non_member_cannot_invite_to_workspace` FAIL.
  3. SSO-link branch disabled →
     `accept_links_existing_sso_user_instead_of_parallel_account` FAILS.
  All mutations restored; working tree verified pristine afterwards.
- **Round-9 repairs F1–F6** all confirmed present in code (SSO account
  linking via tenant-scoped `find_by_email` + membership activation;
  `sanitize_username`/`resolve_unique_username` for derived handles;
  explicit-username validation before the single-use claim → 400/409;
  `GYRE_INVITE_EXPIRY_INTERVAL_SECS=0` fallback; `load_policy` fails
  closed; two-tenant HTTP-layer containment tests, cap 429, workspace
  expiry).
- **Security structure**: magic-link token routes live outside the
  require_auth/ABAC middleware stack by design (the 256-bit token is the
  auth factor) with per-handler invitee-match guards when a Bearer
  identity is presented; `check-abac-exempt-handlers` passes. mem adapters
  enforce the pending-scope port contract and check-and-set transitions
  under the store lock. Single-use acceptance is a conditional UPDATE /
  check-and-set in every adapter, with best-effort rollback to Pending on
  side-effect failure.
- **Mechanical guards**: 20/22 pass. The 2 failures
  (`check-non-atomic-creation` → `git_http.rs:1555`;
  `check-dependent-scope-resolution` → `explorer_views.rs:736`,
  `reconciliation.rs:925`) are byte-identical pre-existing findings in
  files this diff does not touch (verified empty diff for all three files
  base→candidate) — left for upstream-side fixes per the round-8 note.

## Findings

- [open] **F1 (docs/process): the coverage matrix was never updated for the
  four sections this task declares.** `specs/coverage/system/user-management.md`
  rows 5 (Tenant-Level User Onboarding), 6 (Workspace Invitation Flow),
  7 (Invitation Expiry), and 25 (Tenant Invitations) — exactly the task's
  `coverage_sections:` frontmatter — remain `task-assigned` with empty notes,
  and the header still counts "3/36 implemented, 23 task-assigned". No commit
  on this branch ever touched the file (`git log 6f9c799f -- <file>` shows
  only base-side history). This deviates from the repo's established
  ready-for-review convention: task-120 marked its rows `implemented` with
  evidence notes "pending independent review"; task-111 did the same in
  commit `efd37804d` ("Follows the task-120 precedent for ready-for-review
  rows"); task-128's repair round explicitly included a "coverage honesty"
  coverage-file update; task-121/123 touched their rows on their feature
  branches. Consequence: the matrix under-reports the shipped state, and the
  pipeline's definition of done (spec-fidelity auditor confirming no
  `implemented` section is hollow; coverage counts) can never engage on
  these sections. **Repair:** a docs commit marking rows 5/6/7/25
  `implemented` (status + Task column) with concise evidence notes
  ("pending independent review"), updating the header counts accordingly,
  following the task-111 precedent. No product-code change required.

## Non-findings (checked, judged acceptable)

- Global username uniqueness collision on invitation accept — resolved by
  the round-9 deterministic suffix scheme; regression test kills the
  mutation class.
- `resolve_workspace_invitee` lists all users and filters by tenant in the
  handler — O(n) over users per workspace invite; acceptable at current
  scale, consistent with other list-then-filter handlers in the codebase.
- Notification on workspace invite is best-effort (`let _ =`) — the invitee's
  durable surface is `GET /api/v1/users/me/invitations`; documented in code.
- Bulk invite is partial-success (per-entry errors, no batch rollback) —
  matches the spec's CSV-style onboarding rationale, documented.
- The magic-link accept routes burn the invitation on expiry (persist
  `Expired` immediately) — spec-aligned (audit states, never deleted).

## Summary

The product implementation is real, complete against the task contract, and
well-guarded: every round-9 repair is load-bearing (proven by mutation), all
focused and full suites for the touched crates pass, and the only mechanical
guard failures are pre-existing in untouched files. The single open item is
the missing coverage-matrix update for the task's four declared sections —
a one-commit docs repair following the task-111/task-120 precedent. Revision
required for that bookkeeping before approval.

— Reviewer, round 10 (candidate `6f9c799f`)
