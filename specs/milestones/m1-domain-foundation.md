# Milestone 1: Domain Foundation

Build the core domain model and persistence layer. After M1, Gyre has real entities (projects, repositories, agents, tasks, merge requests) with persistent storage, a functional REST API, and a dashboard that shows live state.

## Deliverables

### 1. Domain Model (gyre-domain)

Flesh out the domain entities with proper aggregates and value objects:

- **Project** - top-level container (name, description, settings)
- **Repository** - git repo managed by Gyre (name, path, default branch, project reference)
- **Agent** - an agent identity (name, status, parent, current task, lifetime budget, spawned_at)
- **Task** - a unit of work (title, description, status, assignee, parent task, labels, branch, PR link)
- **MergeRequest** - a proposed change (title, source branch, target branch, status, author agent, reviewers, created_at)
- **ActivityEvent** - move from in-memory to persisted domain entity

Status enums, lifecycle transitions, and domain invariants must be enforced in the domain layer.

### 2. Port Traits (gyre-ports)

Define repository port traits for each aggregate:

- `ProjectRepository` - CRUD for projects
- `RepoRepository` - CRUD for repositories (the domain entity, not git)
- `AgentRepository` - CRUD for agents
- `TaskRepository` - CRUD + query (by status, assignee, parent)
- `MergeRequestRepository` - CRUD + query (by status, repo, author)
- `ActivityRepository` - append + query (by time range, agent, event type)

### 3. SQLite Adapters (gyre-adapters)

Implement all port traits for SQLite:

- Proper schema with migrations (use refinery or sqlx migrations)
- Foreign keys, indexes for common queries
- Connection pooling (r2d2 or deadpool)
- All operations tested

### 4. REST API (gyre-server)

Full CRUD REST API under `/api/v1/`:

- `POST/GET /api/v1/projects` - create, list
- `GET/PUT/DELETE /api/v1/projects/{id}` - read, update, delete
- `POST/GET /api/v1/repos` - create, list
- `POST/GET /api/v1/agents` - register, list
- `PUT /api/v1/agents/{id}/status` - update agent status
- `POST/GET /api/v1/tasks` - create, list (with query params)
- `PUT /api/v1/tasks/{id}` - update task
- `PUT /api/v1/tasks/{id}/status` - transition task status
- `POST/GET /api/v1/merge-requests` - create, list
- `PUT /api/v1/merge-requests/{id}/status` - transition MR status
- `GET /api/v1/activity` - query activity log (already exists, enhance)

JSON request/response types derived from domain entities. Proper error responses.

### 5. Svelte Dashboard

Replace the hello-world SPA with a functional dashboard:

- **Navigation** - sidebar with Projects, Agents, Tasks, Activity, Settings
- **Activity Feed** - live-updating, filterable by agent/event type
- **Agent List** - show all agents with status, current task, uptime
- **Task Board** - kanban-style view (backlog, in_progress, review, done)
- **Project/Repo List** - basic listing pages
- Uses real API endpoints, not mock data

### 6. Agent Lifecycle via API

The server can manage agent lifecycle:

- `POST /api/v1/agents` - register agent (returns agent ID + auth token)
- `PUT /api/v1/agents/{id}/heartbeat` - agent heartbeat (server tracks liveness)
- `GET /api/v1/agents/{id}/messages` - agent polls for messages
- `POST /api/v1/agents/{id}/messages` - send message to agent
- Server detects stale agents (no heartbeat for N seconds) and marks them dead

## Success Criteria

- All domain entities have full CRUD via REST API
- SQLite persistence with proper migrations
- Dashboard shows live agent and task state
- Agent heartbeat/liveness detection works
- Domain logic has no infrastructure dependencies (arch lint passes)
- 50+ tests across domain, adapters, and API layers

## Non-Goals for M1

- No git forge operations yet (clone, push, pull - that's M2)
- No merge queue implementation (M2)
- No SSO/SCIM, no SPIFFE
- No jj integration
- No WireGuard/Tailscale networking
- No eBPF audit
- No CI runner integration

## Dependencies

- M0 Walking Skeleton (complete)
