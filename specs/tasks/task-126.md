---
title: "Implement user management CLI commands"
spec_ref: "user-management.md §CLI"
depends_on:
  - task-124
  - task-125
progress: not-started
coverage_sections:
  - "user-management.md §CLI"
commits: []
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
