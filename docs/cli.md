# Gyre CLI Reference

The `gyre` CLI is a thin client for interacting with a Gyre server.

---

## Setup (M3.3)

```bash
# Register this CLI instance as a named agent; saves token + agent ID to ~/.gyre/config
gyre init --server http://localhost:3000 --name my-agent --token gyre-dev-token

# Clone a Gyre-hosted repository (uses token from ~/.gyre/config)
gyre clone myproject/myrepo            # clones into ./myrepo/
gyre clone myproject/myrepo --dir /tmp/work

# Push current branch (uses token from ~/.gyre/config)
gyre push                              # pushes to origin
gyre push --remote gyre

# Push through warn-mode gyre-stack.lock drift with a recorded override reason
# (supply-chain.md §The Laptop Problem — the reason lands in the repo's
# stack-drift audit trail; without it the forge rejects warn-mode drift)
gyre push --stack-override-reason "security hotfix: model upgrade approved on-call"
```

Config file is stored at `~/.gyre/config` (JSON):

```json
{
  "server": "http://localhost:3000",
  "token": "<per-agent-auth-token>",
  "agent_id": "<uuid>",
  "agent_name": "my-agent"
}
```

### Bootstrap (platform-model.md §8)

```bash
# Full non-dev bootstrap: tenant, admin user, workspace, repo, built-in
# personas, default gates, and a repo orchestrator — one command.
gyre bootstrap \
  --tenant "Acme Corp" \
  --workspace "Platform Team" \
  --repo gyre \
  --repo-path /home/user/code/gyre \
  --admin-user jsell \
  --oidc-issuer https://keycloak.example.com/realms/acme \
  --server http://localhost:3000 \
  --token gyre-dev-token

# Dev mode: skips OIDC + admin user, uses static tokens, tenant "dev",
# workspace "default"
gyre bootstrap --dev

# Also write a starter spec structure (specs/manifest.yaml, specs/index.md,
# specs/system/design-principles.md, AGENTS.md, .prek.yaml) into --repo-path
# (required: --starter-kit without --repo-path is an error — the kit must land
# at an explicit path, never a cwd-relative default)
gyre bootstrap --dev --starter-kit --repo-path /home/user/code/myrepo
```

Steps performed in order: health check → create tenant → create admin user
(skipped in `--dev`) + API key saved to `~/.gyre/config` → create workspace →
register repo (bare repo initialized server-side) → register + pre-approve the
four built-in personas (workspace-orchestrator, repo-orchestrator,
accountability, security) → repository import and spec registry sync (when a
local manifest is available) →
default gates (`cargo test`, `cargo clippy`, `scripts/check-arch.sh`, detected
from `--repo-path`) → spawn the repo orchestrator → print summary with all
IDs, the one-time API key, and URLs.

To import Gyre itself, point `--repo-path` at a local Gyre Git checkout with
at least one commit. `--starter-kit` requires `--repo-path` and creates
`specs/manifest.yaml` there if it is missing. Without `--starter-kit`,
bootstrap skips import if you omit `--repo-path` or the checkout lacks a
manifest. A supplied path is still used to detect default gates.

When the manifest exists, bootstrap pushes the checkout's `HEAD` and its
history to Gyre's `main`, then syncs the server's spec registry. Commit the
changes you want to import first. If existing starter-kit paths have pending
changes, bootstrap stages them and makes a local commit. Any unrelated changes
already staged can be included in that commit and pushed with it.

A normal push creates a missing `main` or fast-forwards it when the checkout
is ahead. A matching `HEAD` needs no branch update. To replace the server's
empty seed, bootstrap uses a force-with-lease pinned to the inspected commit
SHA. Any parentless commit with an empty tree qualifies for this replacement,
regardless of who created it. For any other `main` with commits missing from
the checkout, such as changes merged on the server, bootstrap refuses the
push. Bootstrap reports import failures and continues. If the checkout shares
history with the intended Gyre repository, pull from or rebase onto its
`main` before retrying a refused push.

The run is resumable: re-running `gyre bootstrap` with the same names reuses
already-created resources (tenant, workspace, repo, personas are matched by
slug/name; the admin user's saved API key in `~/.gyre/config` is reused; a
live repo orchestrator is kept rather than re-spawned). If a step still fails,
the CLI reports which steps completed. The admin API key is shown exactly
once. Auth token precedence: `--token` flag > `GYRE_AUTH_TOKEN` env var >
`gyre-dev-token` default.

