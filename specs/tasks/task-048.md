# TASK-048: Cross-Repo Dependency Auto-Detection Pipeline

**Spec reference:** `dependency-graph.md` §Automatic Detection
**Depends on:** None (builds on M22.4 base dep-graph infrastructure — done)
**Progress:** `not-started`

## Spec Excerpt

From `dependency-graph.md` §Automatic Detection:

> **On every push**, the forge scans changed files for dependency declarations:
>
> 1. **Package managers:** Parse `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml` for cross-repo references.
> 2. **Spec links:** Parse `specs/manifest.yaml` for `depends_on`, `implements`, `extends` links targeting other repos.
> 3. **API contracts:** Parse `openapi.yaml`, `swagger.json`, `*.proto`, `mcp.json` for service-to-service dependencies.
>
> **Reconciliation:** Compare detected edges against existing graph. Create new edges for newly detected dependencies. Mark edges as `Orphaned` when their source declaration disappears. Update `version_pinned` when declared versions change.

## Current State

Only Cargo.toml path deps are auto-detected on push (`crates/gyre-server/src/git_http.rs:1536–1700`). The function `detect_cargo_path_deps` parses `[dependencies]` entries with `path = "../..."` and creates `DependencyEdge` records with `DetectionMethod::CargoToml`.

Missing entirely:
- `package.json` workspace/file references
- `go.mod` `replace` directives pointing to local paths
- `pyproject.toml` path dependencies
- Spec manifest link extraction (cross-repo `@workspace/repo/path` links)
- API contract parsing (openapi, proto, mcp)
- Reconciliation logic (orphan detection, version updates)

The `DependencyType` enum already has all needed variants (`Code`, `Spec`, `Api`, `Schema`, `Manual`). The `DetectionMethod` enum currently has: `CargoToml`, `PackageJson`, `GoMod`, `ManifestLink`, `OpenApiRef`, `ProtoImport`, `McpToolRef`, `Manual`. Note: there is no `PyprojectToml` variant yet — add it as part of this task. Use `ManifestLink` (not "ManifestYaml") for spec manifest detection, and `OpenApiRef`/`ProtoImport`/`McpToolRef` (not a single "ApiContract") for API contract detection.

## Implementation Plan

1. **Package manager parsers** (in `git_http.rs` alongside existing `detect_cargo_path_deps`):
   - `detect_package_json_deps(content: &str) -> Vec<(String, Option<String>)>` — parse `dependencies`, `devDependencies` for `file:` or `workspace:` references; extract version for non-local deps
   - `detect_go_mod_deps(content: &str) -> Vec<(String, Option<String>)>` — parse `replace` directives with local paths; extract module versions
   - `detect_pyproject_deps(content: &str) -> Vec<(String, Option<String>)>` — parse `[tool.poetry.dependencies]` or `[project.dependencies]` for path references

2. **Spec manifest link extraction:**
   - Parse `specs/manifest.yaml` for entries with cross-repo links (`@workspace/repo/path` format)
   - Create `DependencyEdge` with `DependencyType::Spec` and `DetectionMethod::ManifestLink`

3. **API contract detection:**
   - Detect presence of `openapi.yaml`/`swagger.json` — create `Api` dependency edges to repos whose services are referenced
   - Detect `*.proto` imports referencing other repos
   - This is best-effort: many API contracts don't explicitly reference repo names, so detection is limited to conventions (e.g., `import "other-service/v1/types.proto"`)

4. **Reconciliation logic** (new function `reconcile_dependencies`):
   - After all detection runs, compare detected edges against existing edges for the pushed repo
   - New edges: create with `DependencyStatus::Active`
   - Missing edges (detected before, not detected now): mark as `DependencyStatus::Orphaned`
   - Changed versions: update `version_pinned` field
   - Never touch `Manual` edges — those are user-managed

5. **Integration:**
   - Call all detectors in the post-push handler (after existing `detect_cargo_path_deps` call)
   - Run reconciliation after all detectors complete
   - Log detection results for observability

6. **Tests:**
   - Unit tests for each parser function
   - Integration test for reconciliation (create → detect new → detect orphaned)
   - Test that `Manual` edges are not affected by reconciliation

## Acceptance Criteria

- [ ] `package.json` `file:` and `workspace:` references detected on push, creating `DependencyEdge` records
- [ ] `go.mod` `replace` directives with local paths detected on push
- [ ] `pyproject.toml` path dependencies detected on push
- [ ] Spec manifest cross-repo links create `Spec` type dependency edges
- [ ] Reconciliation marks disappeared deps as `Orphaned`
- [ ] Reconciliation updates `version_pinned` when declared version changes
- [ ] `Manual` dependency edges are never modified by auto-detection
- [ ] Unit tests for each parser function
- [ ] Integration test for reconciliation lifecycle
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/git_http.rs` — search for `detect_cargo_path_deps` to find the existing detection code and the post-push hook where detection runs
3. Read `crates/gyre-domain/src/dependency.rs` (or wherever `DependencyEdge`, `DependencyType`, `DetectionMethod` are defined) for the domain types
4. Read `crates/gyre-ports/src/dependency.rs` for the `DependencyRepository` port trait
5. Add new parser functions alongside `detect_cargo_path_deps`
6. Add reconciliation logic
7. Wire new detectors into the post-push handler
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
