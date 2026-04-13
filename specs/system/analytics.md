# Product Analytics

## Purpose

Built-in analytics (PostHog-like) — not a third-party integration, part of Gyre. Track user behavior, feature usage, funnels, retention. But unlike generic analytics tools, Gyre's analytics are **agent-consumable**: agents query analytics to make autonomous decisions.

The analytics system closes the loop:
1. Agents ship features
2. Analytics measures impact (usage, errors, adoption rate, latency changes)
3. Agents query analytics and decide: promote, rollback, iterate, or escalate

---

## Event Schema

```rust
pub struct AnalyticsEvent {
    pub id: Id,
    pub event_name: String,           // e.g., "task.status_changed", "feature.used"
    pub agent_id: Option<Id>,         // Agent that caused the event (if applicable)
    pub user_id: Option<Id>,          // Human user that caused the event (if applicable)
    pub session_id: Option<String>,   // Session identifier
    pub workspace_id: Option<Id>,
    pub repo_id: Option<Id>,
    pub properties: serde_json::Value, // Arbitrary event properties
    pub timestamp: u64,
}
```

### Auto-Emitted Events

Gyre automatically records analytics events for key platform activities:

| Event Name | Trigger | Properties |
|---|---|---|
| `task.status_changed` | Task transitions status | `old_status`, `new_status`, `task_id`, `assigned_to` |
| `mr.merged` | MR successfully merged | `mr_id`, `repo_id`, `gate_count`, `queue_wait_secs` |
| `mr.closed` | MR closed without merge | `mr_id`, `repo_id`, `reason` |
| `agent.spawned` | Agent spawned | `agent_id`, `task_id`, `compute_target`, `persona` |
| `agent.completed` | Agent completes successfully | `agent_id`, `task_id`, `duration_secs` |
| `agent.failed` | Agent fails or is killed | `agent_id`, `task_id`, `reason` |
| `merge_queue.processed` | Queue entry processed | `mr_id`, `outcome` (merged/failed/skipped), `wait_secs` |
| `gate.failed` | Quality gate fails | `gate_id`, `gate_type`, `mr_id`, `output_snippet` |
| `gate.passed` | Quality gate passes | `gate_id`, `gate_type`, `mr_id`, `duration_secs` |
| `spec.approved` | Spec approved | `spec_path`, `approver_type` (human/agent), `approval_mode` |
| `budget.warning` | Budget threshold crossed | `workspace_id`, `metric` (tokens/cost/agents), `threshold_pct` |
| `search.query` | Full-text search executed | `query_length`, `entity_types`, `result_count`, `duration_ms` |

---

## Query API

```
POST /api/v1/analytics/events      → record event
GET  /api/v1/analytics/events      → query events (?event_name=&agent_id=&since=)
GET  /api/v1/analytics/count       → count events by name (aggregated totals)
GET  /api/v1/analytics/daily       → daily counts (time-series, ?event_name=&days=30)
```

### Query Parameters

| Parameter | Type | Description |
|---|---|---|
| `event_name` | string | Filter by event name (exact or prefix with `*`, e.g., `mr.*`) |
| `agent_id` | UUID | Filter by agent that caused the event |
| `workspace_id` | UUID | Filter by workspace |
| `repo_id` | UUID | Filter by repository |
| `since` | ISO8601 | Start of time range |
| `until` | ISO8601 | End of time range (default: now) |
| `limit` | integer | Max results (default: 100, max: 10000) |
| `group_by` | string | Aggregate by field: `event_name`, `agent_id`, `workspace_id`, `day` |

---

## Decision API for Agents

Agents use analytics to make autonomous data-driven decisions — should this feature flag be promoted? Did this change improve the merge success rate? Is this agent persona performing well?

### `GET /api/v1/analytics/decide`

Evaluate a named decision based on current analytics data.

**Request:**
```
GET /api/v1/analytics/decide?decision=<name>&context=<json>
```

**Built-in decisions:**

| Decision Name | Question | Returns |
|---|---|---|
| `feature.promote` | Should this feature flag be promoted to all users? | `{promote: bool, confidence: float, reason: string, metrics: {...}}` |
| `feature.rollback` | Should this feature be rolled back? | `{rollback: bool, confidence: float, reason: string, trigger: string}` |
| `gate.effectiveness` | Is this quality gate catching real bugs? | `{effective: bool, false_positive_rate: float, catch_rate: float}` |
| `agent.performance` | Is this agent persona performing within norms? | `{healthy: bool, p50_duration_secs, p99_duration_secs, failure_rate: float}` |
| `budget.headroom` | How much budget headroom does this workspace have? | `{tokens_remaining_pct: float, cost_remaining_pct: float, projected_exhaustion_at: string}` |
| `merge_queue.health` | Is the merge queue healthy or degraded? | `{healthy: bool, avg_wait_secs, failure_rate: float, blocked_count: int}` |

