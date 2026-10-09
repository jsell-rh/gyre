# Coverage: Product Analytics

**Spec:** [`system/analytics.md`](../../system/analytics.md)
**Last audited:** 2026-10-09
**Coverage:** 5/12 (0 not-started, 7 task-assigned, 5 implemented)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Purpose | 2 | implemented | task-146 | Context/rationale — analytics system (event recording + query endpoints + all 12 auto-emitted events) now real; no implementable requirement beyond the sections below. |
| 2 | Event Schema | 2 | implemented | task-146 | All 9 spec fields present on `AnalyticsEvent` (gyre-domain/src/analytics.rs); `with_scope` populates user/session/workspace/repo; serde round-trip test pins the shape. Id-typed fields stored as text (Option<String>), matching the storage layer's TEXT columns. |
| 3 | Auto-Emitted Events | 3 | implemented | task-146 | All 12 events emit at real trigger points with all spec properties, each with a passing per-event test (see task-146.md Shipped). Multi-path triggers covered: agent.failed (fail/admin-kill/stale-abort), mr.closed (transition/repo-delete/spec-reject), mr.merged (HTTP transition + queue merge). |
| 4 | Query API | 2 | implemented | task-146 | POST/GET /api/v1/analytics/events, GET /count, GET /daily all real against SQLite/Postgres/mem adapters; ISO8601 or unix-sec since/until; daily supports ?event_name=&days= per spec. |
| 5 | Query Parameters | 3 | implemented | task-146 | event_name (incl. trailing-`*` prefix via LIKE on both SQLite and Postgres), agent_id, user_id, workspace_id, repo_id, since, until, limit (default 100, max 10_000), group_by (event_name/agent_id/workspace_id/day, unsupported fields rejected) — all implemented and adapter-tested (analytics_query_filtered_all_params). |
| 6 | Decision API for Agents | 2 | task-assigned | task-147 | Not started — no /api/v1/analytics/decide endpoint exists. |
| 7 | `GET /api/v1/analytics/decide` | 3 | task-assigned | task-147 | Not started — no built-in decision evaluators. |
| 8 | `POST /api/v1/analytics/decide/custom` | 3 | task-assigned | task-147 | Not started — no custom decision rule DSL. |
| 9 | MCP Tool: `gyre_analytics_decide` | 2 | task-assigned | task-148 | Not started — gyre_analytics_query exists but gyre_analytics_decide does not. |
| 10 | Analytics Dashboard (UI) | 2 | task-assigned | task-149 | Not started — no analytics UI components exist in web/src/. |
| 11 | Integration with the Ralph Loop | 2 | task-assigned | task-148 | Not started — conceptual pattern, enabled by decision API + MCP tool. |
| 12 | Data Retention | 2 | task-assigned | task-149 | Not started — no retention endpoint or cleanup job. |
