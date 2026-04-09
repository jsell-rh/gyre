# TASK-024: Dep Graph — CLI Commands

**Spec reference:** `dependency-graph.md` §CLI  
**Depends on:** TASK-020 (breaking change endpoints), TASK-021 (stale endpoint)  
**Progress:** `complete`  
**Review:** [`specs/reviews/task-024.md`](../reviews/task-024.md)

## Spec Excerpt

From `dependency-graph.md` §CLI:

> ```bash
> gyre deps show                              # This repo's dependencies and dependents
> gyre deps show --workspace                  # Workspace-wide graph (text)
> gyre deps show --tenant                     # Tenant-wide graph (text)
> gyre deps graph --format dot                # DOT format for visualization
> gyre deps impact repo-b                     # Blast radius for repo-b
> gyre deps stale                             # All stale dependencies
> gyre deps breaking                          # Unacknowledged breaking changes
> gyre deps add --target repo-b --type api    # Manual dependency declaration
> gyre deps acknowledge breaking-123          # Acknowledge breaking change
> ```

## Current State

- **No `gyre deps` CLI commands exist.** The CLI (`crates/gyre-cli/src/main.rs`) has no dependency-related subcommand.
- **API endpoints exist** for the base operations:
  - `GET /api/v1/repos/:id/dependencies` (outgoing deps)
  - `GET /api/v1/repos/:id/dependents` (incoming deps)
  - `GET /api/v1/dependencies/graph` (tenant-wide graph)
  - `GET /api/v1/repos/:id/blast-radius` (impact analysis)
  - `POST /api/v1/repos/:id/dependencies` (add manual)
  - `DELETE /api/v1/repos/:id/dependencies/:dep_id` (remove)
- **Missing API endpoints** (TASK-020 and TASK-021 will add these):
  - `GET /api/v1/dependencies/stale` (stale dependencies)
  - `GET /api/v1/dependencies/breaking` (breaking changes)
  - `POST /api/v1/dependencies/breaking/:id/acknowledge` (acknowledge)

## Implementation Plan

1. **Add `deps` subcommand** to `gyre-cli/src/main.rs`:
   - `gyre deps show` — call `GET /repos/:id/dependencies` + `GET /repos/:id/dependents`, display as table
   - `gyre deps show --workspace` — call `GET /dependencies/graph?workspace={id}`, display as table
   - `gyre deps show --tenant` — call `GET /dependencies/graph`, display as table
   - `gyre deps graph --format dot` — call `GET /dependencies/graph`, render as DOT format
   - `gyre deps impact <repo>` — call `GET /repos/:id/blast-radius`, display as tree
   - `gyre deps stale` — call `GET /dependencies/stale`, display as table
   - `gyre deps breaking` — call `GET /dependencies/breaking`, display as table
   - `gyre deps add --target <repo> --type <type>` — call `POST /repos/:id/dependencies`
   - `gyre deps acknowledge <id>` — call `POST /dependencies/breaking/:id/acknowledge`

2. **Follow existing CLI patterns:**
   - Use clap for argument parsing (same as other subcommands)
   - Use the existing HTTP client from the CLI
   - Table output format consistent with `gyre tasks`, `gyre mrs`

3. **DOT format output** for `gyre deps graph --format dot`:
   - Render graph as Graphviz DOT
   - Nodes = repos, edges = dependencies
   - Color edges by type (code=blue, spec=green, api=orange, schema=purple, manual=gray)

4. **Add tests:**
   - Each subcommand produces correct output format
   - `--format dot` produces valid DOT syntax
   - Error handling for missing repo context

## Acceptance Criteria

- [ ] `gyre deps show` displays repo dependencies and dependents
- [ ] `gyre deps show --workspace` displays workspace-scoped graph
- [ ] `gyre deps graph --format dot` produces valid DOT format
- [ ] `gyre deps impact <repo>` displays blast radius
- [ ] `gyre deps stale` lists stale dependencies
- [ ] `gyre deps breaking` lists unacknowledged breaking changes
- [ ] `gyre deps add` creates manual dependency
- [ ] `gyre deps acknowledge` marks breaking change acknowledged
- [ ] Output format consistent with existing CLI commands
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/dependency-graph.md` §CLI
3. Read `crates/gyre-cli/src/main.rs` for existing CLI structure and HTTP client patterns
4. Read `crates/gyre-server/src/api/dependencies.rs` for API response shapes
5. Follow the existing subcommand pattern (e.g., `tasks`, `mrs`, `inbox`)
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `bb7ff9d7` feat(cli): add `gyre deps` subcommand for dependency graph operations (TASK-024)