---

## Agent Operations (M3.3)

```bash
# Show this agent's registered status and current task
gyre status

# List tasks (optional filters)
gyre tasks list
gyre tasks list --status in_progress
gyre tasks list --mine                 # only tasks assigned to this agent

# Assign a task to this agent and mark it in_progress
gyre tasks take <task-id>
```

---

## Merge Requests (M3.3)

```bash
# Create a merge request for the current branch
gyre mr create --title "My feature" --repo-id <repo-uuid>

# Custom source/target branches
gyre mr create --title "Fix bug" --repo-id <repo-uuid> \
  --source fix/my-bug --target main
```

### Verify a merge attestation (agent-gates.md §Part 3)

```bash
# Verify the merge attestation bundle attached to a merge commit:
# reads the bundle from the commit's git note
# (refs/notes/merge-attestations), runs the verification protocol
# (bundle signature, per-gate signatures, spec approval signature,
# consistency) via POST /api/v1/attestations/verify, and prints the
# per-component result. Exits 1 when the bundle fails verification.
gyre verify-attestation <commit-sha>

# From a different checkout / notes ref
gyre verify-attestation <commit-sha> --repo /path/to/clone \
  --notes-ref refs/notes/merge-attestations

# Explicit server/token (default: ~/.gyre/config)
gyre verify-attestation <commit-sha> --server http://localhost:3000 --token <token>
```

Notes live in a separate git ref; fetch them into a clone with
`git fetch origin refs/notes/merge-attestations:refs/notes/merge-attestations`
before verifying a merge made on the server.

---

## Spec Operations

```bash
# Get LLM-suggested edits for a spec file
gyre spec assist system/vision.md "Add a section on observability"
gyre spec assist system/vision.md "Simplify principle 3" --repo myrepo --workspace myws

# Show all links (outbound and inbound) for a spec
gyre spec links system/identity-security.md

# Show specs that depend on a given spec (impact analysis)
gyre spec dependents system/source-control.md

# Full tenant-wide spec dependency graph (text summary)
gyre spec graph

# Spec graph in Graphviz DOT format (pipe to dot for PNG)
gyre spec graph --format dot | dot -Tpng -o graph.png

# List all stale spec links across the tenant
gyre spec stale-links

# List all active conflicts (specs with conflicts_with where both are approved)
gyre spec conflicts
```

---

## Budget (platform-model.md §5)

```bash
# Current repo budget usage — resolves the repo's owning workspace
# from the git remote (the budget governance boundary is the workspace;
# there is no repo-level budget store)
gyre budget show

# Workspace-level usage (named workspace, or inferred from git remote)
gyre budget show --workspace --workspace-name platform

# Tenant-level usage summary (Admin only)
gyre budget show --tenant

# Set repo budget (the owning workspace's limits; Admin only)
gyre budget set --llm-tokens 500000

# Set workspace cost limit
gyre budget set --workspace --llm-cost 100.00

# Other limits: --max-agents, --max-agent-lifetime-secs
# Omitted limits keep their current values (fetch-merge-put).
```

Notes:

- Calls the real budget endpoints (`GET/PUT /api/v1/workspaces/:id/budget`,
  `GET /api/v1/budget/summary`); setting limits requires the Admin role.
- A limit exceeding the tenant ceiling surfaces the server's cascade error;
  tenant-level limits cannot be set (no tenant-level set endpoint exists).
- `--llm-cost` must be a finite non-negative amount (USD): NaN, Infinity,
  and negative values are rejected before any request is sent (a non-finite
  value would serialize as `null` and silently clear the limit). A finite
  zero cap is valid and explicitly disables all spending for the day.

---

## Secrets (platform-model.md §7)

```bash
# Create (or update) a secret in the current workspace — workspace inferred
# from the git remote (Admin only server-side)
gyre secret set DATABASE_URL "postgres://user:pass@host/db"

# Repo-scoped secret (workspace from --workspace-name or the git remote)
gyre secret set --repo gyre DATABASE_URL "postgres://..."

# Tenant-scoped rotated secret with an expiry (unix epoch seconds)
gyre secret set --scope tenant --secret-type rotated --expires-at 1893456000 \
  API_KEY "secret-value"

# List secret metadata for a scope (names, types, rotation/expiry state —
# values are never returned by the API)
gyre secret list

# Force-rotate a secret's value
gyre secret rotate API_KEY "new-value"

# Delete a secret by name
gyre secret delete API_KEY
```

