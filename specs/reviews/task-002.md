# Review: TASK-002 — CLI HSI Parity Commands

**Reviewer:** Verifier  
**Date:** 2026-04-07 (R9)  
**Verdict:** needs-revision

---

## R1 Findings (all addressed)

- [x] [process-revision-complete] **F1 — `gyre briefing --workspace` is required; spec says optional.**  
  HSI §11 defines `gyre briefing [--workspace <slug>] [--since <epoch>]` — both parameters in brackets (optional). The implementation declares `workspace: String` (required, not `Option<String>`). The test `cli_briefing_requires_workspace` (main.rs:1238) asserts this error, enshrining spec-contradicting behavior. If no workspace is given, the command should have a default behavior (e.g., list briefings across all workspaces the user has access to, or prompt for selection).  
  **Files:** `crates/gyre-cli/src/main.rs:97-98`, test at line 1238.

- [x] [process-revision-complete] **F2 — `gyre inbox` requires `list` subcommand; spec defines flat command.**  
  HSI §11 shows `gyre inbox [--workspace <slug>] [--priority <min>-<max>]`. The CLI requires `gyre inbox list` to list notifications. Bare `gyre inbox` should list (defaulting to the `list` behavior). `dismiss` and `resolve` subcommands are reasonable additions, but the default/bare invocation must match the spec's flat command form.  
  **Files:** `crates/gyre-cli/src/main.rs:104-106` (subcommand required, no default).

- [x] [process-revision-complete] **F3 — `gyre explore` fails when neither `--repo` nor `--workspace` is provided.**  
  HSI §11 shows `gyre explore <concept> [--repo <name>]` — `--repo` is optional. The client method `get_graph` (client.rs:341) bails with `"either --repo or --workspace is required for explore"`. This is a hard error for a case the spec allows. When neither flag is given, the command should either search globally or return a clear error that explains the spec's intent.  
  **Files:** `crates/gyre-cli/src/client.rs:341`.

- [x] [process-revision-complete] **F4 — `gyre explore --repo` accepts a repo ID; spec says `--repo <name>`.**  
  HSI §11 shows `--repo <name>`, suggesting a human-readable name (analogous to `--workspace <slug>` which is resolved via `resolve_workspace_slug`). The implementation passes `--repo` directly as a URL path segment (a raw UUID/ID). There should be a `resolve_repo_name` step, consistent with the workspace slug resolution pattern.  
  **Files:** `crates/gyre-cli/src/main.rs:113` (help text says "Repository ID"), `crates/gyre-cli/src/client.rs:336-337`.

- [x] [process-revision-complete] **F5 — `gyre spec assist` requires `--repo-id` absent from spec signature.**  
  HSI §11 defines `gyre spec assist <path> "<instruction>"` — two positional arguments, no `--repo-id`. The implementation adds a mandatory `--repo-id` flag (main.rs:234). This deviates from the spec's CLI signature. The repo should either be inferred from the current directory (like `gyre push` infers the remote) or from the spec path.  
  **Files:** `crates/gyre-cli/src/main.rs:233-234`.

- [x] [process-revision-complete] **F6 — `gyre explore --workspace` concept filter is silently ignored by server.**  
  The client sends `GET /api/v1/workspaces/{wid}/graph?concept={concept}` (client.rs:339), but `get_workspace_graph` (graph.rs:724) takes only `Path(id)` — no `Query` parameter. The `?concept=` query string is silently dropped. The result is that `gyre explore <concept> --workspace <slug>` returns *all* graph nodes, not concept-filtered ones. The correct endpoint is `/api/v1/workspaces/{wid}/graph/concept/{concept_name}` (graph.rs:1089).  
  **Files:** `crates/gyre-cli/src/client.rs:339`, `crates/gyre-server/src/api/graph.rs:724-727` vs `graph.rs:1089`.

## R2 Findings