**Example: agent deciding whether to promote a feature flag**

```bash
curl -H "Authorization: Bearer $GYRE_JWT" \
  "https://gyre.example.com/api/v1/analytics/decide?decision=feature.promote&context=%7B%22flag%22%3A%22new-diff-viewer%22%2C%22baseline_days%22%3A7%7D"
```

Response:
```json
{
  "decision": "feature.promote",
  "promote": true,
  "confidence": 0.87,
  "reason": "Adoption rate 68% (target: 50%), no error rate increase, p99 latency unchanged",
  "metrics": {
    "adoption_rate": 0.68,
    "error_rate_delta": -0.002,
    "p99_latency_delta_ms": 12,
    "sample_size": 1420
  },
  "evaluated_at": "2026-03-22T18:00:00Z"
}
```

**Example: agent deciding whether to roll back after a bad deploy**

```json
GET /api/v1/analytics/decide?decision=feature.rollback&context={"flag":"optimistic-merge","window_mins":30}

Response:
{
  "decision": "feature.rollback",
  "rollback": true,
  "confidence": 0.95,
  "reason": "Gate failure rate increased from 2% to 34% in last 30 min after flag enabled",
  "trigger": "gate.failure_rate_spike",
  "metrics": {
    "before_rate": 0.02,
    "after_rate": 0.34,
    "affected_mrs": 12
  }
}
```

### `POST /api/v1/analytics/decide/custom`

Agents can define and evaluate custom decisions using a simple rule DSL:

```json
{
  "name": "spec-approval-pace",
  "description": "Are specs being approved fast enough to unblock the merge queue?",
  "rules": [
    {
      "metric": "spec.approved",
      "window": "7d",
      "threshold": 3,
      "comparison": "gte",
      "then": "approve"
    },
    {
      "metric": "merge_queue.processed",
      "filter": {"outcome": "failed"},
      "window": "24h",
      "threshold": 5,
      "comparison": "gte",
      "then": "escalate"
    }
  ],
  "default": "hold",
  "context": {}
}
```

Response:
```json
{
  "decision": "spec-approval-pace",
  "outcome": "hold",
  "matched_rule": null,
  "reason": "Only 2 specs approved in last 7 days (threshold: 3), no queue failures yet",
  "evaluated_at": "2026-03-22T18:00:00Z"
}
```

---

## MCP Tool: `gyre_analytics_decide`

Agents can invoke the decision API via the MCP server at `/mcp`:

```json
{
  "tool": "gyre_analytics_decide",
  "arguments": {
    "decision": "feature.promote",
    "context": {
      "flag": "new-diff-viewer",
      "baseline_days": 7
    }
  }
}
```

This is the primary way agent code invokes analytics decisions without constructing raw HTTP calls. The MCP tool is included in the `tools/list` response.

---

## Analytics Dashboard (UI)

The Analytics view in the Svelte dashboard (sidebar: "Analytics" under Operations):

- **Event counts bar chart** — daily event volume for the last 30 days
- **Recent events list** — paginated, filterable by event name, agent, workspace
- **Decision panel** — interactive decision evaluator (run any built-in decision against live data)
- **Funnel view** — define a funnel (spawn → implement → complete → merge) and see drop-off rates
- **Top agents** — ranked by events generated, task completion rate, average duration

---

## Integration with the Ralph Loop

Analytics close the feedback loop for autonomous development:

1. **Workspace orchestrator** creates delegation tasks to implement a feature
2. **Implementation agent** ships the code, opens MR
3. **MR merges** → `mr.merged` event recorded
4. **Analytics tracks adoption** over the next N days
5. **Repo orchestrator** (or dedicated review agent) calls `GET /api/v1/analytics/decide?decision=feature.promote`
6. Agent either promotes the flag (removing the feature flag toggle from code) or rolls back (reverts the MR)

This replaces human "did the metrics look good?" review cycles with a quantified, auditable decision backed by real data.

---

## Data Retention

Analytics events are retained for 365 days by default (configurable via `PUT /api/v1/admin/retention`). After retention expiry, events are deleted (not archived — analytics is not an audit trail; use the audit system for compliance).
