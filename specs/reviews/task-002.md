# Review: TASK-002 — CLI HSI Parity Commands

**Reviewer:** Verifier  
**Date:** 2026-04-07 (R3)  
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

- [ ] **F10 — `gyre inbox resolve <id>` sends no JSON body; server requires `Json<ResolveRequest>`.**  
  The client method `resolve_notification` (client.rs:327–344) sends a POST request with no body and no `Content-Type: application/json` header. The server handler `resolve_notification` (users.rs:371–394) uses `Json(req): Json<ResolveRequest>` as a required Axum extractor. Axum's `Json` extractor rejects requests without a valid JSON body with a 400 or 415 status. As a result, `gyre inbox resolve <id>` will **fail at runtime every invocation**. Fix: the client must send an empty JSON object body (e.g., `.json(&serde_json::json!({}))`) to satisfy the extractor, since `ResolveRequest.action_taken` is `Option<String>` and will default to `None`.  
  **Files:** `crates/gyre-cli/src/client.rs:327-344` (missing `.json(...)` call), `crates/gyre-server/src/api/users.rs:371-375` (requires `Json<ResolveRequest>`).
