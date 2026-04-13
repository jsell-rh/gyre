# Coverage: Product Analytics

**Spec:** [`system/analytics.md`](../../system/analytics.md)
**Last audited:** 2026-04-13
**Coverage:** 0/12

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Purpose | 2 | task-assigned | task-146 | Context/rationale — analytics system partially exists (event recording + query endpoints) but auto-emitted events incomplete. |
| 2 | Event Schema | 2 | task-assigned | task-146 | AnalyticsEvent struct exists but may be missing spec-required fields (user_id, session_id, workspace_id, repo_id). |
| 3 | Auto-Emitted Events | 3 | task-assigned | task-146 | Partial — task.status_changed, agent.spawned, mr.merged, merge_queue.processed exist. Missing: mr.closed, agent.completed, agent.failed, gate.failed, gate.passed, spec.approved, budget.warning, search.query. |
| 4 | Query API | 2 | task-assigned | task-146 | Implemented — POST/GET /api/v1/analytics/events, GET /count, GET /daily exist. Need to verify all spec-required filter parameters. |
| 5 | Query Parameters | 3 | task-assigned | task-146 | Partially implemented — event_name, since, until, limit filtering exists. Need to verify workspace_id, repo_id, agent_id, group_by. |
| 6 | Decision API for Agents | 2 | task-assigned | task-147 | Not started — no /api/v1/analytics/decide endpoint exists. |
| 7 | `GET /api/v1/analytics/decide` | 3 | task-assigned | task-147 | Not started — no built-in decision evaluators. |
| 8 | `POST /api/v1/analytics/decide/custom` | 3 | task-assigned | task-147 | Not started — no custom decision rule DSL. |
| 9 | MCP Tool: `gyre_analytics_decide` | 2 | task-assigned | task-148 | Not started — gyre_analytics_query exists but gyre_analytics_decide does not. |
| 10 | Analytics Dashboard (UI) | 2 | task-assigned | task-149 | Not started — no analytics UI components exist in web/src/. |
| 11 | Integration with the Ralph Loop | 2 | task-assigned | task-148 | Not started — conceptual pattern, enabled by decision API + MCP tool. |
| 12 | Data Retention | 2 | task-assigned | task-149 | Not started — no retention endpoint or cleanup job. |
