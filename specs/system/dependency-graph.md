# Cross-Repo Dependency Graph

## Problem

At enterprise scale, repos don't exist in isolation. Repo A imports a crate from Repo B. Repo C calls an API defined by Repo D. Repo E and F both depend on a shared schema in Repo G. These dependencies are invisible to the forge today - discovered only when something breaks.

Without a first-class dependency graph:
- Breaking changes in repo B silently break repos A, C, and F
- Nobody knows the blast radius of a change before merging
- Teams upgrade dependencies on their own schedule (or never), causing version drift
- Cross-repo impact analysis requires manual investigation
- The Workspace Orchestrator has no data to make cross-repo prioritization decisions

## Solution

Gyre maintains a **tenant-wide dependency graph** across all repos, detected automatically and enforced mechanically. The forge sees every push across every repo - it uses this visibility to build, maintain, and enforce the graph.

## Dependency Types

| Type | Detected From | Example |
|---|---|---|
| `code` | `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml` | Repo A depends on crate from Repo B |
| `spec` | `specs/manifest.yaml` links | Repo A's spec `depends_on` Repo B's spec |
| `api` | OpenAPI specs, gRPC proto files, MCP tool definitions | Repo A calls Repo B's REST API |
| `schema` | Shared schema references, protobuf imports | Repo A and B both use schema from Repo C |
| `manual` | Explicitly declared via API/CLI | Dependencies that can't be auto-detected |

## Dependency Entity

```rust
pub struct DependencyEdge {
    pub id: Id,
    pub source_repo_id: Id,
    pub target_repo_id: Id,
    pub dependency_type: DependencyType,
    pub source_artifact: String,      // "Cargo.toml", "specs/manifest.yaml", "api/openapi.yaml"
    pub target_artifact: String,      // crate name, spec path, API endpoint
    pub version_pinned: Option<String>, // "1.2.3", "^2.0", "latest"
    pub target_version_current: Option<String>, // current version in target repo
    pub version_drift: Option<u32>,   // how many versions behind
    pub detection_method: DetectionMethod,
    pub detected_at: u64,
    pub last_verified_at: u64,
    pub status: DependencyStatus,
}

pub enum DependencyType {
    Code,
    Spec,
    Api,
    Schema,
    Manual,
}

pub enum DetectionMethod {
    CargoToml,
    PackageJson,
    GoMod,
    ManifestLink,
    OpenApiRef,
    ProtoImport,
    McpToolRef,
    Manual,
}

pub enum DependencyStatus {
    Active,       // Dependency is current
    Stale,        // Source pins an old version of target
    Breaking,     // Target has published a breaking change source hasn't adopted
    Orphaned,     // Target repo was deleted or archived
}
```

## Automatic Detection

On every push to any repo, the forge:

### 1. Parse Dependency Files

```
Cargo.toml       -> extract [dependencies] with git/path references to other Gyre repos
package.json     -> extract dependencies pointing to Gyre-hosted packages
go.mod           -> extract require directives referencing Gyre modules
pyproject.toml   -> extract dependencies referencing Gyre packages
```

The forge matches dependency identifiers against known repos in the tenant. External dependencies (crates.io, npm) are ignored - only Gyre-to-Gyre dependencies are tracked.

### 2. Parse Spec Links

From `specs/manifest.yaml`, extract all cross-repo spec links (already covered in `spec-links.md`). These become `spec` type edges in the dependency graph.

### 3. Parse API Contracts

```
openapi.yaml / swagger.json  -> extract server URLs matching other Gyre repos
*.proto                       -> extract import paths matching other Gyre repos
mcp.json                     -> extract tool references to other repos' MCP servers
```

### 4. Reconcile

Compare detected dependencies against the existing graph:
- New dependency detected -> create edge, log activity event
- Existing dependency no longer detected -> mark as orphaned (don't delete - might be a detection gap)
- Version changed -> update edge, check for drift

## Breaking Change Detection

When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):

```
1. Forge detects breaking change in Repo B

2. Query dependency graph: "What repos depend on Repo B?"
   Result: [Repo A, Repo C, Repo F]

3. For each dependent repo:
   a. Create a task:
      title: "Breaking change in {repo_b}: {change_description}"
      labels: ["dependency-update", "breaking-change", "auto-created"]
      priority: High

   b. Notify the dependent repo's orchestrator via MCP

   c. If policy requires: BLOCK Repo B's merge until dependent repos
      acknowledge the breaking change
```

### Enforcement Policies

Configurable per-workspace:

```yaml
# workspace dependency policy
dependency_policy:
  breaking_change_behavior: block | warn | notify
  max_version_drift: 3              # flag repos >3 versions behind
  stale_dependency_alert_days: 30   # flag deps not updated in 30 days
  require_cascade_tests: true       # run dependent repo tests on breaking changes
  auto_create_update_tasks: true    # auto-file tasks for dependency updates
```

| Policy | Effect |
|---|---|
| `block` | Breaking change in Repo B cannot merge until all dependent repos acknowledge. Strongest enforcement. |
| `warn` | Breaking change merges with warnings. Tasks auto-created in dependent repos. |
| `notify` | Breaking change merges silently. Dependent repo orchestrators notified. |

