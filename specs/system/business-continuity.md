# Business Continuity

Gyre is a platform that orchestrates autonomous agents. If the platform goes down, agents go down with it — there is no fallback orchestrator. This makes reliability a first-class concern, not an afterthought.

## Recovery Objectives

| Tier | Scenario | RTO Target | RPO Target |
|---|---|---|---|
| T1 | Server process crash (in-memory mode) | < 1 min (restart) | 0 (no persistent state) |
| T2 | Server crash with SQLite DB | < 2 min (restart + WAL replay) | < 30 s (last WAL checkpoint) |
| T3 | Host failure (SQLite) | < 15 min (restore from snapshot) | < 1 h (last snapshot) |
| T4 | Host failure (PostgreSQL) | < 5 min (standby promotion) | < 30 s (streaming replication) |
| T5 | Data corruption | < 30 min (snapshot restore + replay) | last clean snapshot |

**RTO / RPO rationale:**
- In-memory mode is for development and CI. No persistence expectation.
- SQLite with WAL is the default production mode. WAL checkpoints every 30 s by default.
- PostgreSQL with streaming replication is recommended for multi-replica or SLA-bound deployments.
- Snapshot cadence (default: 1 h) bounds RPO for T3/T5. Admins can shorten via cron + `POST /api/v1/admin/snapshot`.

---

## BCP Primitives

### 1. Snapshot / Restore

Point-in-time DB snapshots via admin API:

```
POST /api/v1/admin/snapshot          → create snapshot
GET  /api/v1/admin/snapshots         → list snapshots (name, size, timestamp)
POST /api/v1/admin/restore           → restore from snapshot
DELETE /api/v1/admin/snapshots/{id}  → delete old snapshot
```

Snapshots are written to `GYRE_SNAPSHOT_PATH` (default: `./snapshots/`). Each snapshot is a self-contained file. For PostgreSQL, snapshots delegate to `pg_dump`. For SQLite, they use an atomic VACUUM INTO.

**Retention:** configurable via `PUT /api/v1/admin/retention`. Default policy: keep 24 hourly + 7 daily + 4 weekly snapshots. Older snapshots are deleted by the `snapshot-retention` background job.

**Recovery procedure:**
1. Stop the server
2. Copy snapshot file to the server's snapshot directory
3. Call `POST /api/v1/admin/restore {snapshot_id}` (or set `GYRE_RESTORE_SNAPSHOT` env var on startup)
4. Restart

For zero-downtime restore on PostgreSQL, use standby promotion instead.

### 2. Health Checks / Liveness Probes

Three levels of health endpoint:

| Endpoint | Checks | Use |
|---|---|---|
| `GET /health` | Process alive, basic response | Load balancer liveness probe |
| `GET /healthz` | DB connectivity, background jobs alive | Kubernetes liveness probe |
| `GET /readyz` | DB connectivity, migration state, merge processor running | Kubernetes readiness probe |

`/healthz` and `/readyz` return a structured `{status, checks}` JSON:

```json
{
  "status": "ok",
  "checks": {
    "database": "ok",
    "merge_processor": "ok",
    "stale_agent_detector": "ok",
    "snapshot_retention": "ok",
    "spawn_budget_reset": "ok"
  }
}
```

Status is `"ok"` only when all checks pass. Any failing check returns HTTP 503.

### 3. Graceful Degradation

**What happens when the Gyre server is unreachable?**

Agents operate in two modes:

**Connected mode (normal):** Agent communicates with server via REST API — heartbeat, log append, status updates, task transitions, git push via Smart HTTP. All state is server-authoritative.

**Disconnected mode (degraded):** When an agent cannot reach the server (network partition, server crash, transient outage):

1. Agent detects connectivity loss via failed heartbeat (3 consecutive failures within 30 s window = disconnected)
2. Agent switches to local-only mode:
   - Continues writing to local git worktree (git operations are local until push)
   - Buffers log lines locally (flushed to server on reconnect)
   - Skips heartbeat (no server to update)
   - Does NOT transition task status (server is authoritative for task state)
3. Agent stores a `disconnected_at` timestamp locally
4. On reconnect:
   - Sends buffered logs in order (oldest first)
   - Sends a single heartbeat with `reconnected: true` flag
   - Resumes normal operations

**Limits of disconnected mode:**
- Agent cannot spawn sub-agents (spawn requires server-issued JWT)
- Agent cannot open MRs (MR creation is server-side)
- Agent cannot complete its task (completion requires server acknowledgment)
- Push via Smart HTTP is unavailable — agent must buffer commits locally

