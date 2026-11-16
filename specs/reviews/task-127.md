# Review — task-127 (Implement user management UI pages)

**Candidate:** `ec542eaf` (base `e5df7847`) · **Verdict: NOT approved** — one
mechanical-invariant failure introduced by the candidate; the shipped code
itself is real and tested.

## Blocking finding

`scripts/check-task-commit-attribution.sh` FAILS at the candidate (exit 1) and
passes at the base (exit 0) — the candidate introduces the failure:

```
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:
  fdc9962e  task-110  fix(task-127): re-point invitation UI surfaces to the merged task-110 contract
```

`fdc9962e`'s subject mentions `task-110` in prose ("the merged task-110
contract"), and the script requires every `task-NNN` appearing in a
product-surface commit subject to be recorded in that task's `commits:`
frontmatter. The commit is correctly listed in task-127's frontmatter but not
task-110's. This is a frozen CI check (exemption file growth is forbidden), so
verification will fail on it. Fix: reword the commit subject (drop the prose
`task-110` mention, e.g. "…to the merged WorkspaceInvitation contract") via the
executor's branch rewrite, or record `fdc9962e` in `specs/tasks/task-110.md`'s
`commits:` frontmatter.

Evidence: `/tmp/stage/review-evidence/check-attribution-candidate.log` (exit 1),
`check-attribution-base.log` (exit 0).

## Minor finding (non-blocking, fix in the same round)

`web/src/components/InvitationAccept.svelte`: the `loadError` state is dead —
it is declared and nulled but never assigned, so the `{:else if loadError}`
branch never renders. A non-404 failure of `GET /workspaces/{id}` (e.g. 500)
silently renders the invite card with an empty workspace name instead of the
error state. Reproduce: mock `api.workspace` to reject with `status: 500`.

## Verified real (independent evidence, exact candidate tree)

- **Pages all real Svelte 5 runes components** consuming registered routes
  (verified route-by-route against `api/mod.rs`): User Settings + preferences +
  Session Management (`UserProfile.svelte`: PUT /users/me, GET/PUT
  /notifications/preferences, GET /users/me/sessions, DELETE …/sessions/:id,
  POST …/sessions/revoke-all); Tenant User Management (`TenantSettings.svelte`:
  GET /users, PUT /users/:id/deactivate, POST /users; TenantAdmin-gated with
  not-admin note); Workspace Members/Invitations/Teams (`WorkspaceSettings.svelte`:
  PUT /members/:user_id/role with ownership-transfer reason, POST /invite, GET
  /invitations, DELETE /invitations/:id, teams CRUD + membership); Invitation
  Accept (`InvitationAccept.svelte` at `/invite/{workspaceId}`: GET
  /users/me/invitations → Pending row, POST /invite/accept, POST
  /invite/decline, expiry countdown, 404 state via the now-status-carrying
  `api.js` errors); Notification Drawer (`NotificationDrawer.svelte`: unread
  badge, priority grouping, active-only `?active=true` listing, deep links with
  no fabricated targets, mark read/unread, mark-all-read).
- **Rust side real**: new `list_active_for_user` port method implemented in
  SQLite, PostgreSQL, mem, and the broadcasting wrapper — the SQL guards
  `resolved_at/dismissed_at IS NULL` in the query itself. `?active=true` wired
  in `api/users.rs` with an end-to-end routed handler test.
- **Focused vitest** (after `npm ci`, 9 task-127 files): 346/346 passed, exit 0
  (`web-focused-vitest-rerun.log`). Prior round's 11-file run: 289/289.
- **Mutation probes** (tests are load-bearing): removing the drawer's
  `active: true` param fails exactly the active-only test
  (`web-mutation-drawer-active-review.log`, source restored); the interrupted
  round's probe removing the SQL active guard fails
  `list_active_for_user_excludes_resolved_and_dismissed`
  (`review-resume/.../cargo-test-adapters-mutation-review.log`).
- **`cargo check -p gyre-server`**: clean, exit 0. **`cargo test -p
  gyre-adapters --lib sqlite::notification::tests::list_active_for_user_excludes_resolved_and_dismissed`**:
  pass. **check-arch / check-mem-port-contracts / check-abac-exempt-handlers /
  check-in-memory-state-stores**: all exit 0.
- **No dangling references** to the removed task-124 parallel invitation
  handlers in `api/mod.rs`/`api/users.rs`.

## Honest reporting (no fake completion)

Coverage row 31 stays `task-assigned` with the remaining gaps enumerated:
Tenant Invitation Management page (task-110 endpoints in-tree, no page), the
`/invite/{token}` magic-link page (token endpoints exist but no
unauthenticated token-preview endpoint for the specced workspace preview), and
inline global-role editing. The `/invite/{workspaceId}` key (vs the spec's
`/invite/{token}`) is a documented consequence of the merged task-110
WorkspaceInvitation contract — the authenticated page delivers the specced
preview + accept/decline purpose — and the section is not claimed closed over
it. The bell-behavior change is covered by a dated spec-lifecycle amendment to
`ui-navigation.md`. This framing is acceptable.

Minor observations (non-blocking): the workspace invite modal is username-only
(the task plan sketched "by username or email"; the email path is task-110's
tenant-invite flow, covered by the task-126 CLI); `cargo check`'s build.rs
rebuilt `web/dist/` during probing — restored; generated bundle state is the
verifier's domain.

## For the independent verifier (4 GiB budget — not run here)

- `cargo test --all`
- gyre-server: `api::users::tests::spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  task-110 `api::invitations` suite, task-114 profile/activity/stats suites
- `cd web && npm ci && npm test` (full suite; prior round recorded 2435 passed at
  the round-4 head)
