# Review: task-127 — Implement user management UI pages

**Candidate:** `fcdcf5401aeb36ee1f03b427d7755106db58109f` (base `e8f73c061f1eaad0b38024a60de8599432b7f2c8`)
**Verdict:** Approved — independent evidence supports the task contract.

## Scope reviewed

Diff base..candidate across backend (users.rs, invitations.rs, notification port +
sqlite/postgres/mem/broadcasting adapters, abac_middleware, api/mod.rs, lib.rs),
frontend (App.svelte routing, InvitationAccept.svelte, TenantSettings,
WorkspaceSettings, UserProfile, NotificationBell/Drawer, api.js, locales, tests),
docs, and spec/coverage amendments (ui-navigation.md bell amendment, coverage
row 31 → implemented).

## Findings

None blocking. One cosmetic nit, recorded for completeness: in
`web/src/__tests__/InvitationAccept.test.js` the `TOKEN` constant is
`'***'.repeat(8)` while its comment says "64-hex magic-link token". The token
value is opaque to the component under test (shape disambiguation lives in
`parseUrl` and is pinned by AppShell.test.js with real 64-hex/UUID fixtures),
so no assertion is weakened.

## Independent evidence (saved under /tmp/stage/review-evidence)

1. **Web suite**: `cd web && npm ci && npm test` → exit 0, 127 files,
   2563 passed / 41 skipped / 0 failed.
2. **Adapter tests**: `cargo test -p gyre-adapters --lib notification` →
   14 passed, including `list_active_for_user_excludes_resolved_and_dismissed`
   against real SQLite storage.
3. **Mutation probe**: removing the `resolved_at/dismissed_at IS NULL` guards
   from `list_active_for_user` makes that test FAIL
   (`left: ["n-read","n-dismissed","n-resolved","n-active"]`) — the test kills
   the disabled production behavior and is not self-confirming. File restored
   byte-for-byte afterwards (git diff clean).
4. **Server type check**: `cargo check -p gyre-server --lib --tests` → clean.
5. **Mechanical invariants** (all exit 0): check-arch, check-abac-route-registry
   (new `/api/v1/users/:id/role` resolver entry, no exemption-file growth),
   check-abac-exempt-handlers, check-mem-port-contracts, check-unwired-port-methods.
6. **Prior verification failure disposition**: attempt ed736d08's full-suite
   failure (`preview_resolves_tenant_invitation_with_preassigned_workspaces`
   panicking on a missing `invite_url`) is a genuine test bug, correctly fixed
   by candidate commit 19372722: the test invited with `workspace_roles:
   ["Maintainer"]`, a role that exists nowhere in the spec nor in
   `WorkspaceRole::parse_role`; production `POST /api/v1/tenant/invite`
   correctly rejects unknown roles with 400 (verified in source,
   invitations.rs ~line 294), so no `invite_url` existed to unwrap. The fix
   uses the spec-real `Admin` role and adds a diagnosable CREATED assertion;
   assertion strength is unchanged (role round-trips through the
   unauthenticated preview). Not a masked production bug.

## Code-review assessment

- `PUT /api/v1/users/:id/role`: real TenantAdmin enforcement via the stored
  record (`is_tenant_admin`), same-tenant containment, self-change and
  deactivated-target 409s, unknown-role 400, GlobalRole↔UserRole lockstep
  matching the invitation/provisioning mapping, `user_global_role_changed`
  audit event. Tests cover promotion/demotion/no-op audit counts,
  forbidden caller, cross-tenant, and deactivated targets through real
  one-shot HTTP calls against real state.
- `GET /api/v1/invite/:token/preview`: unauthenticated magic-link trust model
  identical to the pre-existing accept/decline routes (256-bit CSPRNG token
  hashed at rest; registered on the top-level router outside
  require_auth_middleware exactly like its siblings). Strictly read-only —
  expired-but-Pending rows report Expired without persisting the transition
  (pinned by test), and no credential/token material is returned (pinned by
  test). Unknown token → 404.
- `?active=true` drawer listing: new `NotificationRepository::
  list_active_for_user` port method implemented consistently in SQLite,
  PostgreSQL, mem, and the broadcasting wrapper; empty type-set semantics
  match `list_for_user`'s include-filter contract; the default (Inbox)
  listing is unchanged. The mem adapter guards the same contract in code.
- Frontend: `InvitationAccept.svelte` implements both disjoint URL shapes
  (64-hex token vs UUID workspace id) with preview/accept/decline via the
  correct endpoints, one-time API-key display with copy, load-error and
  terminal-status none-states; `api.js` gains `skipAuth` so magic-link calls
  never present a mismatched session identity. TenantSettings (Users +
  Invitations tabs, TenantAdmin-gated with 403 fallback states, inline
  global-role select, bulk invite, revoke), WorkspaceSettings (member role
  select incl. ownership transfer with reason, invite by username, pending
  invitations with expiry countdown and revoke, teams CRUD), UserProfile
  (sessions list/revoke/revoke-all). Svelte 5 runes throughout.
- Spec lifecycle: the bell-supersedes-decisions-badge change is recorded as a
  dated amendment in ui-navigation.md §1 (and docs/ui.md), with the Decisions
  surface preserved on workspace home — an amendment, not a silent
  contradiction.
- Coverage matrix row 31 (§UI Pages) marked implemented accurately;
  `grep -c '| not-started |'` on user-management.md returns 0.

## Deferred to the independent verifier (build-wrapper budget)

- `cargo test --all` (full suite; the candidate's new server tests:
  `api::users::tests::update_user_role_*` (×2),
  `api::users::tests::spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  `api::invitations::tests::preview_*` (×4)).
- Web bundle rebuild (`web/dist/` is verifier-owned; my probe run regenerated
  it as a side effect of `npm test` and was restored to the committed state).
- The three gawk-based notification check scripts could not run in this
  sandbox (mawk parse errors; scripts unchanged by the candidate, so the
  limitation is environmental, not a defect).