**Maximum disconnected duration:** configurable via `GYRE_AGENT_OFFLINE_TTL` (default: 30 min). After this, the stale agent detector marks the agent as `Dead`. If the agent reconnects after being declared Dead, it receives a `401` and must be respawned.

**Human-facing degradation:** The dashboard shows a "Gyre Connectivity" banner when the WebSocket to the server is lost. REST calls fail with a toast notification. The dashboard caches the last-known agent/task state and shows it as stale.

### 4. Failover

**Single-server (vertical) deployment:**

- Recommended for most teams. One server, SQLite with WAL + periodic snapshots.
- Restart time after crash is < 2 min. This is "good enough" for non-SLA deployments.
- Automated restarts via systemd or Kubernetes `restartPolicy: Always`.

**Why vertical + simplicity is sufficient for most cases:**
- Gyre orchestrates agents; if Gyre is down for 2 min, agents pause. They do not lose work (git worktrees are local to the agent host).
- The bottleneck is usually the LLM API, not Gyre itself. A 2-min Gyre outage causes a 2-min agent pause, not a catastrophic data loss event.
- Complexity of distributed failover is only warranted when: (a) multiple orgs share a Gyre instance with SLAs, or (b) Gyre is on the critical path of a time-sensitive pipeline.

**Multi-server (horizontal) deployment (when warranted):**

- Use PostgreSQL as the backing store (all state in DB, no local SQLite).
- Run 2+ `gyre-server` instances behind a load balancer.
- Sticky sessions are NOT required (the server is stateless beyond the DB and in-memory event bus).
- WebSocket connections are server-affine (a client reconnects to any instance on failure).
- The merge processor and background jobs use DB-level advisory locks to prevent double-execution.
- GitLab-style: the git repos directory (`GYRE_REPOS_PATH`) must be on shared storage (NFS, Ceph, S3-backed FUSE) — or use an external git hosting backend.

**Automated failover is NOT built-in.** Gyre relies on infrastructure-level failover (Kubernetes pod restart, PostgreSQL streaming replication + standby promotion, load balancer health checks). The `GET /readyz` endpoint is the signal for all of these.

### 5. Data Retention Policies

Configurable via `GET/PUT /api/v1/admin/retention`:

| Data Type | Default Retention | Rationale |
|---|---|---|
| Activity events | 90 days | Dashboard history; older events rarely queried |
| Agent logs | 30 days | Debugging; compress after 7 days |
| Audit events | 365 days | Compliance; forward to SIEM for longer retention |
| DB snapshots | 24h×24 + 7d×7 + 4w×4 | See snapshot policy above |
| Merge attestations | Forever | Non-repudiation; git notes survive DB loss |
| Notifications | 90 days (read), 365 days (unread) | Inbox hygiene |
| Analytics events | 365 days | Trend analysis |

Retention jobs run nightly at `02:00 UTC` via the background job scheduler. Jobs are idempotent and safe to run multiple times.

### 6. Export / Migration

Zero vendor lock-in:

```
GET /api/v1/admin/export         → full JSON export of all entities
```

Export includes: projects, repos, tasks, agents, MRs, merge queue, activity, audit events, specs, approvals, notifications. Git repositories are portable as bare clones (standard git format — clone anywhere).

**Migration path:**
1. `GET /api/v1/admin/export` → save JSON
2. Clone all repos (`git clone --mirror`)
3. Spin up new Gyre instance
4. Restore: call `POST /api/v1/admin/restore` with the JSON (or re-import via admin API)
5. Push all repo mirrors to the new instance

No proprietary data formats. SQLite DB is a standard SQLite file. PostgreSQL DB is standard PostgreSQL.

---

## SIEM Integration as Audit Backup

Audit events are forwarded in real time to configured SIEM targets (`POST /api/v1/admin/siem`). This provides:
- Off-box copy of all audit data (survives Gyre host failure)
- Long-term retention beyond Gyre's own retention policy
- Integration with existing security tooling

For compliance-bound deployments, the SIEM is the backup for audit data. Gyre's local audit store is the primary (fast query), SIEM is the durable archive.

---

## Summary

| Question | Answer |
|---|---|
| Target RTO | < 2 min (SQLite restart), < 5 min (PostgreSQL failover), < 30 min (snapshot restore) |
| Target RPO | < 30 s (WAL/streaming replication), < 1 h (snapshot-based) |
| Disconnected agent mode | Yes — agents buffer locally, resume on reconnect; 30-min TTL before declared Dead |
| Automated failover | No (Kubernetes/PostgreSQL infrastructure handles this) |
| SIEM as audit backup | Yes — all audit events forwarded in real time |
