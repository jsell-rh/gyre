# Spec Links & Mechanical Gates

## Problem

At enterprise scale (hundreds of repos, many teams), specs form a massive graph. Without machine-readable links between specs, you get:

- Specs that contradict each other with no one knowing
- Specs superseded but still referenced by active code
- Circular dependencies between specs that nobody can trace
- Teams implementing against a spec that another team already replaced
- The Accountability agent drowning in false positives because it can't tell which spec version is authoritative

The current system has informal cross-references (prose like "see `agent-gates.md`") but no machine-readable links. Prose references don't survive renames, don't cascade invalidation, and can't be mechanically enforced.

## Link Types

Every link is SHA-pinned. Not "this spec relates to identity-security.md" but "this spec relates to identity-security.md@abc123." When the linked spec changes, the link becomes stale and the forge flags it.

| Link Type | Meaning | Forge Enforcement |
|---|---|---|
| `implements` | This spec is a concrete plan for a higher-level spec | Implementation spec can't be approved until parent is approved |
| `supersedes` | This spec replaces another spec | Old spec marked deprecated in registry. Code referencing old spec gets flagged. |
| `depends_on` | This spec can't be implemented until the linked spec is implemented | Implementation tasks blocked until dependency spec's implementation is complete |
| `conflicts_with` | These specs have overlapping or contradictory requirements | Neither spec can be approved simultaneously. Conflict must be resolved first. |
| `extends` | This spec adds requirements on top of the linked spec | Changes to the parent spec trigger drift-review in the extending spec |
| `references` | Informational link (no enforcement) | No mechanical enforcement. For human/agent navigation only. |

## Manifest Integration

Links are declared in the spec's manifest entry:

```yaml
- path: system/platform-model.md
  title: Platform Model
  owner: user:jsell
  approval:
    mode: human_and_agent
    human_approvers:
      - user:jsell
    agent_approvers:
      - persona: accountability
  links:
    - type: supersedes
      target: development/manager-agent.md
      target_sha: abc123
      reason: "Split into workspace + repo orchestrators"

    - type: supersedes
      target: personas/workspace-orchestrator.md
      target_sha: def456
      reason: "Evolved into workspace-orchestrator persona"

    - type: depends_on
      target: system/source-control.md
      target_sha: ghi789

    - type: extends
      target: system/identity-security.md
      target_sha: jkl012
      reason: "Adds tenant_id, workspace_id to OIDC claims"

    - type: conflicts_with
      target: system/old-auth-model.md
      target_sha: mno345
      reason: "Incompatible permission model"
```

## Cross-Repo and Cross-Workspace Links

At enterprise scale, specs in different repos and workspaces reference each other. A platform team's spec might be `depended_on` by 30 application team specs.

Cross-repo links use a fully qualified target with an `@` prefix for disambiguation:

```yaml
links:
  - type: depends_on
    target: "@platform-team/core-services/system/api-contract.md"
    target_sha: abc123
    # Format: @{workspace_slug}/{repo_name}/{spec_path}
    # The @ prefix distinguishes cross-repo paths from same-repo spec paths.
    # {workspace_slug} is the workspace's URL-safe slug (not display name).
    # Same-workspace cross-repo: @{repo_name}/{spec_path} (no workspace prefix)
    # Same-repo: just the spec_path (no @ prefix)
```

The forge resolves cross-repo links by querying the tenant-wide spec graph. The link is validated:
- Target workspace exists and the linking user has read access
- Target repo exists within that workspace
- Target spec path exists in the target repo's manifest
- Target SHA matches (or is flagged as stale if the spec has moved on)

## Forge-Maintained Spec Graph

The forge maintains a tenant-wide directed graph of all spec links:

```sql
CREATE TABLE spec_links (
    id              TEXT PRIMARY KEY,
    source_repo_id  TEXT NOT NULL,
    source_path     TEXT NOT NULL,
    source_sha      TEXT NOT NULL,
    link_type       TEXT NOT NULL,
    target_repo_id  TEXT,               -- NULL for unresolved cross-workspace links (resolved later by staleness checker)
    target_path     TEXT NOT NULL,
    target_sha      TEXT NOT NULL,
    target_display  TEXT,               -- human-readable composite path (e.g., "@platform-core/api-svc/system/auth.md")
    reason          TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      INTEGER NOT NULL,
    stale_since     INTEGER
);
```

### Link Status

| Status | Meaning |
|---|---|
| `active` | Both source and target SHAs are current |
| `stale` | Target spec has changed since the link was created. Source may need updating. |
| `broken` | Target spec was deleted or is no longer in the manifest |
| `conflicted` | A `conflicts_with` link where both specs are approved (violation) |

