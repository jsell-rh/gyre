---
title: "Implement user management CLI commands"
spec_ref: "user-management.md §CLI"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §CLI"
commits: ["7d8f93d904e47303e5d17b20497e1b4b0191cedb", "dacb3b04"]
---

## Spec Excerpt

From `user-management.md` §CLI:

```bash
# Identity
gyre whoami                                  # Current user profile
gyre profile set --display-name "Jordan Sell" --timezone "America/New_York"
gyre sessions list                           # Active sessions
gyre sessions revoke <session-id>
gyre sessions revoke-all

# Tenant invitations (TenantAdmin only)
gyre tenant invite alice@example.com
gyre tenant invite alice@example.com --workspace platform --role Developer
gyre tenant invite --bulk users.csv
gyre tenant invitations list
gyre tenant invitations revoke <invitation-id>

# Workspace membership
gyre workspace members list
gyre workspace invite @alice --role Developer
gyre workspace invite alice@example.com --role Developer
gyre workspace members set-role @asmith Admin
gyre workspace members set-role @asmith Owner --transfer
gyre workspace members remove @asmith
gyre workspace invitations list
gyre workspace invitations revoke <invitation-id>

# Teams
gyre team create "Platform Team" --lead @jsell
gyre team add @asmith --team "Platform Team"
gyre team list
gyre team remove @asmith --team "Platform Team"

# Notifications
gyre notifications list
gyre notifications list --unread --priority high
gyre notifications preferences set --email-digest hourly --email-min-priority high
gyre notifications preferences set --slack-url https://hooks.slack.com/...
```

## Implementation Plan

1. **Audit existing CLI commands** in `gyre-cli/`:
   - Check which commands already exist (gyre whoami, gyre status, etc.)
   - Identify the CLI framework and command registration pattern

2. **Identity commands:**
   - `gyre whoami` — GET /api/v1/users/me, display profile
   - `gyre profile set` — PUT /api/v1/users/me with provided fields
   - `gyre sessions list` — GET /api/v1/users/me/sessions
   - `gyre sessions revoke` — DELETE /api/v1/users/me/sessions/:id
   - `gyre sessions revoke-all` — POST /api/v1/users/me/sessions/revoke-all

3. **Tenant invitation commands** (TenantAdmin only):
   - `gyre tenant invite` — POST /api/v1/tenant/invite
   - `gyre tenant invite --bulk` — POST /api/v1/tenant/invite/bulk (parse CSV)
   - `gyre tenant invitations list` — GET /api/v1/tenant/invitations
   - `gyre tenant invitations revoke` — DELETE /api/v1/tenant/invitations/:id

4. **Workspace membership commands:**
   - `gyre workspace members list` — GET /api/v1/workspaces/:id/members
   - `gyre workspace invite` — POST /api/v1/workspaces/:id/invite
   - `gyre workspace members set-role` — PUT /api/v1/workspaces/:id/members/:user_id/role
   - `gyre workspace members remove` — DELETE /api/v1/workspaces/:id/members/:user_id
   - `gyre workspace invitations list/revoke`

5. **Team commands:**
   - `gyre team create` — POST /api/v1/workspaces/:id/teams
   - `gyre team add` — POST /api/v1/workspaces/:id/teams/:team_id/members
   - `gyre team list` — GET /api/v1/workspaces/:id/teams
   - `gyre team remove` — DELETE /api/v1/workspaces/:id/teams/:team_id/members/:user_id

6. **Notification commands:**
   - `gyre notifications list` — GET /api/v1/notifications with filters
   - `gyre notifications preferences set` — PUT /api/v1/notifications/preferences

## Acceptance Criteria

- [ ] `gyre whoami` displays current user profile
- [ ] `gyre profile set` updates display name, timezone, locale
- [ ] Session management commands (list, revoke, revoke-all)
- [ ] Tenant invitation commands (invite, bulk, list, revoke)
- [ ] Workspace membership commands (list, invite, set-role, remove)
- [ ] Team commands (create, add, list, remove)
- [ ] Notification commands (list with filters, preferences set)
- [ ] All commands use consistent output formatting
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §CLI for the full command tree. Existing CLI: `gyre-cli/src/` — check `main.rs` or `commands/` for the command registration pattern and existing commands. API client: look for how the CLI makes HTTP requests to the server (likely a shared client module). The CLI should use the same endpoints that the API tasks (task-124, task-125) create. Check `docs/cli.md` for existing CLI documentation.

## Shipped

Production work: commit `7d8f93d9` (feat(task-126)) — every command in
the spec's §CLI block now exists in `gyre-cli` as a real client
invocation of the user-management API. The assigned task body above is
left byte-identical to the assigned contract, including the unchecked
acceptance criteria — checking those boxes is a normative change
reserved for the independent reviewer.

### What each command group does

- **Identity** — `gyre whoami` prints the full profile from
  `GET /api/v1/users/me` (id, username, display name, email, timezone,
  locale, global role, roles, last login, created/updated). `gyre
  profile set` sends a partial `PUT /api/v1/users/me` body containing
  only the requested `--display-name/--timezone/--locale/--avatar-url`
  fields (omitted fields keep stored values server-side); at least one
  field is required and empty values are rejected before any request.
