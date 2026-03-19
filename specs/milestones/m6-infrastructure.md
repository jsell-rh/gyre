# Milestone 6: Infrastructure & Operations

Final infrastructure pieces. After M6, Gyre has product analytics, business continuity primitives, cost tracking, and the foundation for remote compute provisioning.

## Deliverables

### 1. Product Analytics

Built-in PostHog-like analytics, agent-consumable:

- **Event tracking** — track user/agent actions (feature usage, API calls, task completion rates)
- **Analytics domain** — AnalyticsEvent entity, time-series storage
- **Query API** — `GET /api/v1/analytics/events` with aggregation (count, rate, funnel)
- **Agent-consumable** — agents can query analytics to make decisions
- **Dashboard** — analytics page with charts (task completion rate, agent throughput, MR merge time)

### 2. Business Continuity

BCP primitives:

- **Snapshot/restore** — `POST /api/v1/admin/snapshot` creates point-in-time DB backup
- **Restore** — `POST /api/v1/admin/restore` restores from snapshot
- **Health probes** — standardized liveness + readiness endpoints
- **Data retention** — configurable retention policies per data type
- **Export** — `GET /api/v1/admin/export` exports all data as JSON

### 3. Cost Tracking

Track LLM/compute costs per agent/task:

- **CostEntry domain** — agent_id, task_id, cost_type (llm_tokens, compute_minutes), amount, timestamp
- **Record costs** — `POST /api/v1/costs` records a cost entry
- **Query costs** — `GET /api/v1/costs?agent_id=&task_id=&since=` with aggregation
- **Budget alerts** — when agent exceeds lifetime_budget, notify via activity event
- **Dashboard** — cost breakdown by agent, task, time period

### 4. Background Job Framework

Formalize the background job system:

- **Job registry** — register named jobs with schedule (cron-like or interval)
- **Job history** — track every run (start, end, success/failure, duration)
- **Job management API** — list jobs, trigger manual run, view history
- **Existing jobs** — migrate merge processor + stale agent detector to framework

### 5. Dashboard Finalization

- Analytics charts (use simple SVG/canvas, no heavy charting library)
- Cost breakdown views
- Snapshot/restore controls in admin
- Job history table with logs

## Success Criteria

- Analytics events tracked and queryable
- Snapshot/restore works for SQLite DB
- Cost tracking per agent/task
- Background jobs have proper history and management
- 400+ tests

## Dependencies

- M5 Agent Protocols (complete)
