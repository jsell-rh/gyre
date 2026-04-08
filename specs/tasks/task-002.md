# TASK-002: CLI HSI Parity Commands

**Spec reference:** `human-system-interface.md` §11 (CLI/MCP Parity Constraint)  
**Depends on:** None (consumes existing REST endpoints)  
**Progress:** `complete`  
**Review:** [specs/reviews/task-002.md](../reviews/task-002.md)

## Spec Excerpt

From `human-system-interface.md` §11:

> **Every data surface in the UI must be consumable outside the browser.** If understanding requires the Gyre platform UI to be running, we've created the same lock-in that killed Dark's proprietary editor.
>
> | UI Surface | CLI Equivalent | MCP Equivalent |
> |---|---|---|
> | Briefing | `gyre briefing [--workspace <slug>] [--since <epoch>]` | Resource: `briefing://` |
> | Inbox | `gyre inbox [--workspace <slug>] [--priority <min>-<max>]` | Resource: `notifications://` |
> | Explorer (concept search) | `gyre explore <concept> [--repo <name>]` | Tool: `graph.concept` |
> | System Trace | `gyre trace <mr-id>` | Resource: `trace://<mr-id>` |
> | Spec editing assist | `gyre spec assist <path> "<instruction>"` | Tool: `spec.assist` |
> | Divergence alerts | `gyre divergence [--workspace <slug>]` | (via notifications) |

## Current State

The CLI (`crates/gyre-cli/src/main.rs`) currently supports:
- `connect`, `ping`, `health`, `tui`, `init`, `clone`, `push`
- `mr` (create, list)
- `tasks` (list, take)
- `status`
- `release` (next-version, changelog)

**Missing:** All six HSI parity commands (`briefing`, `inbox`, `explore`, `trace`, `spec assist`, `divergence`). These consume existing REST endpoints — no server-side changes needed.

## Implementation Plan

1. **`gyre briefing`** — `GET /api/v1/workspaces/:workspace_id/briefing`
   - Args: `--workspace <slug>` (resolve via `GET /api/v1/workspaces?slug=`), `--since <epoch>`, `--repo <name>`
   - Output: formatted narrative (Completed / In Progress / Exceptions / Metrics sections)

2. **`gyre inbox`** — `GET /api/v1/users/me/notifications`
   - Args: `--workspace <slug>`, `--priority <min>-<max>` (e.g., `--priority 1-5`)
   - Output: table of unresolved notifications with priority, type, title, age
   - Sub-commands: `gyre inbox dismiss <id>`, `gyre inbox resolve <id>`

3. **`gyre explore`** — `GET /api/v1/repos/:id/graph/concept/:concept_name` or `GET /api/v1/workspaces/:id/graph/concept/:concept_name`
   - Args: `<concept>` (positional), `--repo <name>`, `--workspace <slug>`
   - Output: table of matching graph nodes with type, name, qualified_name, spec linkage

4. **`gyre trace`** — `GET /api/v1/merge-requests/:id/trace`
   - Args: `<mr-id>` (positional)
   - Output: gate trace data (spans, root_spans, service_graph, commit_sha, gate_run_id)

5. **`gyre spec assist`** — `POST /api/v1/repos/:repo_id/specs/assist`
   - Args: `<path>` (positional), `<instruction>` (positional or `--instruction`)
   - Output: diff suggestions (op, path, content) with accept/reject prompt

6. **`gyre divergence`** — `GET /api/v1/users/me/notifications?notification_type=ConflictingInterpretations`
   - Args: `--workspace <slug>`
   - Output: list of divergence alerts with spec references and agent pairs

All commands require auth (`~/.gyre/config` from `gyre init` or `--token` flag) and should handle workspace slug resolution consistently.

## Acceptance Criteria

- [ ] `gyre briefing` displays workspace briefing narrative
- [ ] `gyre inbox` lists notifications with priority filtering
- [ ] `gyre inbox dismiss <id>` and `gyre inbox resolve <id>` work
- [ ] `gyre explore <concept>` shows matching graph nodes
- [ ] `gyre trace <mr-id>` shows SDLC timeline
- [ ] `gyre spec assist <path> "<instruction>"` shows LLM-suggested diffs
- [ ] `gyre divergence` shows conflicting interpretation alerts
- [ ] All commands support `--workspace <slug>` where applicable
- [ ] Tests for each command (at least argument parsing + output formatting)

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-cli/src/main.rs` for the existing command pattern (clap derive macros)
3. Read `crates/gyre-cli/src/client.rs` for the HTTP client pattern
4. Add new subcommands to the `Commands` enum
5. Implement handlers following the existing pattern (HTTP call → format → print)
6. Add tests following the existing test pattern
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(cli): add 6 HSI parity commands — briefing, inbox, explore, trace, spec assist, divergence`
- `fix(cli): address R1 review findings — spec-signature conformance for all 6 HSI commands`
- `fix(cli): address R2 review findings — notification_type filter, trace endpoint, repo inference`
- `fix(cli): address R3 finding — send JSON body for inbox resolve POST`
- `fix(cli): address R4 findings — stale help text, SSE event-type discrimination`
- `fix(cli): address R5 findings — response field name/shape mismatch`
- `fix(cli): address R6 findings — dead agent_name field access + stale doc comment`
