# Review — task-110 (Implement tenant & workspace invitation flow)

Spec: `specs/system/user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations.
Candidate under review: `66a2edb88ad09e61256e408dad2b406ec7802a37`, base `def0add9eb44d8e790aae3c1502317435104c0bf`.
Evidence: `/tmp/stage/review-evidence/task-110-review-round6.md` (+ raw guard-script and test logs alongside it). Prior rounds' records: review-checkpoint branch commits (`66c5ce2a8` and earlier).

## Round 6 (independent review of the repair-round-5 merge + F1a/F1b closure)

Verdict: **approved.**

### What changed since the last reviewed tree, and what was verified

Delta from the round-5 candidate `c1f347a4f` to `66a2edb88`: merge of upstream `def0add9e` (task-157 meta-spec rollout policies, independently approved on main) as `dbfc76e13`; lint-classification fix `8c469c538`; two specs-only process commits. Verified: `8c469c538` is +11 comment lines with zero non-comment changes; the two process commits touch only `specs/tasks/task-110.md`; the full base→candidate production diff is the previously-reviewed task-110 surface plus the independently-reviewed upstream task-157 surface.

### F1a/F1b closure (the blocking findings from round 5)

- `bash scripts/check-non-atomic-creation.sh` at the candidate → **exit 0** (was exit 1 / 2 violations).
- The `// non-atomic-create:ok - <reason>` markers are the check's own documented exemption syntax (pre-existing pattern in `snapshot.rs`, `git_http.rs`, `spawn.rs`), not an invented bypass.
- Both markers' rationales are true of the code, verified by reading it:
  - `accept_tenant_invitation` compensates on failure — rolls the claimed invitation back Accepted→Pending via `transition_status`; retry dedups (user found via `find_by_external_id` before create; membership creation skips existing rows), so the cross-repo creation is idempotent and recoverable as claimed.
  - `invite_to_workspace`'s notification create is an explicit best-effort `let _ =` side channel; the invitation itself is the primary durable effect and the invitee's durable surface (`GET /api/v1/users/me/invitations`) cannot be orphaned by a lost notification.

### Independent re-verification of the task contract at the candidate

- **Focused suites, all green:** `api::invitations` 18/18 (full lifecycle, revocation-blocks-accept, expiry + background job marks Expired, bulk invite with per-item error reporting, HTTP-layer 403 for non-admin tenant invite and non-member workspace invite through the real routing chain, bearer-identity match/mismatch on token accept, invitee accept/decline, CSPRNG+hash known-answer, policy configurability), `auth::` 46/46, `reconciliation` 36/36 (upstream suite at the merged tree — merge interaction), `gyre-adapters` full 388 passed / 0 failed / 12 ignored including the fresh-DB migration boot over the combined migration set (task-110 invitations + user-tenant-scope index + upstream rollout_policies; migration versions unique).
- **Crypto:** 32-byte ring `SystemRandom` CSPRNG token, SHA-256 hash only persisted.
- **Mechanical checks, all exit 0:** non-atomic-creation, arch, abac-route-registry, migration-versions, mem-port-contracts, fabricated-scope-defaults, warn-continue-creation, task-commit-attribution. Exemption-file edits in the diff are shrink/re-anchor/comment only — no new entries.
- **`check-dependent-scope-resolution.sh`** fails at the candidate with exactly one finding — upstream task-157's `maybe_advance_rolling_rollout` (`reconciliation.rs:925`) — and fails with an identical finding at the assigned base `def0add9e` itself (verified in a base worktree, findings diffed): pre-existing mainline condition inherited by the merge, not introduced or modified by task-110, and disclosed in the shipped record. Left for an upstream-side fix.
- **Frontmatter:** the candidate's `commits:` list matches the assignment's declared list; the dropped `dbfc76e13` is a merge commit of independently-reviewed upstream content (out of the attribution check's scope by design); the fix commit `8c469c538` is listed, so review scoping loses nothing.

### Scope notes

- A live-server probe was not repeated this round: the only production change since the round-4 live HTTP probe of the same surface is 11 comment lines, and the in-process tests exercise the full middleware/routing chain. Full-workspace `cargo test --all` and the live-server smoke belong to host verification.
- Round-4's non-blocking observation (API-key auth branch resolves `tenant_id: "default"` regardless of the linked user's tenant) is unchanged pre-existing platform behavior outside this diff; recommend a separate follow-up task.

— Reviewer, 2026-11-08 (round 6; evidence under /tmp/stage/review-evidence)