### Automatic Staleness Detection

When any spec changes (new SHA), the forge:
1. Queries `spec_links` for all links where `target_path` matches the changed spec
2. Marks those links as `stale` (target_sha no longer matches)
3. Creates drift-review tasks in the source specs' repos
4. For `extends` links: the extending spec may need to be updated
5. For `depends_on` links: implementation work may need to be re-evaluated
6. For `supersedes` links: the superseded spec should already be deprecated

## Mechanical Gates

### Approval Gates

| Link Type | Gate Behavior |
|---|---|
| `implements` | Source spec cannot be approved until target (parent) spec is approved at the pinned SHA |
| `depends_on` | Implementation tasks for source spec are blocked until target spec's implementation tasks are complete |
| `conflicts_with` | Both specs cannot have `approval_status: approved` simultaneously. The forge rejects the second approval and creates a conflict-resolution task. |
| `supersedes` | When source is approved, target is automatically set to `deprecated`. Code referencing target gets flagged. |
| `extends` | When target changes, source's approval is invalidated (it may need to incorporate the parent's changes) |

### Merge Gates

When an MR references a spec (`spec_ref`), the forge also checks that spec's links:

- If the spec `depends_on` another spec that isn't implemented yet, the MR gets a warning (not a block - the code might be valid, but the dependency isn't satisfied yet)
- If the spec is `superseded`, the MR is rejected: "spec {path} has been superseded by {new_path}. Update your spec_ref."
- If the spec has a `conflicts_with` link to an approved spec, the MR is blocked until the conflict is resolved

### Cycle Detection

The forge rejects manifest changes that would create cycles in the spec graph:
- A `depends_on` B `depends_on` C `depends_on` A -> rejected
- A `implements` B `implements` A -> rejected
- `conflicts_with` is bidirectional by nature (not a cycle)
- `references` and `supersedes` are excluded from cycle detection

## Querying the Graph

### API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/specs/graph` | GET | Full tenant-wide spec dependency graph |
| `GET /api/v1/specs/graph?repo={id}` | GET | Specs for one repo with all outbound/inbound links |
| `GET /api/v1/specs/{path}/links` | GET | Links for one spec (both outbound and inbound) |
| `GET /api/v1/specs/{path}/dependents` | GET | "What specs depend on this one?" (impact analysis) |
| `GET /api/v1/specs/{path}/dependencies` | GET | "What does this spec depend on?" |
| `GET /api/v1/specs/stale-links` | GET | All stale links across the tenant |
| `GET /api/v1/specs/conflicts` | GET | All active conflicts |

### CLI

```bash
gyre spec links system/identity-security.md    # Show all links for a spec
gyre spec dependents system/source-control.md  # Who depends on this spec?
gyre spec graph                                 # Full graph (text or DOT format)
gyre spec graph --format dot | dot -Tpng -o graph.png  # Visual graph
gyre spec stale-links                           # All stale links
gyre spec conflicts                             # All active conflicts
```

### UI

- **Spec detail page:** link section showing all outbound/inbound links with staleness indicators
- **Spec graph visualization:** interactive DAG at workspace or tenant level. Nodes colored by approval status. Stale links highlighted in yellow. Conflicts highlighted in red. Click to navigate.
- **Impact analysis view:** select a spec, see all transitive dependents. "If I change this spec, these 14 specs across 6 repos need review."

## Accountability Agent Integration

The Accountability agent's patrol gains spec-graph awareness:

- **Stale links:** flag specs with stale links that haven't been reviewed
- **Orphaned supersessions:** a spec is superseded but code still references it
- **Unresolved conflicts:** two conflicting specs are both approved
- **Dangling implementations:** an `implements` link points to a spec that was deleted
- **Deep dependency chains:** specs with >5 levels of `depends_on` (decomposition smell)

## Relationship to Existing Specs

- **Spec Registry** (`spec-registry.md`): links are declared in the manifest and tracked in the forge ledger. The approval status computation now considers link constraints.
- **Spec Lifecycle** (`spec-lifecycle.md`): spec changes cascade through the link graph, creating drift-review tasks in dependent specs.
- **Platform Model** (`platform-model.md`): cross-workspace links are resolved through the tenant-wide graph. The Workspace Orchestrator uses the graph for impact analysis.
- **Agent Gates** (`agent-gates.md`): MRs referencing superseded specs are rejected. Spec-to-code binding verifies the referenced spec is not deprecated.