- [x] [process-revision-complete] **F7 — `gyre divergence` filter silently dropped: `notification_type` not in server's `NotificationParams`.**  
  The `gyre divergence` command calls `get_notifications(..., Some("ConflictingInterpretations"))` (main.rs:848), which sends `notification_type=ConflictingInterpretations` as a query parameter to `GET /api/v1/users/me/notifications` (client.rs:294). However, the server's `NotificationParams` struct (users.rs:246–252) does not include a `notification_type` field — it only has `workspace_id`, `min_priority`, `max_priority`, `limit`, and `offset`. The `notification_type` query parameter is **silently ignored** by the Axum `Query<NotificationParams>` extractor. As a result, `gyre divergence` returns **all** notifications regardless of type, not just `ConflictingInterpretations` alerts. Fix: add `notification_type: Option<String>` to `NotificationParams` and filter by it in `list_for_user` (or add a new port method with a type filter).  
  **Files:** `crates/gyre-cli/src/client.rs:294`, `crates/gyre-server/src/api/users.rs:246-252`.

- [x] [process-revision-complete] **F8 — `gyre trace` calls `/timeline` endpoint; spec requires `/trace`.**  
  HSI §11 defines `gyre trace <mr-id>` with MCP equivalent `Resource: trace://<mr-id>`. The spec at line 484 and line 557 of `human-system-interface.md` explicitly states: "`GET /api/v1/merge-requests/:id/trace` REST endpoint (returns the `GateTrace` struct as JSON for CLI/MCP consumption per §11)". The CLI calls `/api/v1/merge-requests/{mr_id}/timeline` instead (client.rs:429), which returns SDLC activity events — a different data shape entirely. The output formatter (main.rs:737–758) also expects the timeline response format (an `events` array) rather than the `GateTrace` format (fields: `spans`, `root_spans`, `service_graph`, `commit_sha`, `gate_run_id`). Both the endpoint URL and the output formatter must be updated.  
  **Files:** `crates/gyre-cli/src/client.rs:429` (wrong endpoint), `crates/gyre-cli/src/main.rs:737-758` (wrong response format).

- [x] [process-revision-complete] **F9 — `gyre explore --repo <name>` without `--workspace` hard-fails; spec allows it.**  
  HSI §11 defines `gyre explore <concept> [--repo <name>]` — `--repo` is optional and standalone; the spec signature has no `--workspace` parameter. The implementation (main.rs:667–672) bails with `"--repo requires --workspace"` when `--repo` is given without `--workspace`. The `spec assist` command (main.rs:776–800) already demonstrates git-remote inference when explicit flags are omitted — `explore` should use the same pattern. When `--repo` is given without `--workspace`, either infer the workspace from the git remote or search all workspaces for a repo matching the name.  
  **Files:** `crates/gyre-cli/src/main.rs:667-672`.

## R3 Findings

- [x] [process-revision-complete] **F10 — `gyre inbox resolve <id>` sends no JSON body; server requires `Json<ResolveRequest>`.**  
  The client method `resolve_notification` (client.rs:327–344) sends a POST request with no body and no `Content-Type: application/json` header. The server handler `resolve_notification` (users.rs:371–394) uses `Json(req): Json<ResolveRequest>` as a required Axum extractor. Axum's `Json` extractor rejects requests without a valid JSON body with a 400 or 415 status. As a result, `gyre inbox resolve <id>` will **fail at runtime every invocation**. Fix: the client must send an empty JSON object body (e.g., `.json(&serde_json::json!({}))`) to satisfy the extractor, since `ResolveRequest.action_taken` is `Option<String>` and will default to `None`.  
  **Files:** `crates/gyre-cli/src/client.rs:327-344` (missing `.json(...)` call), `crates/gyre-server/src/api/users.rs:371-375` (requires `Json<ResolveRequest>`).

## R4 Findings

