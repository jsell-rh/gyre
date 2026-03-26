# Repo Lifecycle

> This spec defines how repositories are created, configured, imported, archived, and deleted within workspaces. It fills a gap: `platform-model.md` defines the `Repository` struct and hierarchy, `human-system-interface.md` defines navigation, but neither specifies the full CRUD lifecycle or where it lives in the UI.

## The Problem

Repos are the unit of work in Gyre — specs, tasks, agents, MRs, and merge queues are all repo-scoped. But there's no defined flow for:
- Creating a new repo within a workspace
- Importing an existing external repo
- Configuring repo settings (gates, policies, budget)
- Archiving or deleting repos
- Discovering and navigating to repos within a workspace

The HSI rewrite removed the legacy RepoList/RepoDetail views from primary navigation (correctly — they're contextual drill-downs). But it didn't define where repo management lives in the new model.

---

## 1. Where Repo Management Lives

### Admin → Workspace Scope → Repos Tab

Repo management is a tab in the Admin view at workspace scope, alongside the existing tabs (Workspace settings, Budget, Trust Level, Teams, Policies).

**Admin tabs at workspace scope:**
```
Settings | Budget | Trust Level | Teams | Policies | Repos
```

The **Repos tab** shows:
- List of all repos in the workspace (name, status, agent count, last activity)
- "+ New Repo" button (creates a new repo)
- "Import Repo" button (imports an existing external repo)
- Click a repo → navigates to repo scope (breadcrumb updates, all views re-render for that repo)

### Admin → Repo Scope → Settings

At repo scope, the Admin view shows repo-specific configuration:

**Admin tabs at repo scope:**
```
Settings | Gates | Policies | Danger Zone
```

- **Settings:** Name, description, default branch, max concurrent agents, budget allocation
- **Gates:** Gate chain configuration (test commands, lint commands, agent review toggle, trace capture)
- **Policies:** Repo-level spec policies (require spec_ref, require approval, stale spec warning)
- **Danger Zone:** Archive repo, delete repo (with confirmation)

### Explorer → Workspace Scope

The Explorer at workspace scope already shows repos as part of the architecture view. Clicking a repo node in the Explorer enters repo scope — this is the primary discovery path for day-to-day use. Admin is for management, Explorer is for navigation.

---

## 2. Repo Creation

### New Repo

**UI:** Admin → Workspace scope → Repos tab → "+ New Repo"

**Form fields:**
| Field | Required | Default | Description |
|---|---|---|---|
| Name | Yes | — | Unique within workspace. Used in cross-workspace spec links. `[a-zA-Z0-9._-]` |
| Description | No | — | Human-readable purpose |
| Default branch | No | `main` | Branch name for the merge target |
| Initialize | No | Yes | Create initial commit with empty `specs/manifest.yaml` |

**What gets bootstrapped on creation:**
1. Bare git repo created on server filesystem
2. If initialized: first commit with `specs/manifest.yaml` (empty spec list)
3. Default gate chain created: `test` (no command — disabled until configured), `spec-review` (agent review, enabled by default)
4. Budget inherited from workspace (no repo-level override until explicitly set)
5. Repo appears in workspace's repo list and Explorer

**API:** `POST /api/v1/repos` (existing endpoint)

```json
Request:
{
  "workspace_id": "<uuid>",
  "name": "payment-api",
  "description": "Payment processing service",
  "default_branch": "main",
  "initialize": true
}

Response 201:
{
  "id": "<uuid>",
  "workspace_id": "<uuid>",
  "name": "payment-api",
  "path": "/data/repos/<uuid>.git",
  "default_branch": "main",
  "created_at": 1711468800
}
```

### Import External Repo

**UI:** Admin → Workspace scope → Repos tab → "Import Repo"

**Purpose:** Connect an existing Git repository (GitHub, GitLab, self-hosted) to Gyre. The repo is mirrored — Gyre maintains a local copy and syncs periodically.

**Form fields:**
| Field | Required | Description |
|---|---|---|
| Clone URL | Yes | HTTPS or SSH URL of the external repo |
| Name | Yes | Name within workspace (defaults to repo name from URL) |
| Auth | Conditional | PAT, SSH key, or none (for public repos) |
| Sync interval | No | How often to pull from upstream (default: 5 minutes) |
| Default branch | No | Detected from remote HEAD, overridable |

**What happens on import:**
1. Server clones the external repo (bare)
2. Sets `is_mirror: true` on the repo record
3. Creates a sync job (periodic `git fetch` from upstream)
4. If the repo has a `specs/manifest.yaml`, parses and registers specs
5. If no manifest, creates an empty one on the default branch

**API:** `POST /api/v1/repos/mirror` (exists in code, not previously specced)

**Mirror sync:** `POST /api/v1/repos/:id/mirror/sync` triggers an immediate sync. The periodic sync job also runs on the configured interval.

**Bidirectional sync (future):** Initially mirrors are read-only (Gyre pulls from upstream). Bidirectional push (Gyre pushes MRs upstream) is a future extension requiring webhook integration with the external forge.

---

## 3. Repo Configuration

### Settings (Admin → Repo Scope → Settings)

| Setting | Type | Default | Description |
|---|---|---|---|
| Name | String | — | Rename repo (updates cross-workspace spec link `target_display`) |
| Description | String | — | Human-readable purpose |
| Default branch | String | `main` | Merge target for all MRs |
| Max concurrent agents | u32 | Workspace default | Limit parallel agent execution in this repo |
| Budget | BudgetConfig | Inherited from workspace | Repo-level budget override (cannot exceed workspace) |

**API:** `PUT /api/v1/repos/:id` (new endpoint)

```json
Request:
{
  "name": "payment-api-v2",
  "description": "Updated payment service",
  "max_agents": 5,
  "budget": {"max_tokens_per_day": 1000000, "max_cost_per_day": 50.0}
}

Response 200: updated repo object
```

### Gates (Admin → Repo Scope → Gates)

Gate configuration is per-repo. See `agent-gates.md` for gate types and execution semantics.

**UI shows:**
- List of configured gates (name, type, enabled/disabled toggle)
- "+ Add Gate" button with gate type selector
- Per-gate configuration (test command, lint command, reviewer persona, etc.)
- Drag to reorder (gates execute in order)

**API:** Existing `GET/POST /api/v1/repos/:id/gates`, `PUT/DELETE /api/v1/repos/:id/gates/:gate_id`

### Spec Policies (Admin → Repo Scope → Policies)

Per-repo spec enforcement policies. See `spec-lifecycle.md` for semantics.

**UI shows toggles for:**
- Require `spec_ref` on all MRs
- Require fully approved spec before merge
- Warn on stale spec
- Require current spec (blocks merge queue when stale)

**API:** Existing `GET/PUT /api/v1/repos/:id/spec-policy`

---

## 4. Repo Archival and Deletion

### Archive

Archiving a repo makes it read-only. No new agents can be spawned, no new MRs created, no pushes accepted. Existing MRs are closed. The repo remains visible in the Explorer (grayed out) and its specs/history are preserved.

**UI:** Admin → Repo Scope → Danger Zone → "Archive Repo" (confirmation dialog)

**What happens:**
1. Repo status set to `Archived`
2. All active agents in the repo are gracefully stopped (60-second grace period)
3. All open MRs are closed with reason "Repo archived"
4. All non-terminal tasks (`InProgress`, `Backlog`, and `Blocked`) are cancelled
5. Git push hook rejects new pushes
6. Agent spawn rejects new agents scoped to this repo
7. Repo appears grayed out in Explorer and repo lists
8. If `is_mirror: true`, the periodic mirror sync job is **paused** (no upstream fetches while archived)

**Unarchive:** Reverses the above — resumes mirror sync if applicable. "Unarchive Repo" button appears in Danger Zone when archived.

**API:** `POST /api/v1/repos/:id/archive`, `POST /api/v1/repos/:id/unarchive`

### Delete

Deleting a repo permanently removes it. This is destructive and irreversible.

**UI:** Admin → Repo Scope → Danger Zone → "Delete Repo" (requires typing repo name to confirm)

**Preconditions:**
- Repo must be archived first (cannot delete an active repo)
- No cross-workspace spec links reference this repo as a target (or they must be removed first)

**What happens:**
1. All repo data removed from database (tasks, MRs, agents, specs, graph, gates)
2. If `is_mirror: true`, stored mirror credentials (PAT, SSH key) are deleted from the credential store
3. Bare git repo removed from filesystem
4. Workspace repo count decremented

**API:** `DELETE /api/v1/repos/:id` (new endpoint, requires archived status)

---

## 5. Repo Discovery

Users discover repos through three paths:

1. **Explorer at workspace scope:** The architecture view shows repos as top-level nodes. Click to enter repo scope. This is the primary path for day-to-day navigation.

2. **Admin → Repos tab:** Lists all repos with metadata (name, status, agent count, last activity). Click to enter repo scope. This is the management path.

3. **Breadcrumb dropdown:** When at workspace scope, clicking the repo segment of the breadcrumb shows a dropdown of all repos in the workspace. This is the quick-switch path.

4. **Cmd+K global search:** Repos appear in search results. Selecting one enters repo scope.

---

## 6. Domain Changes

### Repository Status

Add a `status` field to the `Repository` struct:

```rust
pub enum RepoStatus {
    Active,     // Normal operation
    Archived,   // Read-only, no new agents/MRs/pushes
}

// Only NEW and CHANGED fields shown. Existing fields from platform-model.md
// (id, workspace_id, name, path, default_branch, budget, max_agents, is_mirror,
// created_at) and implementation-specific fields (mirror_url, mirror_interval_secs,
// last_mirror_sync) are retained unchanged.
pub struct Repository {
    // ... existing fields ...
    pub description: Option<String>,   // NEW
    pub status: RepoStatus,            // NEW (default: Active)
    pub updated_at: u64,               // NEW
}
```

### API Summary

| Endpoint | Method | Purpose | Auth |
|---|---|---|---|
| `GET /api/v1/repos` | GET | List repos (filtered by `?workspace_id=`) | ABAC: read on repo |
| `POST /api/v1/repos` | POST | Create repo | ABAC: write on workspace |
| `GET /api/v1/repos/:id` | GET | Get repo details | ABAC: read on repo |
| `PUT /api/v1/repos/:id` | PUT | Update repo settings | ABAC: write on repo (Admin) |
| `DELETE /api/v1/repos/:id` | DELETE | Delete repo (must be archived) | ABAC: delete on repo (Admin) |
| `POST /api/v1/repos/:id/archive` | POST | Archive repo | ABAC: `action_override: "archive"` on repo (Admin) |
| `POST /api/v1/repos/:id/unarchive` | POST | Unarchive repo | ABAC: `action_override: "archive"` on repo (Admin) |
| `POST /api/v1/repos/mirror` | POST | Import external repo as mirror | ABAC: write on workspace |
| `POST /api/v1/repos/:id/mirror/sync` | POST | Trigger mirror sync | ABAC: write on repo |

---

## Relationship to Existing Specs

**Amends:**

| Spec | Amendment |
|---|---|
| `platform-model.md` §Repository | Add `description`, `status`, `updated_at` fields to `Repository` struct. Add `RepoStatus` enum. |
| `human-system-interface.md` §1 Admin row | Add "Repos" to workspace-scope Admin tabs. Clarify repo creation lives here. |
| `hierarchy-enforcement.md` §4 | Add `PUT /api/v1/repos/:id`, `DELETE /api/v1/repos/:id`, `POST /api/v1/repos/:id/archive`, `POST /api/v1/repos/:id/unarchive`, `POST /api/v1/repos/mirror`, `POST /api/v1/repos/:id/mirror/sync` to route table with ABAC resource mappings. Archive/unarchive use `action_override: "archive"` (same action for both — same privilege level controls both operations). |

**Depends on:**
- `platform-model.md` — Repository struct, workspace hierarchy
- `agent-gates.md` — gate configuration per repo
- `spec-lifecycle.md` — spec policy configuration per repo
- `human-system-interface.md` — navigation model, Admin view structure
