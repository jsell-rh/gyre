---
title: "Implement root-level health probes (/health, /healthz, /readyz)"
spec_ref: "business-continuity.md §2.2"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "business-continuity.md §2.2 Health Checks / Liveness Probes"
commits: []
---

## Spec Excerpt

From `business-continuity.md` §2 — BCP Primitives:

> ### 2. Health Checks / Liveness Probes
>
> Three levels of health endpoint:
>
> | Endpoint | Checks | Use |
> |---|---|---|
> | `GET /health` | Process alive, basic response | Load balancer liveness probe |
> | `GET /healthz` | DB connectivity, background jobs alive | Kubernetes liveness probe |
> | `GET /readyz` | DB connectivity, migration state, merge processor running | Kubernetes readiness probe |
>
> `/healthz` and `/readyz` return a structured `{status, checks}` JSON.
> Status is `"ok"` only when all checks pass. Any failing check returns HTTP 503.

## Implementation Plan

1. **Add 3 root-level health endpoints** (`crates/gyre-server/src/api/mod.rs`):
   - Register `GET /health`, `GET /healthz`, `GET /readyz` — these must be **unauthenticated** (outside the auth middleware layer) since they're used by load balancers and Kubernetes probes

2. **Implement `/health`** (minimal liveness):
   - Return `200 OK` with `{"status": "ok"}` — no database checks, no dependencies
   - This endpoint proves the process is alive and accepting HTTP connections

3. **Implement `/healthz`** (deep liveness):
   - Check database connectivity (execute a simple query like `SELECT 1`)
   - Check that background jobs are alive (query `job_registry` for job status — any job that hasn't run within 2× its interval is "unhealthy")
   - Return `{"status": "ok", "checks": {"database": "ok", "merge_processor": "ok", "stale_agent_detector": "ok", ...}}`
   - Return HTTP 503 if any check fails, with the failing check's status set to `"error"`

4. **Implement `/readyz`** (readiness):
   - All `/healthz` checks plus:
   - Migration state: verify all migrations have been applied (Diesel's `diesel_migrations::MigrationHarness`)
   - Merge processor running: confirm the merge processor background job has run recently
   - Return same structured JSON format
   - Return HTTP 503 if not ready (e.g., during startup before migrations complete)

5. **Tests**:
   - Test `/health` always returns 200
   - Test `/healthz` returns 200 when DB is connected, 503 when not
   - Test `/readyz` returns 200 when fully ready
   - Test response JSON structure matches spec

## Acceptance Criteria
- [x] `GET /health` returns 200 with `{"status": "ok"}` — no auth required
- [x] `GET /healthz` returns 200 when DB and background jobs are healthy, 503 otherwise
- [x] `GET /readyz` returns 200 when fully ready (DB + migrations + merge processor), 503 otherwise
- [x] `/healthz` and `/readyz` return structured `{status, checks}` JSON per spec
- [x] All three endpoints are unauthenticated (accessible without bearer token)
- [x] Tests cover healthy and unhealthy states

## Implementation Notes

- The three routes already existed at the router root (outside the auth
  layer) in `lib.rs::build_router` — but the handlers were hardcoded
  always-200 fakes. This task replaced them with real evaluation.
- `database` check: `StoragePort::health_check()` (a real `SELECT 1`
  round-trip) through a new `AppState.storage` handle typed
  `Option<Arc<dyn StoragePort>>`, wired from whichever backend is
  configured (`db_storage` was SQLite-only and `None` under PG).
  Pure in-memory mode (no `GYRE_DATABASE_URL`) reports
  `"not_configured"` and passes — there is no DB to be down.
- `migrations` check: new `StoragePort::migrations_pending()` (implemented
  on BOTH adapters) — real diff of embedded migrations vs.
  `MigrationHarness::applied_migrations()` on a pooled connection.
- Job liveness: `JobRegistry` gained a `scheduled` set (set by `spawn_job`
  and by the `merge_processor` / `stale_agent_detector` module loops) and
  per-cycle `record_cycle` entries from those loops — previously the loops
  recorded nothing, so job liveness could not be evaluated at all.
  `liveness()` = last run within 2× interval (interval jobs) or
  interval + 1 h slack (wall-clock daily jobs); never-run scheduled jobs
  are `pending` (pass) until 2× interval since process start, then
  `stale` (fail).
- Check sets follow the spec TABLE (the contract): `/healthz` =
  `database` + every scheduled job, with required loops checked even when
  registration or scheduling failed (`merge_processor`,
  `stale_agent_detector`, `retention_cleanup`, `spawn_budget_reset`);
  `/readyz` = `database`,
  `migrations`, `merge_processor`. The plan's "all /healthz checks plus"
  is subsumed by the table's narrower readyz column.
- Spec example-JSON keys `snapshot_retention` / `spawn_budget_reset`:
  snapshot retention runs inside the scheduled `retention_cleanup` job
  (covered under that check name); the budget-reset loop now records real
  cycles under `spawn_budget_reset`. A failed recent cycle fails liveness.
- `GET /api/v1/admin/health` unchanged (admin stats, not infra probes).

## Agent Instructions

- Read `crates/gyre-server/src/api/mod.rs` for route registration — note the auth middleware layers; these health routes must be registered OUTSIDE the auth middleware (look for the pattern used by other unauthenticated routes)
- Read `crates/gyre-server/src/api/admin.rs` for `admin_health` (line ~33-55) as a reference for health response patterns, but note this is an admin-only endpoint — the new endpoints are unauthenticated
- Read `crates/gyre-server/src/jobs.rs` for the job registry and how to query job health
- The existing `GET /api/v1/admin/health` remains unchanged — it provides admin-level system stats (agent count, repo count, etc.), which is different from infrastructure health probes
