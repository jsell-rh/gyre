# Review: task-127 — Implement user management UI pages

**Candidate:** `9c1425d90ab57b09a04c981700578910905b627f`
**Base:** `047d652495dde3b1ae6ddbea4f8cf1b077a3c18f`
**Round:** re-review of the review-repair candidate (prior round flagged the
attribution invariant and a dead error state in `InvitationAccept.svelte`).
**Verdict:** approved.

## Scope of this round

The delta from the prior-reviewed merge base `41420fb1` is exactly the two
claimed repairs plus specs bookkeeping:

- `14f27ab5` — `fix(task-127)`: `loadError` assignment + load-error render
  branch in `web/src/components/InvitationAccept.svelte` and a
  distinguishing vitest case.
- `fc7d03ad`, `9c1425d9` — process-only commits touching
  `specs/tasks/task-127.md` (review-repair notes, commits-list update).
  No product surface.

## What was verified

1. **Attribution invariant (blocking prior round):** the on-branch rewrite
   `fdc9962e` → `f5aee7da` removed the prose `task-110` mention from the
   commit subject. `bash scripts/check-task-commit-attribution.sh` on the
   exact candidate tree (full history): exit 0, "OK: every task-labeled
   product-surface commit is recorded". Evidence:
   `check-attribution-candidate.log`.
2. **Load-error repair:** `load()` now assigns `loadError` on non-404
   workspace-fetch failures and the template renders
   `invite_accept.load_error_title` (key present in `en.json`) with no
   invite card and no accept/decline actions.
3. **Bug-proof probe:** with ONLY `InvitationAccept.svelte` reverted to the
   pre-fix blob (`14f27ab5^`), the new test
   "renders the load-error state when the workspace fetch fails (non-404)"
   FAILS (invite card rendered); with the fix it passes. The test kills the
   reviewed bug, not a mirror of it. Working tree restored and verified
   clean afterwards. Evidence: `vitest-bugproof-prefix.log` (exit 1),
   `vitest-invitationaccept-candidate.log` (8/8 pass).
4. **Focused frontend suite at the candidate:** InvitationAccept +
   InvitationDeclineNavigate (8/8) and the other task-touched test files
   (AppShell, NotificationDrawer, TenantSettings, UserProfile,
   WorkspaceSettings, Inbox — 336/336) after `npm ci`. Evidence:
   `vitest-touched-candidate.log`.
5. **Server type check:** `/tmp/stage/bin/cargo check -p gyre-server` at
   the candidate: clean, 0 errors (`cargo-check-gyre-server.log`). The
   Rust tree is byte-identical to the prior-verified merge base
   (`git diff 41420fb1..9c1425d9 -- crates/` is empty), so no new Rust
   surface is unreviewed this round.

## Honesty of the completion state

The coverage row (user-management.md §UI Pages, row 31) stays
`task-assigned` with the remaining gaps named explicitly — Tenant
Invitation Management page (task-110 endpoints in-tree, no consuming page),
the `/invite/{token}` magic-link form (no unauthenticated token-preview
endpoint exists for the specced workspace preview), and inline global-role
editing (no endpoint). The `/invite/{workspaceId}` deviation from the
excerpt's `/invite/{token}` is documented in the component and coverage
notes and is consistent with the merged task-110 invitation contract. No
section is falsely marked implemented.

## For the independent verifier

- `cargo test --all` (workspace) — full suites deferred per the 4 GiB
  build budget; in particular the gyre-server invitation/user/session/team
  handler suites, `api::invitations`, the notification active-filter
  tests, and the task-114 profile/activity/stats suites.
- `cd web && npm ci && npm test` (full vitest).
- `bash scripts/check-task-commit-attribution.sh` on the final merged tree
  (needs full history).