Notes:

- Calls the real Admin API (`POST/GET /api/v1/secrets`,
  `DELETE /api/v1/secrets/:id`, `POST /api/v1/secrets/:id/rotate`); every
  operation requires the Admin role.
- Secret values are write-only: they are sent on set/rotate and never
  displayed or returned — the list response physically contains no value
  field.
- Scope selection: `--repo <name>` selects repo scope; `--scope tenant`
  selects the caller's tenant; otherwise workspace scope via
  `--workspace-name <slug>` or the git remote. `gyre secret delete/rotate`
  are name-keyed and resolve the id via the scope's list.

---

## Policies (abac-policy-engine.md §CLI)

```bash
# List all policies (id, name, scope, effect, priority, enabled, built-in)
gyre policy list

# Workspace-scoped policies for one workspace
gyre policy list --scope workspace --workspace-id ws-1

# Create a policy from a YAML definition (Admin only server-side)
gyre policy create --file policy.yaml

# Dry-run: would the current policy set allow this? (nothing is audited)
gyre policy evaluate --subject user:jsell --action approve --resource spec:identity

# Decision audit log with filters (newest first)
gyre policy decisions --subject agent:worker-42 --decision deny --since 1d

# "What can jsell do with specs?" — answered from the subject's real
# stored attributes (roles, memberships, teams)
gyre policy effective --subject user:jsell --resource-type spec
```

The `policy.yaml` file for `create` mirrors the `POST /api/v1/policies`
body — required fields `name`, `scope` (tenant/workspace/repo), `priority`,
`effect` (allow/deny), `actions`, `resource_types`; optional `description`,
`scope_id`, `conditions`, `enabled` (default true):

```yaml
name: workspace-developer-push
description: Allow developers to push in this workspace
scope: workspace
scope_id: ws-1
priority: 200
effect: allow
conditions:
  - attribute: subject.workspace_role
    operator: equals
    value: developer
actions: [push, read]
resource_types: [repo]
```

Notes:

- Calls the real policy API (`GET/POST /api/v1/policies`,
  `POST /api/v1/policies/evaluate`, `GET /api/v1/policies/decisions`,
  `GET /api/v1/policies/effective`); create requires the Admin role
  server-side (the seeded `builtin:policy-admin-only` policy).
- `--subject`/`--resource` take the `{type}:{id}` form (`user:jsell`,
  `spec:identity`); for `decisions` and `effective` the type prefix is CLI
  syntax — the audit log and the effective explorer key on the subject id.
- `decisions` applies the `--decision`/`--since` filters to the fetched
  newest-first page (default 100 records; raise with `--limit` to scan
  deeper into the log). `--since` accepts `s`/`m`/`h`/`d`/`w` durations.
- Server-side failures (auth 403, validation 400, not-found 404) are
  reported with the HTTP status and the server's message.

---

## User Management (user-management.md §CLI)

```bash
# Identity

gyre whoami                                   # Current user profile

gyre profile set --display-name "Jordan Sell" --timezone "America/New_York"
gyre profile set --locale en-US               # Omitted fields keep their values

# Active sessions

gyre sessions list
gyre sessions revoke <session-id>
gyre sessions revoke-all

# Tenant invitations (TenantAdmin only)

gyre tenant invite alice@example.com                              # Invite to tenant
gyre tenant invite alice@example.com --workspace platform --role Developer  # With workspace pre-assignment
gyre tenant invite --bulk users.csv                               # Bulk invite from CSV
                                                 # (columns: email[,workspace][,role])
gyre tenant invitations list
gyre tenant invitations revoke <invitation-id>

# Workspace membership

gyre workspace members list
gyre workspace invite @alice --role Developer                     # By username (already in tenant)
gyre workspace invite alice@example.com --role Developer          # By email (sends tenant invite if needed)
gyre workspace members set-role @asmith Admin
gyre workspace members set-role @asmith Owner --transfer          # Transfer ownership
                                                 # (--transfer requires role Owner; audited server-side)
gyre workspace members remove @asmith
gyre workspace invitations list
gyre workspace invitations revoke <invitation-id>

# Teams

gyre team create "Platform Team" --lead @jsell
gyre team add @asmith --team "Platform Team"
gyre team list
gyre team remove @asmith --team "Platform Team"

# Notifications

gyre notifications list                      # Recent notifications
gyre notifications list --unread --priority high
gyre notifications preferences set --email-digest hourly --email-min-priority high
gyre notifications preferences set --slack-url https://hooks.slack.com/...
```

