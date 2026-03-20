# M10: Persistent Storage & Real-time Events

## Goal

Data survives server restarts. Domain events broadcast in real-time via
WebSocket so the UI stays live without polling.

## Problem

The current in-memory store (`mem.rs`) loses all state on restart. The WebSocket
exists but doesn't broadcast domain events — the UI polls on mount and never
updates. This makes Gyre unusable for any real workflow.

## Deliverables

### M10.1 SQLite Persistence
- Replace in-memory HashMaps with SQLite via the existing port traits
- Migrations for all entity tables: projects, repos, agents, tasks, merge_requests, merge_queue, activity, analytics, costs, audit, snapshots, retention, network_peers, compute_targets, users, api_keys, siem_targets
- Connection pool (r2d2 or deadpool-sqlite)
- `GYRE_DATABASE_URL` env var (default: `sqlite://gyre.db`)
- Data directory: `GYRE_DATA_DIR` (default: `./data/`)
- All existing tests continue to pass (use in-memory SQLite for tests)

### M10.2 Real-time WebSocket Events
- Domain event bus: publish events on all mutations (create/update/delete)
- Event types: AgentStatusChanged, TaskTransitioned, MRCreated, MRMerged, ActivityRecorded, QueueUpdated
- WebSocket broadcasts events to all connected clients
- Frontend subscribes and updates views reactively (no full-page polling)
- Activity feed updates in real-time

### M10.3 Git Repository Storage
- Bare git repos stored on disk at `GYRE_REPOS_DIR` (default: `./repos/`)
- `POST /api/v1/repos` creates both the DB record and `git init --bare`
- Smart HTTP git endpoints serve from actual repos on disk
- Branch listing, commit log, and diff use real git data (libgit2 or shell out to git)

## Acceptance Criteria

- [ ] Server restart preserves all data
- [ ] Creating an entity in the UI appears in real-time on another browser tab
- [ ] Git clone/push works against Gyre-hosted repos
- [ ] 500+ tests passing
- [ ] No data loss on graceful shutdown
