# Coverage: Business Continuity

**Spec:** [`system/business-continuity.md`](../../system/business-continuity.md)
**Last audited:** 2026-04-13 (full audit — verified against admin.rs, snapshot.rs, retention.rs, siem.rs, stale_agents.rs, jobs.rs)
**Coverage:** 6/7 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Recovery Objectives | 2 | n/a | - | RTO/RPO targets and rationale — no implementable requirement. |
| 2 | BCP Primitives | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | 1. Snapshot / Restore | 3 | implemented | - | 4 admin endpoints: POST /api/v1/admin/snapshot (create), GET /snapshots (list), POST /restore, DELETE /snapshots/{id}. snapshot.rs: VACUUM INTO for SQLite, SnapshotMeta with snapshot_id/timestamp/size. GYRE_SNAPSHOT_PATH configurable. BCP drill endpoint (admin.rs:679). |
| 4 | 2. Health Checks / Liveness Probes | 3 | task-assigned | task-151 | Only admin-level health exists (GET /api/v1/admin/health — behind auth). Spec requires 3 unauthenticated root-level probes: /health (liveness), /healthz (deep liveness with DB + job checks), /readyz (readiness with migration state). |
| 5 | 3. Graceful Degradation | 3 | implemented | - | DisconnectedBehavior enum (Pause/ContinueOffline/Abort) in gyre-domain agent.rs. stale_agents.rs honors per-agent disconnected_behavior setting: abort terminates, pause stops, continue_offline leaves running. Heartbeat timeout detection. Agent status lifecycle. |
| 6 | 4. Failover | 3 | n/a | - | Architecture guidance — recommends infrastructure-level failover (Kubernetes restartPolicy, PostgreSQL standby promotion, load balancer health checks). Explicitly states "Automated failover is NOT built-in." No application code required. |
| 7 | 5. Data Retention Policies | 3 | implemented | - | retention.rs with configurable policies: activity 90d, analytics 365d, cost 365d. GET/PUT /api/v1/admin/retention endpoints. Background retention job in jobs.rs. TelemetryBuffer ring-buffer auto-eviction. |
| 8 | 6. Export / Migration | 3 | implemented | - | GET /api/v1/admin/export returns full JSON export of all entities. ABAC-gated (admin resource). Git repos portable as standard bare clones. No proprietary data formats. |
| 9 | SIEM Integration as Audit Backup | 2 | implemented | - | Full CRUD: POST/GET/PUT/DELETE /api/v1/admin/siem. siem.rs: Syslog (RFC 5424) + webhook targets. JSON + CEF output formats. Background forwarder every 10s. Off-box audit data copy. |
| 10 | Summary | 2 | n/a | - | Summary table — no implementable requirement. |
