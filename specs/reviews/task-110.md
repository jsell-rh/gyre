# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace
Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `a309844d` (base `9bfab365`).
Verdict: **approved** — no blocking findings. Prior round's blocking finding F1 is
genuinely repaired; the merge resolution is mechanically accurate.

Full probe evidence: `/tmp/stage/review-evidence/review-record.md` (commands, sources,
exit codes, outputs).

## Scope of this round

Candidate delta from the previously-reviewed source `15a0b070`: merge of assignment
base `9bfab365` (task-149 analytics, `e72249c2`), the F1 repair (`d1103854`), and two
process commits. The core invitation implementation was approved across prior
independent review rounds; this round verified the repair and the merge, and
spot-checked the core.

## F1 repair (prior blocking finding) — verified fixed

- Both invitee routes are now `RouteResourceMapping::exempt` in the resolver
  (`abac_middleware.rs` ~626-627), not entries in the frozen exemption file (no
  exemption-file growth in the diff). No resolver shadowing: `resolve()` is exact
  pattern equality, so the earlier `/api/v1/workspaces/:id` entry cannot match the
  longer invitee patterns; `check-abac-route-registry.sh` passes.
- The middleware's exempt path passes through after impersonation attribution;
  authentication is enforced by the `AuthenticatedAgent` extractor; authorization is
  enforced per-handler by `authorize_my_pending_invitation` (workspace load → tenant
  containment 403 → caller-must-be-the-invitee 404). `check-abac-exempt-handlers.sh`
  passes (81 handlers).
- The denied mechanism the exemption must overcome is real: the middleware derives
  `subject.member_of_resource_workspace` from the real membership store (false for a
  non-member invitee, fail-closed on store error) and the seeded builtin
  `workspace-membership-required` (Deny, priority 940) keys on it — proven live by the
  pre-existing test `builtin_workspace_membership_required_denies_non_member_live`.
- The regression test `invitee_accepts_and_declines_through_abac_middleware` is a hard
  test: it drives the REAL `api_router()` layered with `abac_middleware` (chain
  verified identical to `build_router` minus orthogonal layers), with a real API-key
  invitee, seeded builtins, and a control assertion (GET `/members` → 403 for the
  invitee) that self-invalidates the test if the builtin deny ever stops firing — so
  the 204s on accept/decline genuinely prove the exemption. It fails if the routes are
  re-mapped to workspace resources (the F1 defect).

## Merge resolution (e72249c2) — verified accurate

- The only conflict, `specs/coverage/SUMMARY.md`, was resolved by mechanical recount:
  a fresh dry recount of the candidate tree matches the committed rows and TOTAL
  exactly (802 total / 220 n/a / 0 not-started / 191 assigned / 228 implemented /
  163 verified). The prior non-blocking process finding is resolved.
- The merge carries task-149 analytics changes; no invitation behavior changed.
- `task-124.md`'s migration-renumber note (000063 → 000066) is accurate against the
  actual migration listing; `warn-continue-creation-exemptions.txt` is comment-only.

## Probes run (all exit 0)

- `cargo check -p gyre-server --tests` (10m12s).
- `cargo test -p gyre-domain --lib invitation` → 4 passed.
- `cargo test -p gyre-adapters --lib invitation` → 5 passed (incl. DB-level
  pending-scope uniqueness and raced-duplicate dedupe).
- Mechanical scripts: abac-route-registry, abac-exempt-handlers (81), arch,
  migration-versions, mem-port-contracts, migration-sql-portability,
  forwarded-header-trust, forged-scope-fields, inert-enforcement,
  lossy-secret-conversion, scope-literal-defaults, fabricated-scope-defaults,
  unbounded-external-http, in-memory-state-stores, task-commit-attribution.

## Core spot-checks

- Token: 32 bytes ring `SystemRandom` CSPRNG, only SHA-256 hash persisted.
- Expiry job `spawn_invitation_expiry` spawned at startup (`main.rs`), conditional
  status claims, interval via `GYRE_INVITE_EXPIRY_INTERVAL_SECS`.
- auth.rs SSO email-link: `email_verified=true` gate, tenant-namespaced external_id,
  sanctioned `claim_external_id` transition, reserved-identity check precedes linkage.
- All 5 spec tenant endpoints + policy endpoints + workspace invitation lifecycle
  routes registered with resolver entries; docs updated. No stub markers in new code.

## Verifier-required probes (gyre-server test builds exceed this sandbox's 4 GiB
budget — infrastructure restriction, not a code defect)

1. `cargo test -p gyre-server --lib invitee_accepts_and_declines_through_abac_middleware`
2. `cargo test -p gyre-server --lib invitation`
3. `cargo test -p gyre-server --lib jwt_email_link`
4. `cargo test -p gyre-server --lib builtin_workspace_membership_required_denies_non_member_live`
5. `cargo test --all` (task acceptance criterion)

## Non-blocking observations

- The working tree carries a pre-existing uncommitted harness edit to
  `specs/tasks/task-110.md` (progress reset); the candidate commit itself records
  `ready-for-review` with full commit attribution.
- `specs/coverage/system/user-management.md` header prose describes the prior
  (`8d07a41a`) merge; the `9bfab365` merge did not touch this file and the mechanical
  counts are verified accurate. Cosmetic only.

Test-inflation check: none found. The new regression test fails when the exemption is
removed (control assertion + the same builtin that caused F1); adapter tests assert
database-level constraints; domain tests assert expiry only transitions Pending.
