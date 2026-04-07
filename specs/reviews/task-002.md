# Review: TASK-002 — CLI HSI Parity Commands

**Reviewer:** Verifier  
**Date:** 2026-04-07 (R1)  
**Verdict:** needs-revision

---

## Findings

- [-] [process-revision-complete] **F1 — `gyre briefing --workspace` is required; spec says optional.**  
  HSI §11 defines `gyre briefing [--workspace <slug>] [--since <epoch>]` — both parameters in brackets (optional). The implementation declares `workspace: String` (required, not `Option<String>`). The test `cli_briefing_requires_workspace` (main.rs:1238) asserts this error, enshrining spec-contradicting behavior. If no workspace is given, the command should have a default behavior (e.g., list briefings across all workspaces the user has access to, or prompt for selection).  
  **Files:** `crates/gyre-cli/src/main.rs:97-98`, test at line 1238.

- [-] [process-revision-complete] **F2 — `gyre inbox` requires `list` subcommand; spec defines flat command.**  
  HSI §11 shows `gyre inbox [--workspace <slug>] [--priority <min>-<max>]`. The CLI requires `gyre inbox list` to list notifications. Bare `gyre inbox` should list (defaulting to the `list` behavior). `dismiss` and `resolve` subcommands are reasonable additions, but the default/bare invocation must match the spec's flat command form.  
  **Files:** `crates/gyre-cli/src/main.rs:104-106` (subcommand required, no default).

- [-] [process-revision-complete] **F3 — `gyre explore` fails when neither `--repo` nor `--workspace` is provided.**  
  HSI §11 shows `gyre explore <concept> [--repo <name>]` — `--repo` is optional. The client method `get_graph` (client.rs:341) bails with `"either --repo or --workspace is required for explore"`. This is a hard error for a case the spec allows. When neither flag is given, the command should either search globally or return a clear error that explains the spec's intent.  
  **Files:** `crates/gyre-cli/src/client.rs:341`.

- [-] [process-revision-complete] **F4 — `gyre explore --repo` accepts a repo ID; spec says `--repo <name>`.**  
  HSI §11 shows `--repo <name>`, suggesting a human-readable name (analogous to `--workspace <slug>` which is resolved via `resolve_workspace_slug`). The implementation passes `--repo` directly as a URL path segment (a raw UUID/ID). There should be a `resolve_repo_name` step, consistent with the workspace slug resolution pattern.  
  **Files:** `crates/gyre-cli/src/main.rs:113` (help text says "Repository ID"), `crates/gyre-cli/src/client.rs:336-337`.

- [-] [process-revision-complete] **F5 — `gyre spec assist` requires `--repo-id` absent from spec signature.**  
  HSI §11 defines `gyre spec assist <path> "<instruction>"` — two positional arguments, no `--repo-id`. The implementation adds a mandatory `--repo-id` flag (main.rs:234). This deviates from the spec's CLI signature. The repo should either be inferred from the current directory (like `gyre push` infers the remote) or from the spec path.  
  **Files:** `crates/gyre-cli/src/main.rs:233-234`.

- [-] [process-revision-complete] **F6 — `gyre explore --workspace` concept filter is silently ignored by server.**  
  The client sends `GET /api/v1/workspaces/{wid}/graph?concept={concept}` (client.rs:339), but `get_workspace_graph` (graph.rs:724) takes only `Path(id)` — no `Query` parameter. The `?concept=` query string is silently dropped. The result is that `gyre explore <concept> --workspace <slug>` returns *all* graph nodes, not concept-filtered ones. The correct endpoint is `/api/v1/workspaces/{wid}/graph/concept/{concept_name}` (graph.rs:1089).  
  **Files:** `crates/gyre-cli/src/client.rs:339`, `crates/gyre-server/src/api/graph.rs:724-727` vs `graph.rs:1089`.
