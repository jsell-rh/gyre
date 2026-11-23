---
title: "Agent self-observability: durable per-agent logs, metrics, and traces with real query surfaces (REST + MCP)"
spec_ref: "Spec gap spec-8f4edcf188b12e2a350050e2 / agent-telemetry-query / fd38a2fca0f8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-experience.md §Observability for Agents: "Agents query their own logs, metrics, and traces (LogQL, PromQL, TraceQL or equivalent)" and "Agents can reason about performance, errors, and behavior from instrumentation - not just code." Current state (verified at base 87377fe1): agent logs are a plain limit/offset tail over an in-memory AgentLogBuffer (crates/gyre-server/src/log_compression.rs, wired at lib.rs:2147) that a restart orphans — no filtering, search, or time range; GET /metrics (lib.rs:1014) exposes only server-process Prometheus totals with no per-agent dimension; server OTel spans export to an external operator-side collector (default http://localhost:4317) with no in-repo storage or query surface; the only agent-reachable trace data is the MR-scoped gate trace (GET /api/v1/merge-requests/:id/trace, MCP trace://{mr_id}); no MCP tool queries logs, metrics, or traces. Deliver: (1) Durable agent logs: a port-backed agent-log store (gyre-ports trait + mem and SQLite adapters via the existing store! wiring pattern; every adapter implementing the port enforces the trait's failure constraints per check-mem-port-contracts.sh; new migration uses the next unused 6-digit sequence number and portable SQL per check-migration-sql-portability.sh) that the existing append/get/stream handlers read and write — lines survive a server restart against durable storage, and the unparameterized query preserves today's limit/offset response shape. Never add the durable state as a bare Arc<Mutex<HashMap>> (check-in-memory-state-stores.sh). The store records structured lines (timestamp, level, message); the append API gains an optional level field (absent keeps today's raw-message behavior; legacy unclassified lines still return). (2) Structured log query ("LogQL or equivalent"): GET /api/v1/agents/:id/logs gains real query parameters — substring search and since/until time range (plus the level field) — executed in the repository against stored lines, not post-fetch truncation of a fixed tail. (3) Per-agent metrics ("PromQL or equivalent"): record real per-agent runtime series from events the platform already observes (heartbeats, gate runs with pass/fail outcomes, error signals, completions) in an agent-dimensioned store, and expose a per-agent metrics query returning named series with values; /metrics operator totals remain unchanged. (4) Agent-scoped traces ("TraceQL or equivalent"): capture per-agent runtime spans (session phases from spawn through heartbeat, gate run, and completion, with status and duration) into durable per-agent trace storage, queryable via GET /api/v1/agents/:id/trace with filtering by operation/status/time range — distinct from the existing MR-scoped gate trace. (5) MCP tools (e.g. gyre_query_logs, gyre_query_metrics, gyre_query_traces): read-only, NOT in the needs_write RBAC gate, returning the same data as the REST queries; any new route gets a RouteResourceMapping resolver entry (no exemption-file entries, no duplicates). Authorization scoping matches the REST routes: a caller sees telemetry only for agents within its tenant/workspace scope. (6) The "reason about performance, errors, and behavior" half is carried by the data: series and spans carry status/error/duration attributes derived from real recorded events, never hardcoded values standing in for measured ones.

## Acceptance Criteria

- [ ] After appending log lines and rebuilding AppState from the same durable database (restart equivalence), GET /api/v1/agents/:id/logs returns the exact lines; on current code the in-memory buffer returns none.
- [ ] With lines "ERROR boom" and "INFO ok" stored, a filtered query (level/substring) returns only the matching line, and a time-range query excludes lines outside the range; the unfiltered query returns today's shape.
- [ ] The per-agent metrics query returns series derived from recorded events: a heartbeat increments the heartbeat series, a failed gate run increments the failure series — values change with real events, and /metrics process totals are unchanged.
- [ ] GET /api/v1/agents/:id/trace returns spans with operation, status, and duration for the agent's runtime phases; a status=error filter returns only error spans.
- [ ] The MCP query tools are callable by an Agent-role principal for its own telemetry, a caller outside the telemetry's tenant/workspace scope is denied, and a tool call creates zero durable rows (row counts identical before/after).
- [ ] check-arch.sh, check-mem-port-contracts.sh, check-migration-versions.sh, check-migration-sql-portability.sh, check-in-memory-state-stores.sh, check-abac-route-registry.sh, and check-abac-exempt-handlers.sh all pass; no new exemption entries.

