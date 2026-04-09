# TASK-025: Spec Links — CLI Commands

**Spec reference:** `spec-links.md` §Querying the Graph (CLI)  
**Depends on:** TASK-019 (query endpoints: dependents, dependencies, stale-links, conflicts)  
**Progress:** `ready-for-review`

## Spec Excerpt

The spec-links spec defines 6 CLI subcommands under `gyre spec`:

```bash
gyre spec links system/identity-security.md    # Show all links for a spec
gyre spec dependents system/source-control.md  # Who depends on this spec?
gyre spec graph                                 # Full graph (text or DOT format)
gyre spec graph --format dot | dot -Tpng -o graph.png  # Visual graph
gyre spec stale-links                           # All stale links
gyre spec conflicts                             # All active conflicts
```

Currently only `gyre spec assist` exists. The 6 subcommands above are unimplemented.

## Implementation Plan

1. **Extend `SpecCommands` enum** in `crates/gyre-cli/src/main.rs`:
   - `Links { path: String }` — calls `GET /api/v1/specs/:path/links`
   - `Dependents { path: String }` — calls `GET /api/v1/specs/:path/dependents` (TASK-019)
   - `Graph { format: Option<String> }` — calls `GET /api/v1/specs/graph`; `--format dot` emits Graphviz DOT
   - `StaleLinks` — calls `GET /api/v1/specs/stale-links` (TASK-019)
   - `Conflicts` — calls `GET /api/v1/specs/conflicts` (TASK-019)

2. **Table formatting** — follow existing CLI patterns (clap, table output consistent with `gyre deps`, `gyre briefing`, etc.)

3. **DOT rendering** — for `gyre spec graph --format dot`:
   - Nodes = specs, labeled with path and approval status
   - Edges colored by link type (implements=blue, depends_on=green, supersedes=gray, conflicts_with=red, extends=orange, references=dotted gray)
   - Stale links highlighted in yellow
   - Output piped to stdout for `dot` consumption

4. **Partial availability** — `links` and `graph` use existing endpoints and can work before TASK-019. `dependents`, `stale-links`, and `conflicts` require TASK-019 endpoints.

## Acceptance Criteria

- [ ] `gyre spec links <path>` displays outbound and inbound links with staleness status
- [ ] `gyre spec dependents <path>` displays all specs that depend on the given spec
- [ ] `gyre spec graph` displays text summary of the tenant-wide spec graph
- [ ] `gyre spec graph --format dot` emits valid Graphviz DOT
- [ ] `gyre spec stale-links` lists all stale links across the tenant
- [ ] `gyre spec conflicts` lists all active conflicts
- [ ] Output formatting consistent with other `gyre` CLI subcommands
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `spec-links.md` §Querying the Graph (CLI section)
3. Read `crates/gyre-cli/src/main.rs` for existing CLI patterns — especially `SpecCommands` and `DepsCommands`
4. Read `crates/gyre-server/src/api/specs.rs` for existing `get_spec_links` and `get_spec_graph` handlers
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(listed after commit)_
