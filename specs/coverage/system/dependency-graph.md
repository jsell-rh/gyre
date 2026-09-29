# Coverage: Cross-Repo Dependency Graph

**Spec:** [`system/dependency-graph.md`](../../system/dependency-graph.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 13/17 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Solution | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 3 | Dependency Types | 2 | implemented | - | All 5 types defined: Code, Spec, Api, Schema, Manual (gyre-domain/src/dependency.rs). 8 detection methods: CargoToml, PackageJson, GoMod, PyprojectToml, ManifestLink, OpenApiRef, ProtoImport, McpToolRef, Manual. 4 statuses: Active, Stale, Breaking, Orphaned. |
| 4 | Dependency Entity | 2 | implemented | - | DependencyEdge struct with all spec fields: id, source/target repos, dependency_type, source/target artifacts, version_pinned, target_version_current, version_drift, detection_method, status, detected_at, last_verified_at. DB migration 000007. |
| 5 | Automatic Detection | 2 | implemented | - | detect_dependencies_on_push in git_http.rs:2296 orchestrates detection on every push. |
| 6 | 1. Parse Dependency Files | 3 | implemented | - | detect_cargo_path_deps, extract_cargo_version (Cargo.toml); detect_package_json_deps (package.json); detect_go_mod_deps (go.mod); detect_pyproject_deps (pyproject.toml). All parsers implemented. |
| 7 | 2. Parse Spec Links | 3 | implemented | - | detect_manifest_spec_links parses specs/manifest.yaml for cross-repo spec references. |
| 8 | 3. Parse API Contracts | 3 | implemented | - | detect_openapi_refs (OpenAPI), detect_proto_imports (protobuf), detect_mcp_refs (MCP tool references). |
| 9 | 4. Reconcile | 3 | implemented | - | reconcile_dependencies handles orphaning removed deps, un-orphaning re-added deps, version updates. |
| 10 | Breaking Change Detection | 2 | implemented | - | BreakingChange domain entity with acknowledgment tracking. Detection via conventional commits (feat!:, BREAKING CHANGE: footer). Auto-task creation for dependent repos. |
| 11 | Enforcement Policies | 3 | task-assigned | task-163 | DependencyPolicy model exists (Block/Warn/Notify behaviors) but stored in-memory only (MemDependencyPolicyRepository). No persistent SQLite/Postgres adapter. Policy enforcement at merge time partially connected. |
| 12 | Cascade Testing | 3 | task-assigned | task-163 | DependencyPolicy.require_cascade_tests flag exists but no cascade test runner. Policy flag with no executor. |
| 13 | Version Drift Tracking | 2 | implemented | - | target_version_current and version_drift fields on DependencyEdge. Migration 000049 adds column. Stale dependencies queryable via API. |
| 14 | Impact Analysis | 2 | implemented | - | GET /api/v1/repos/:id/blast-radius with BFS traversal. Returns direct and transitive dependents. |
| 15 | API | 2 | implemented | - | 13 endpoints: GET/POST /repos/:id/dependencies, GET /repos/:id/dependents, DELETE /repos/:id/dependencies/:dep_id, GET /dependencies/graph, GET /repos/:id/blast-radius, GET /workspaces/:id/dependency-graph, GET /dependencies/stale, GET /dependencies/breaking, POST /dependencies/breaking/:id/acknowledge, GET/PUT /workspaces/:id/dependency-policy. |
| 16 | CLI | 2 | task-assigned | task-164 | No gyre deps commands in CLI. Spec requires: gyre deps show, gyre deps impact, gyre deps stale, gyre deps breaking. |
| 17 | UI | 2 | task-assigned | task-164 | No dedicated dependency graph visualization component in web UI. DependencyGraph.svelte exists but for workspace architecture, not cross-repo dependency tracking. |
| 18 | Workspace Orchestrator Integration | 2 | implemented | - | DependencyPolicy model for workspace control. Auto-task creation on breaking changes creates orchestrator-consumable tasks. |
| 19 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
