# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (coverage row 31).
Candidate: `cf3db6a2e6259b5c9a929da78c18398b6edc37e2` on
`pipeline/task-127/4700b40f6bbb46998dd5833869d95429-2`, assigned base
`6f5f4082` (main head). Verdict: **approved** — independent evidence supports
the task contract; remaining section gaps are honestly recorded as open and
do not fake completion.

## Merge-state repair (durable finding d14fff80)

The previously published head (`486ab25f`) was DIRTY against current main.
The candidate resolves it: merge commit `70d85873` ("verify(task-127):
reconcile candidate with main") merges main head `6f5f4082` into the branch,
so the assigned base is an ancestor of the candidate and the
`6f5f4082..cf3db6a2` diff is exactly the task-127 branch work (28 files,
+3339/−222). Conflicts were confined to generated `web/dist` (resolved to
base; the verifier owns the final rebuild) — main's task-185 sources
(`SpecChatInput`, `SpecSuggestion`, `specAssist`, `DetailPanel`,
`EditorSplit`) are disjoint from task-127's surfaces and intact in the
merged tree.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci && npm test` → **124 files / 2470 tests passed, 41
  skipped, exit 0** (`vitest-candidate.log`). Full suite, not a subset.
- `cargo test -p gyre-adapters --lib list_active_for_user` → **pass**
  (real SQL-level test: resolved/dismissed rows excluded from the
  active-only listing while `list_for_user` keeps all four rows).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` →
  clean (the new `?active=true` handler test type-checks).
- `cargo test --all` deferred to the independent verifier per the 4 GiB
  build-wrapper constraint; required commands recorded in
  `cargo-probes.log` (incl. `cargo test -p gyre-server --lib
  spec_path_notifications_active_filter` and the full-history attribution
  script that times out in this sandbox).

## Verified working

- **Every closable acceptance criterion ships as real Svelte 5 (runes)
  production code consuming real endpoints** (source-inspected, not
  label-trusted):
  - User Settings + Session Management (`UserProfile.svelte`): profile
    edit via `PUT /users/me`, preferences, notification channels via
    `PUT /notifications/preferences`, sessions via task-111's
    `GET /users/me/sessions` + per-session DELETE + revoke-all.
  - User Profile `/@username` (`PublicUserProfile.svelte` + App.svelte
    parseUrl/urlFor/pushState/popstate): profile, accepted memberships
    with roles, team memberships, public-action activity feed, 404 state.
  - Tenant User Management (`TenantSettings` Users tab, TenantAdmin-gated
    with a not-admin state): `GET /users` listing + search, deactivate via
    `PUT /users/:id/deactivate`, provisioning via `POST /users`.
  - Workspace Members (`WorkspaceSettings` Members tab): role-change
    dropdown incl. ownership transfer with audit-reason prompt (cancelled
    prompt aborts), remove, invite-by-username (handle resolved via
    `GET /users/:username`), pending invitations with status badge,
    expiry countdown, and revoke (Pending-only, matching the 409
    contract).
  - Team Management (Teams tab): create/edit/delete + add/remove members
    over the task-122 endpoints.
  - Notification Drawer (`NotificationDrawer.svelte` +
    `NotificationBell.svelte`): unread badge via
    `GET /notifications/unread-count`, Urgent→Low grouping, mark
    read/unread + mark-all-read, deep links (server `action_url`,
    InvitationReceived → `/invite/{ws}`, no-fabricated-target disable),
    type/priority/workspace/repo filters, realtime reload on WS push with
    a stale-response guard. Backed by a real backend addition:
    `?active=true` → new `NotificationRepository::list_active_for_user`
    implemented in SQLite, PostgreSQL, mem, and the broadcasting wrapper,
    with the SQL-level adapter test above.
  - Invitation Accept (`InvitationAccept.svelte` at `/invite/{workspaceId}`):
    workspace preview, accept (`POST /workspaces/{id}/invite/accept`) and
    decline, expiry label, 404 / none / load-error states; api.js errors
    now carry `err.status` so the 404 branch is reachable in production.
- **Spec amendment is legitimate, not contradiction:** ui-navigation.md §1
  carries a dated (2026-11, task-127) bell amendment reconciling the old
  decisions-count badge with user-management.md §UI Pages/§In-App
  Notification UI; docs/ui.md and the e2e shell test were updated to the
  same contract (not test weakening — the assertion follows the amended
  spec).
- **Test quality:** new frontend tests are behavioral (exact endpoint
  shapes incl. `{limit: 50, active: true}`, grouping order, disabled
  no-target items, 404/none/load-error states, filter pass-through,
  deactivate/provision flows) and fail if the production behavior breaks.
  No self-confirming or assertionless tests found in the new files.
- **Attribution:** every product-surface commit on the branch
  (`1a32b4e6`, `b6966093`, `1deff0b5`, `0f127bc0`, `fe84485b`,
  `4de47315`, `50f338e9`, `64c85ec9`, `4aa5a87a`, `8d72ffec`, `c5902874`,
  `e468155e`, `f5aee7da`, `14f27ab5`, plus build commits) is recorded in
  the committed task-127.md `commits:` frontmatter. The full-history
  script check belongs to the verifier (sandbox timeout, recorded).

## Open gaps — honestly recorded, not blocking (section stays task-assigned)

Coverage row 31 remains `task-assigned` (not implemented/verified) with
these rows open, each verified genuine during this review:

- **`/invite/{token}` magic-link accept page.** The spec row wants a
  token-keyed page with workspace preview; only POST
  `/api/v1/invite/{token}/accept|decline` exist in-tree — there is no
  unauthenticated token-preview endpoint (verified against
  `api/invitations.rs` route registrations), so the specced preview cannot
  be rendered pre-acceptance. The shipped `/invite/{workspaceId}` form
  covers the authenticated InvitationReceived deep-link flow against
  task-110's merged WorkspaceInvitation contract. Building the token page
  requires backend surface owned by the invitation-endpoint tasks, not
  this UI task; faking a preview-less token page would contradict the
  spec. Correctly left open.
- **Tenant Invitation Management page** (task-110's
  `/tenant/invite`, `/tenant/invite/bulk`, `/tenant/invitations`,
  `DELETE /tenant/invitations/{id}` are in-tree with no UI consumer) and
  **inline global-role editing** (no endpoint). Neither is an acceptance
  criterion of this task; both are listed as remaining in the coverage
  row.

These are the goal's sanctioned "section stays open" path (real work
shipped where possible, gaps documented, no fake completion), not
defects of this candidate.

## Generated-output note (non-blocking by current policy)

`web/dist/` in the candidate resolves generated merge conflicts to the
assignment-base versions; the deterministic verifier rebuilds and binds
publication to the tested tree. Incidental evidence during this review:
`cargo check -p gyre-server` ran build.rs, which rebuilt the web bundle
from the candidate's source without errors (regenerated files restored
afterwards; working tree left at the stage's task-file normalization
only).