- [x] [process-revision-complete] **F11 — `gyre trace` help text says "SDLC timeline"; spec says "System Trace" and data is GateTrace.**  
  The `Trace` command definition (main.rs:125) has help text `/// Show SDLC timeline for a merge request`. HSI §11 names this surface "System Trace", and since the R2 fix (F8), the command calls `/api/v1/merge-requests/:id/trace` returning `GateTrace` data (spans, durations, statuses) — not an SDLC timeline of activity events. The help text is stale from before the R2 endpoint correction. Fix: change the doc comment to `/// Show system trace for a merge request` (or similar wording consistent with the spec's "System Trace" naming).  
  **Files:** `crates/gyre-cli/src/main.rs:125`.

- [x] [process-revision-complete] **F12 — `spec_assist` SSE parser collects all `data:` lines; should extract only the `complete` event.**  
  The `spec_assist` client method (client.rs:471–483) parses the SSE response by collecting every `data:`-prefixed line into the result vec. Per `ui-layout.md` §3, the SSE stream contains two event types: `event: partial` (incremental explanation text chunks: `{"text": "..."}`) and `event: complete` (final response with `{diff, explanation}`). The parser does not track the current `event:` type — it collects ALL data lines indiscriminately. When the server sends N partial events + 1 complete event, the client returns N+1 entries, all treated as diff ops. Only the `complete` event's data should be extracted as the diff op list. Fix: track the current event type as lines are parsed; only push data from `event: complete` lines into the result vec. Partial data can be printed to stderr for progress indication if desired.  
  **Files:** `crates/gyre-cli/src/client.rs:471-483` (SSE parser), `crates/gyre-cli/src/main.rs:853-865` (display loop treats all entries as diff ops).

## R5 Findings

- [x] [process-revision-complete] **F13 — `spec_assist` display code expects DiffOp fields (`op`, `path`, `content`) but server's `complete` event sends `{"text": "..."}`.**  
  The server's spec-assist `complete` event carries `{"text": "<full_text>"}` (specs_assist.rs:170-172). The R4 fix (F12) correctly made the SSE parser extract only `complete` events. But the display code (main.rs:853-864) indexes each collected value with `op["op"]`, `op["path"]`, `op["content"]` — fields that do not exist in the `{"text": "..."}` payload. All three resolve to `None`, producing `Op: unknown`, empty path, and empty content. The actual LLM response text (in `op["text"]`) is silently discarded. `gyre spec assist` shows empty/misleading results every invocation despite receiving a valid server response. Fix: parse the `complete` event's `{"text": "..."}` shape and display the text content. When TASK-012 changes the shape to `{diff, explanation}`, the display code should be updated to match — but today it must handle today's shape.  
  **Files:** `crates/gyre-cli/src/main.rs:853-864` (display code), `crates/gyre-server/src/api/specs_assist.rs:170-172` (server response shape).

- [x] [process-revision-complete] **F14 — `divergence` display reads `n["description"]` but server sends `n["body"]`.**  
  The divergence alert display code (main.rs:897) reads `n["description"].as_str()`, but the server's `NotificationResponse` struct (users.rs:255-268) serializes the field as `body` (no `#[serde(rename)]`). The JSON key is `"body"`, not `"description"`. As a result, the description line in divergence alerts is always empty — even when the notification has body text. Fix: change `n["description"]` to `n["body"]`.  
  **Files:** `crates/gyre-cli/src/main.rs:897` (wrong field name), `crates/gyre-server/src/api/users.rs:262` (field is `body`).

## R6 Findings

- [x] [process-revision-complete] **F15 — `briefing` display reads `item["agent_name"]` but server's `BriefingItem` has no such field.**  
  The `print_briefing` function (main.rs:987, 1004) reads `item["agent_name"].as_str()` for items in both the `completed` and `in_progress` arrays. The server's `BriefingItem` struct (graph.rs:260-267) has fields: `title`, `description`, `entity_type`, `entity_id`, `spec_path`, `timestamp` — no `agent_name`. The `as_str()` call returns `None`, `unwrap_or("")` gives `""`, and the display branch `println!("  - {title} [{agent}]")` is dead code — agent attribution is silently absent in all briefing output. The `in_progress` items come from tasks that have an `assigned_to` field (mapped from `Task.assigned_to`), and the server populates a separate `completed_agents` array with `agent_id` per HSI §4, but neither data source is surfaced by the CLI. Fix: remove the dead `agent_name` access, or map the task's `assigned_to` to a display name and use `completed_agents` for the completed section.  
  **Files:** `crates/gyre-cli/src/main.rs:987,1004` (reads nonexistent field), `crates/gyre-server/src/api/graph.rs:260-267` (`BriefingItem` struct).

- [x] [process-revision-complete] **F16 — `spec_assist` doc comment says "DiffOps" but R5 fix changed the data to `{"text": "..."}` payloads.**  
  The doc comment on `spec_assist` (client.rs:444) reads `"SSE stream → collected DiffOps"`. The R5 fix (F13) changed the display code to handle `{"text": "..."}` payloads from the server's `complete` event (specs_assist.rs:170-172). The function returns `Vec<serde_json::Value>` containing `{"text": "..."}` objects, not DiffOps. This is a stale reference from before the R4/R5 corrections. Fix: update the doc comment to `"SSE stream → collected text payloads"` or similar.  
  **Files:** `crates/gyre-cli/src/client.rs:444` (stale doc comment).

## R7 Findings

- [-] [process-revision-complete] **F17 — `print_briefing` omits the `cross_workspace` section entirely.**  
  HSI §9 (lines 1270-1273) explicitly defines a "CROSS-WORKSPACE" section in the briefing narrative showing cross-workspace spec link changes: `"↔ platform-core updated idempotent-api.md / Your payment-retry.md depends on it."` The server's `BriefingResponse` (graph.rs:250) includes `cross_workspace: Vec<BriefingItem>`. The CLI's `print_briefing` function (main.rs:973-1028) handles `completed`, `in_progress`, `exceptions`, and `metrics` — but has no code to display `cross_workspace` items. Cross-workspace spec dependency notifications are silently dropped by the CLI, violating HSI §11's principle that "every data surface in the UI must be consumable outside the browser." Fix: add a `cross_workspace` section to `print_briefing`, rendering items between the "In Progress" and "Exceptions" sections, consistent with the spec §9 ordering.  
  **Files:** `crates/gyre-cli/src/main.rs:973-1028` (missing section), `crates/gyre-server/src/api/graph.rs:250` (server sends `cross_workspace`).

- [-] [process-revision-complete] **F18 — Briefing items display only `title`, silently dropping `description`, `spec_path`, and other fields.**  
  The spec §9 (lines 1253-1283) shows each briefing item with detailed content — descriptions ("cargo test failed (3 tests). Agent retried once, still failing."), spec references ("spec: payment-retry.md"), and agent attribution. The server's `BriefingItem` (graph.rs:260-267) includes `description: String`, `spec_path: Option<String>`, `entity_type: String`, `entity_id: Option<String>`, and `timestamp: u64`. The CLI renders only `item["title"]` for all sections (main.rs:986, 998, 1010), discarding every other field. For exceptions, the user sees only a title like "Gate failure: billing-service MR #47" with no indication of what failed or how to respond. For completed items, there is no spec reference or description of what was accomplished. HSI §11 requires CLI parity with the UI — the UI displays these fields. Fix: render `description` below the title when non-empty, append `(spec: {spec_path})` when present, and include `timestamp` for ordering context. The display should match the spec §9 structure at minimum.  
  **Files:** `crates/gyre-cli/src/main.rs:982-1015` (all section rendering loops).

## R8 Findings

- [x] **F17 (resolved R9).** `print_briefing` now renders the `cross_workspace` section (main.rs:1003-1011) with `↔` prefix via `print_briefing_item`. Fixed in commit `902643db`.

- [x] **F18 (resolved R9).** All section rendering loops now call `print_briefing_item` (main.rs:1039-1052), which renders `title`, `description`, and `spec_path`. Fixed in commit `902643db`.

## R9 Findings

- [-] [process-revision-complete] **F19 — `print_briefing` silently drops the `completed_agents` section of the BriefingResponse.**  
  The server's `BriefingResponse` (graph.rs:244-257) includes `completed_agents: Vec<BriefingCompletedAgent>` (graph.rs:234-241), which carries per-agent completion data: `agent_id`, `spec_ref`, `decisions` (with reasoning), `uncertainties`, `conversation_sha`, and `completed_at`. HSI §9's COMPLETED section explicitly shows agent attribution and decision reasoning: "Agent: worker-12. Decision: used exponential backoff (confidence: high)". HSI §9's data source table lists "agent completion summaries" as a named data source for the Completed section. HSI §11 requires: "every data surface in the UI must be consumable outside the browser." The CLI's `print_briefing` function (main.rs:973-1036) renders `completed`, `in_progress`, `cross_workspace`, `exceptions`, and `metrics` — but has no code path for `completed_agents`. Agent decisions and uncertainties are silently dropped. This is the same flaw class as F17 (incomplete response consumption) — a composite response section is present in the server response but absent from the CLI rendering. Fix: add a "Completed Agents" subsection (or merge agent data inline with completed items by matching `spec_ref`) to render agent_id, decisions, and uncertainties.  
  **Files:** `crates/gyre-cli/src/main.rs:973-1036` (missing section), `crates/gyre-server/src/api/graph.rs:234-257` (server sends `completed_agents`).
