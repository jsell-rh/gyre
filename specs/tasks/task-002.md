---
title: "CLI HSI Parity Commands"
spec_ref: "human-system-interface.md §11 (CLI/MCP Parity Constraint)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-002.md
coverage_sections: []
commits: ["e970c9155c1d21c397baf35788e8b3531f1554b9"]
---

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

## Shipped

The six HSI parity commands (`gyre briefing`, `gyre inbox` with bare-list + `dismiss`/`resolve`, `gyre explore`, `gyre trace`, `gyre spec assist`, `gyre divergence`) are implemented and present at this branch's base via the merged PR #627 work, carrying every R1–R13 review fix: spec-signature conformance (optional `--workspace`, flat `gyre inbox`, git-remote repo inference for `spec assist`/`explore`), the `/merge-requests/:id/trace` endpoint with GateTrace formatting, `notification_type=ConflictingInterpretations` server-side filtering, JSON body on inbox resolve POST, SSE event-type discrimination in `spec_assist`, `body`/`entity_ref` field names in divergence output, and full `BriefingItem`/`completed_agents` field rendering (all 6 fields, `format_timestamp`, `cross_workspace`).

### This assignment's repair

The task's R13 verification gate `scripts/check-cli-spec-parity.sh` failed on the clean base tree: Check 10 compared the display loop's *nested* diff-op accesses (`d["op"]`, `d["path"]`, `d["content"]`) against the spec-assist complete event's *top-level* field set (`{diff, error, explanation}`): a category error producing six false positives that blocked the pre-commit gate. Repaired in commit `e970c9155` by scoping the check to receivers that actually hold complete-event values (the call's assignment variable and its loop variable), the same precision-repair pattern as task-099's Check 7 fix. Detection strength preserved via mutation test: reintroducing the original task-012 R2 F6 flaw (`op["text"]`) still fails the check with a precise finding.

### Test evidence

- `cargo test -p gyre-cli`: 149 unit + 1 integration tests pass, including all 18 HSI command tests (`cli_briefing_*`, `cli_inbox_*`, `cli_explore_*`, `cli_trace_parses`, `cli_spec_assist_*`, `cli_divergence_*`).
- `scripts/check-cli-spec-parity.sh`: passes on clean tree (exit 0); fails correctly on mutation (exit 1).
- `scripts/check-response-consumption.sh`: passes (exit 0).
- Evidence and mutation logs saved under `/tmp/stage/review-evidence/`.