Notes:

- Identity, workspace-membership, team, and notification commands call the
  real user-management API (`GET/PUT /api/v1/users/me`,
  `GET/PUT .../workspaces/:id/members[...]/role`, `POST .../invite`,
  `GET/DELETE .../invitations[/:id]`, team endpoints under
  `.../workspaces/:id/teams`, and `GET/PUT /api/v1/notifications[/...]`).
- `@handle` arguments are resolved to user ids via `GET /api/v1/users/:username`
  (the tenant-public profile); email arguments take the tenant-invitation
  path with the workspace pre-assigned.
- The target workspace of `gyre workspace`/`gyre team` commands is
  `--workspace <slug>` or the one inferred from the gyre git remote (the
  same inference `gyre budget` uses).
- `gyre notifications list --unread` filters the read flag client-side
  (the API's filter surface is workspace/priority/type/repo); `--priority`
  takes the four-level names (low/medium/high/urgent) and is enforced by
  the server.
- `gyre notifications preferences set` performs a read-merge-write: only
  the channels you touch change (setting `--email-*` enables the email
  channel; `in_app` is always on per the spec).
- `gyre sessions ...` calls the §API > User Management session endpoints
  (`/api/v1/users/me/sessions[...]`, owned by task-111) and `gyre tenant
  invite`/`gyre tenant invitations ...` call the §API > Tenant Invitations
  endpoints (`/api/v1/tenant/...`, owned by task-110); those command groups
  become fully functional when their server-side endpoints land.

---

## Search

```bash
# Simple full-text search across all entities
gyre search "identity security"

# Filter by entity type (spec, task, mr, commit, agent) — server-side
gyre search "merge queue" --type spec

# Filter by live status (task/mr/agent state) — client-side
gyre search "ABAC" --type task --status in_progress

# Workspace scope by slug — resolved server-side
gyre search "budget" --workspace platform-team

# Recency filter: relative (7d) or ISO date (2026-03-01)
gyre search "auth.rs" --since 7d

# Autocomplete for a title prefix
gyre search --suggest "iden"

# Cap result count (default 20, server max 100)
gyre search "queue" --limit 5
```

Notes:

- `--type` and `--workspace` are enforced by the server (`entity_type` /
  `workspace_id` params on `GET /api/v1/search`).
- `--status` and `--since` filter against the entity's live state
  (fetched per-result from the task/MR/agent detail endpoints), because
  the search index snapshots status at creation time. Results whose
  live state cannot be resolved (spec/commit types, deleted entities)
  are excluded and reported on stderr.
- `--suggest` falls back to a title-prefix filter over regular search
  results until the dedicated `/api/v1/search/suggest` endpoint lands
  (task-153).


## Supply Chain (task-165)

```bash
# Generate gyre-stack.lock from the agent's registered stack and write it
# to the repo (run from the repo root; commit the file to pin the stack)
gyre stack lock

# Write to a custom path
gyre stack lock --output lockfiles/gyre-stack.lock
```

Notes:

- `gyre stack lock` fetches the agent's registered stack
  (`GET /api/v1/agents/{id}/stack`) and serializes it as TOML: every stack
  component (agents_md_hash, hooks, mcp_servers, model, cli_version,
  settings_hash, persona_hash) plus the composite fingerprint and
  `lock_timestamp` (supply-chain.md §gyre-stack.lock).
- The committed lockfile is enforced at push time: the server parses it
  from the pushed tree, verifies its internal integrity (recomputes the
  fingerprint), and flags drift against the pushing agent's attested
  stack — rejecting or warning per the repo stack policy's enforcement

---

## Connection / Diagnostics

```bash
# Connect to a running gyre-server (interactive session)
gyre connect --server ws://localhost:3000/ws --token gyre-dev-token

# Ping the server and measure round-trip time
gyre ping --server ws://localhost:3000/ws --token gyre-dev-token

# Check server health via HTTP
gyre health --server http://localhost:3000

# Launch the TUI dashboard (exits on 'q')
gyre tui --server ws://localhost:3000/ws --token gyre-dev-token
```

Default `--server` is `ws://localhost:3000/ws` and default `--token` is `gyre-dev-token`
(matches server defaults, so bare `gyre ping` works against a local dev server).