## Verification intent

Durability arm: POST /api/v1/agents/:id/logs with "ERROR probe-line", then recreate AppState against the same SQLite file and GET the logs — on current code the AgentLogBuffer is in-memory and the response is empty, so the probe fails before the task; after it, the line returns from storage. Query arm: store "ERROR boom" and "INFO ok" and request with the error filter — on current code LogsQuery (api/agent_logs.rs) has only limit/offset, so the filter parameter does not exist and cannot narrow the result; after the task only the error line returns. Metrics arm: query the per-agent metrics for an agent that has sent heartbeats — on current code metrics.rs contains only process-level gauges/counters with no agent dimension, so no per-agent heartbeat count can be returned; after the task it must. Trace arm: GET /api/v1/agents/:id/trace — on current code no such route exists (only /merge-requests/:id/trace), so the probe 404s; after the task it returns the agent's own spans.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-experience.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`.

[
  {
    "id": "c5b26c825b5cd17bd8cd4b61",
    "decision": "gap",
    "requirement_quote": "- Agents query their own **logs, metrics, and traces** (LogQL, PromQL, TraceQL or equivalent).\n- Agents can reason about performance, errors, and behavior from instrumentation - not just code.",
    "detail": "No query language or equivalent exists for agents over their own telemetry. (1) Logs: GET /api/v1/agents/:id/logs is a plain limit/offset tail (default 100, max 1000) over an in-memory AgentLogBuffer that a restart orphans — no filtering, search, time range, or query language; the reference agent runner only ever appends logs, never queries them. (2) Metrics: /metrics (registered on the root router, outside the auth-layered api_router) exposes server-process Prometheus text (http request totals, active agents, merge queue depth) as an operator scrape endpoint — no per-agent metrics dimension and no PromQL. (3) Traces: server OTel spans export to an external operator-side collector (default http://localhost:4317); no collector, storage, or query surface ships in-repo. The only real trace data an agent can reach is the gate-run trace for an MR (GET /api/v1/merge-requests/:id/trace and the MCP trace://{mr_id} resource), which covers test-command spans, not the agent's own runtime behavior. No MCP tool exists for querying logs, metrics, or traces. The closest realized behavior for 'reason about performance, errors, and behavior from instrumentation' is gate-trace inspection and gate-failure feedback, which is MR-scoped, not agent-self-scoped.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/agent_logs.rs",
        "quote": "let limit = params.limit.unwrap_or(100).min(1000);",
        "detail": "The entire agent log query surface is a limit/offset tail returning Vec<String> lines. Combined with crates/gyre-server/src/lib.rs:368 'pub agent_logs: Arc<log_compression::AgentLogBuffer>,' (in-memory buffer), this is a write-mostly plain tail, not a LogQL-or-equivalent query capability; the reference runner (docker/gyre-agent/agent-runner.mjs) only POSTs logs."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": ".route(\"/metrics\", get(metrics::metrics_handler))",
        "detail": "The metrics endpoint is registered on the root router (the auth/ABAC layers apply to api::api_router() only) and returns server-process Prometheus text — an operator scrape surface with no per-agent dimension and no query language. Repo-wide search for promql/logql/traceql/loki/tempo/grafana finds zero query-language implementations."
      },
      {
        "file": "crates/gyre-server/src/telemetry.rs",
        "quote": "Exports spans via OTLP/gRPC to the collector at `OTEL_EXPORTER_OTLP_ENDPOINT`",
        "detail": "Server spans export to an external operator-side collector ('The server works fine without a collector'); no collector, trace storage, or agent-facing trace query surface ships in the repository, so agents cannot query their own runtime traces."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"description\": \"SDLC system trace for a merge request (HSI §3a). URI: trace://{mr_id}\",",
        "detail": "The only trace access exposed to agents is this MR gate-run trace resource (spans captured during a gate's test command). It is real instrumentation an agent can reason over, but it is scoped to MR gate runs — not the agent's own logs, metrics, or runtime traces — and the MCP tool catalog contains no log/metrics/trace query tool."
      }
    ]
  }
]
