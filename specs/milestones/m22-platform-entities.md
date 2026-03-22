# M22: Platform Entities

**Status:** In Progress  
**Goal:** Core platform entities for multi-tenant governance, budget control, spec governance, cross-repo dependency tracking, declarative ABAC, full-text search, and user management.

## Sub-Milestones

### M22.1 – Workspace + Persona Entities
- `Workspace` (tenant-scoped, links repos), `Persona` (reusable agent role + prompt template, `PersonaScope`: Global/Project/Task)
- 12 REST endpoints:
  - `POST/GET /api/v1/workspaces` — create (**Admin only**) / list
  - `GET/PUT/DELETE /api/v1/workspaces/{id}` — read / update (**Admin only**) / delete (**Admin only**)
  - `POST/GET /api/v1/workspaces/{id}/repos` — add / list repos
  - `POST/GET /api/v1/personas` — create (**Admin only**) / list
  - `GET/PUT/DELETE /api/v1/personas/{id}` — read / update (**Admin only**) / delete (**Admin only**)
- `GET /api/v1/repos/{id}` response includes `workspace_id: Option<Id>`

### M22.2 – Budget Governance
- `BudgetConfig` (limits: daily/monthly spend, spawn count) and `BudgetUsage` (current totals) entities
- `POST /api/v1/agents/spawn` returns `429 Too Many Requests` when daily spawn budget is exhausted
- 3 REST endpoints:
  - `GET /api/v1/workspaces/{id}/budget` — current BudgetConfig + BudgetUsage
  - `PUT /api/v1/workspaces/{id}/budget` — set budget limits (**Admin only**)
  - `GET /api/v1/budget/summary` — tenant-wide budget + per-workspace breakdown (**Admin only**)
- Background job: `spawn_budget_daily_reset` resets daily usage counters

### M22.3 – Spec Links
- `SpecLink` entity with `SpecLinkType`: `implements`, `supersedes`, `depends_on`, `conflicts_with`, `extends`, `references`
- SHA-pinned links; forge enforcement via M21.1-C approval gates
- 3 REST endpoints:
  - `GET /api/v1/specs/{path}/links` — outbound and inbound links
  - `GET /api/v1/specs/graph` — full tenant-wide spec link graph
  - (spec approval endpoint enforces link constraints, see M21.1-C)

### M22.4 – Cross-Repo Dependency Graph
- `DependencyEdge` with `DependencyType` (code/api/schema/spec/data) and `DetectionMethod` (auto/manual)
- Cargo.toml auto-detection on push to default branch
- 6 REST endpoints:
  - `GET /api/v1/repos/{id}/dependencies` — outgoing edges
  - `GET /api/v1/repos/{id}/dependents` — incoming edges
  - `POST /api/v1/repos/{id}/dependencies` — add manual edge (**Admin only**, H-12)
  - `DELETE /api/v1/repos/{id}/dependencies/{dep_id}` — remove edge (**Admin only**, H-13)
  - `GET /api/v1/repos/{id}/blast-radius` — BFS transitive dependents
  - `GET /api/v1/dependencies/graph` — full tenant-wide DAG

### M22.6 – Declarative ABAC Policy Engine
- Domain types: `Policy`, `Condition`, `ConditionOp` (8 operators: Equals, NotEquals, In, NotIn, GreaterThan, LessThan, Contains, Exists), `ConditionValue`, `PolicyEffect`, `PolicyScope`, `PolicyDecision`
- 3 built-in system policies; priority + scope ordering; first-match-wins; default-deny
- `AttributeContext` merges JWT claims for evaluation
- 5 REST endpoints:
  - `POST/GET /api/v1/policies` — create / list
  - `GET/PUT/DELETE /api/v1/policies/{id}` — CRUD
  - `POST /api/v1/policies/evaluate` — dry-run evaluation
  - `GET /api/v1/policies/decisions` — decision audit log
  - `GET /api/v1/policies/effective` — effective permissions explorer

### M22.7 – Full-Text Search
- `SearchPort` trait + `MemSearchAdapter` (in-memory)
- Searches across: agents, tasks, repos, projects, merge requests, specs
- `gyre_search` MCP tool added (9th tool in MCP catalog)
- 2 REST endpoints:
  - `GET /api/v1/search?q=&entity_type=&workspace_id=&limit=20`
  - `POST /api/v1/search/reindex` — admin reindex (**Admin only**, H-14)

### M22.8 – User Management, Workspace Membership, Teams, Notifications
- Enhanced `User` entity: `username`, `display_name`, `avatar_url`, `timezone`, `locale`, `tenant_id`, `global_role`, `UserPreferences`
- `WorkspaceMembership` with `WorkspaceRole` (Owner/Admin/Developer/Viewer), accept/pending lifecycle
- `Team` — workspace-scoped, idempotent `add_member`/`remove_member`
- `Notification` — 16 `NotificationType` variants, 4 `NotificationPriority` levels; auto-fired on: agent complete → `MrNeedsReview`, gate failure → `GateFailure`, MR merged → `MrMerged`
- 12 REST endpoints:
  - `GET/PUT /api/v1/users/me`
  - `GET /api/v1/users/me/agents`
  - `GET /api/v1/users/me/tasks`
  - `GET /api/v1/users/me/mrs`
  - `GET /api/v1/users/me/notifications`
  - `PUT /api/v1/users/me/notifications/{id}/read`
  - `POST/GET /api/v1/workspaces/{id}/members`
  - `PUT /api/v1/workspaces/{id}/members/{user_id}` — update member role
  - `DELETE /api/v1/workspaces/{id}/members/{user_id}`
  - `POST/GET /api/v1/workspaces/{id}/teams`
  - `PUT/DELETE /api/v1/workspaces/{id}/teams/{team_id}`

## Security Notes
- **H-12**: add_dependency — AdminOnly (prevents graph manipulation)
- **H-13**: delete_dependency — AdminOnly
- **H-14**: reindex — AdminOnly (prevents DoS via expensive reindex)
- **H-15**: workspace write endpoints — AdminOnly (prevents budget bypass)
- **H-16**: persona write endpoints — AdminOnly (prevents prompt injection)