### Cascade Testing

When `require_cascade_tests` is enabled and Repo B merges a change:

1. Forge identifies all repos that depend on Repo B
2. For each dependent repo, the forge triggers a test run against Repo B's new version
3. Results are reported in the activity feed
4. If any dependent repo's tests fail, a task is auto-created with the failure details
5. The Workspace Orchestrator is notified of the cascade failure

This is like a cross-repo CI matrix, but forge-native. No external CI configuration needed - the forge knows the graph and runs the tests.

## Version Drift Tracking

The forge continuously monitors version drift:

```
Repo A pins repo-b-crate = "1.2.3"
Repo B is currently at v1.5.0

Drift = 3 minor versions (1.2.3 -> 1.5.0)
```

When drift exceeds `max_version_drift`:
- Task auto-created: "Update repo-b dependency from 1.2.3 to 1.5.0"
- The Repo Orchestrator can dispatch an agent to update the dependency, run tests, and open an MR
- The AIBOM flags stale dependencies in release reports

## Impact Analysis

Before merging a change, the Workspace Orchestrator (or a human) can query:

```
"If I merge this change to Repo B, what is the blast radius?"

Response:
  Direct dependents: Repo A, Repo C, Repo F
  Transitive dependents: Repo G (depends on Repo A), Repo H (depends on Repo C)
  Total blast radius: 5 repos

  Breaking: yes (semver major)
  Cascade test results: not yet run

  Dependent repo health:
    Repo A: pinned at v1.4.0 (1 version behind), tests passing
    Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown
    Repo F: pinned at v1.5.0 (current), tests passing
```

## API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/deps/graph` | GET | Full tenant-wide dependency graph |
| `GET /api/v1/deps/graph?workspace={id}` | GET | Workspace-scoped graph |
| `GET /api/v1/deps/repo/{id}/dependents` | GET | What depends on this repo? |
| `GET /api/v1/deps/repo/{id}/dependencies` | GET | What does this repo depend on? |
| `GET /api/v1/deps/repo/{id}/impact` | GET | Full blast radius analysis |
| `GET /api/v1/deps/stale` | GET | All stale dependencies across tenant |
| `GET /api/v1/deps/breaking` | GET | All unacknowledged breaking changes |
| `POST /api/v1/deps/manual` | POST | Declare a manual dependency edge |
| `DELETE /api/v1/deps/{id}` | DELETE | Remove a dependency edge (manual only) |
| `POST /api/v1/deps/breaking/{id}/acknowledge` | POST | Acknowledge a breaking change |

## CLI

```bash
gyre deps show                              # This repo's dependencies and dependents
gyre deps show --workspace                  # Workspace-wide graph (text)
gyre deps show --tenant                     # Tenant-wide graph (text)
gyre deps graph --format dot                # DOT format for visualization
gyre deps impact repo-b                     # Blast radius for repo-b
gyre deps stale                             # All stale dependencies
gyre deps breaking                          # Unacknowledged breaking changes
gyre deps add --target repo-b --type api    # Manual dependency declaration
gyre deps acknowledge breaking-123          # Acknowledge breaking change
```

## UI

- **Dependency graph visualization:** interactive DAG at workspace and tenant level. Nodes = repos, edges = dependencies colored by type. Stale edges in yellow, breaking in red. Click a node to see impact analysis.
- **Repo detail page:** dependency section showing what this repo depends on and what depends on it. Version drift indicators. Breaking change alerts.
- **Workspace dashboard:** aggregate dependency health. "3 repos have stale dependencies. 1 breaking change unacknowledged."
- **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.

## Workspace Orchestrator Integration

The dependency graph is a primary input for the Workspace Orchestrator:

- **Priority decisions:** if Repo B has a breaking change and 5 repos depend on it, updating those 5 repos becomes high priority
- **Cross-repo task creation:** the orchestrator uses the graph to know which repos need tasks when a dependency changes
- **Merge ordering:** cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically.
- **Budget allocation:** repos with many dependents may warrant higher agent budgets (their changes have higher impact)

## Relationship to Existing Specs

- **Merge Dependencies** (`merge-dependencies.md`): the dependency graph can auto-create cross-repo MR dependencies. Code dependencies feed the MR dependency system.
- **Spec Links** (`spec-links.md`): spec-type dependencies are a subset of the full dependency graph. Both are maintained in the forge's tenant-wide graph.
- **Platform Model** (`platform-model.md`): the Workspace Orchestrator uses the dependency graph for cross-repo coordination. Cascade testing uses the budget system.
- **Spec Lifecycle** (`spec-lifecycle.md`): breaking changes in a dependency trigger drift-review tasks in dependent repos.
- **Supply Chain** (`supply-chain.md`): the AIBOM includes the dependency graph snapshot at release time. Stale dependencies are flagged as supply chain risk.
- **Versioning** (`ci-docs-release.md`): conventional commits + semver are how the forge detects breaking changes. The `feat!:` or `BREAKING CHANGE:` footer triggers the cascade.