- **Sessions** — `gyre sessions list/revoke/revoke-all` call the spec'd
  §API > User Management session endpoints
  (`GET/DELETE /api/v1/users/me/sessions[/:id]`,
  `POST .../revoke-all`). **These are the only commands in this task
  whose server endpoints do not exist yet** — they are owned by
  task-111 (§Session Management, not-started), the same split task-124
  recorded for its API rows. The CLI side is complete and real (exact
  request paths asserted by tests); against a current server the
  commands surface the server's 404 until task-111 lands.
- **Tenant invitations** — `gyre tenant invite <email>` posts to
  `POST /api/v1/tenant/invite` with optional `--workspace`/`--role`
  pre-assignment (`--role` without `--workspace` is rejected);
  `gyre tenant invite --bulk users.csv` parses the CSV client-side
  (email[,workspace][,role]; header row recognized, fields trimmed and
  unquoted, non-email rows rejected) and posts one
  `POST /api/v1/tenant/invite/bulk` request; `gyre tenant invitations
  list/revoke` call `GET/DELETE /api/v1/tenant/invitations[/:id]`.
  **These endpoints are owned by task-110 (§Tenant-Level User
  Onboarding, not-started)** — same caveat as sessions: real client
  code, 404 against a current server until task-110 lands. The exact
  request bodies (`{email, workspace, role}` per invitation) follow the
  spec's TenantInvitation pre-assignment fields; when task-110 pins its
  wire contract, any field-name drift is a one-line client fix.
- **Workspace membership** — `members list/set-role/remove` call
  `GET /workspaces/:id/members`,
  `PUT .../members/:user_id/role`, `DELETE .../members/:user_id`;
  `--transfer` requires role Owner and defaults the audit reason;
  `invite @alice` resolves the handle via `GET /api/v1/users/:username`
  (tenant-public profile) then posts `{user_id, role}` to
  `POST .../invite` (role defaults to Developer); `invite
  alice@example.com` takes the tenant-invitation path with the target
  workspace and role pre-assigned (spec: "sends tenant invite if
  needed" — the email path depends on task-110's endpoint). Workspace
  invitations list/revoke call `GET/DELETE .../invitations[/:id]`.
- **Teams** — `create` resolves the `--lead @handle` to a user id
  before creating (so a bad lead fails before the team exists) and
  posts `{name, lead, description}`; `add`/`remove` resolve both the
  user handle and the team (matched by slug first, then name) and post
  to `.../teams/:team_id/members[/:user_id]`; `list` prints the team
  table.
- **Notifications** — `list` calls the spec path
  `GET /api/v1/notifications` with `?priority=&type=&workspace_id=&limit=`;
  `--priority` is validated against the four-level names client-side;
  `--unread` filters the response's `read` flag client-side (the API
  filter surface has no unread parameter) over the server's maximum
  window (200). `preferences set` is a read-merge-write over
  `GET`+`PUT /api/v1/notifications/preferences`: only touched channels
  change, `--email-*` flags enable the email channel and normalize
  digest/priority to the wire representation (Hourly/High), `in_app` is
  forced true (the server rejects `in_app: false`), and a configured
  Slack/webhook channel survives an email-only update.

### Shared machinery

`--workspace <slug>` or git-remote inference selects the target
workspace (same `resolve_workspace_slug` path `gyre budget` uses);
`@handle`/bare-username/user-id arguments resolve through
`GET /api/v1/users/:username`; role names (Owner/Admin/Developer/
Viewer), digest frequencies (immediate/hourly/daily/off), and priority
levels (low/medium/high/urgent) are validated before any request is
sent. Output follows the existing CLI conventions: profile blocks like
`gyre status`, tables like `gyre inbox`/`gyre secret list`, single-line
confirmations for mutations. `docs/cli.md` documents the full group
including the task-110/111 dependency notes.

### Test evidence (in-sandbox, build wrapper)

`cargo test -p gyre-cli` — 189 passed, 0 failed (112 pre-existing + 37
new + rename-affected; log: /tmp/stage/review-evidence/gyre-cli-tests.txt,
exit 0). New tests: parse tests for every command line in the spec's
§CLI block; pure-logic tests (CSV parsing incl. header/quotes/bad
rows, digest/priority/role validation, email-vs-handle detection,
channel merge semantics incl. preservation of untouched channels and
in_app repair); scripted mock-server flow tests asserting the exact
request sequences and bodies (`profile set` sends only requested
fields; member set-role sends role+reason to the spec path; workspace
invite by handle resolves then posts `{user_id, role}`, by email takes
the tenant-invite path with pre-assignment; team create/add/remove
resolve lead/user/team and post to the right ids; tenant invite single
and bulk bodies; notifications preferences GET-merge-PUT with the
merged body asserted; sessions revoke/revoke-all request lines; and
client-side-rejection tests proving bad input produces zero requests).
`cargo clippy -p gyre-cli --all-targets --all-features` — zero gyre-cli
warnings (pre-existing gyre-common/gyre-domain warnings untouched;
log: /tmp/stage/review-evidence/clippy-gyre-cli.txt). `cargo fmt
--check` clean. Mechanical gates re-run clean: check-arch,
check-byte-slice-truncation, check-relative-path-defaults
(truncate_for_table uses chars, never byte slicing).

### Verifier gates (not run in this sandbox)

- `cargo test --all` — the assignment's definition-of-done gate.
- `cargo test -p gyre-cli` — includes all 37 new task-126 tests.
- Full-workspace Clippy and the changed-line gate
  (`scripts/check-clippy-diff.py`) — gyre-cli is warning-free locally.
- `scripts/check-task-commit-attribution.sh` — commit `7d8f93d9` is
  recorded in this task's `commits:` frontmatter.
