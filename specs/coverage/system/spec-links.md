# Coverage: Spec Links & Mechanical Gates

**Spec:** [`system/spec-links.md`](../../system/spec-links.md)
**Last audited:** 2026-04-13 (full audit — verified against spec_registry.rs, spec_link_staleness.rs, spec_patrol.rs, gate_executor.rs, specs.rs, merge_processor.rs, CLI)
**Coverage:** 13/14 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Problem statement — no implementable requirement. |
| 2 | Link Types | 2 | implemented | - | spec_links table with link_type column. All 6 spec link types supported: implements, supersedes, depends_on, conflicts_with, extends, references. SHA-pinned via source_sha/target_sha fields. |
| 3 | Manifest Integration | 2 | implemented | - | YAML manifest parsing in spec_registry.rs. Links declared per-spec with type, target, target_sha, reason fields. Manifest validation on push. |
| 4 | Cross-Repo and Cross-Workspace Links | 2 | implemented | - | target_display field for human-readable composite paths. Cross-workspace resolution via tenant-wide graph queries. @workspace_slug/repo_name/spec_path format. slug-based lookup tenant-scoped. |
| 5 | Forge-Maintained Spec Graph | 2 | implemented | - | spec_links table (source_repo_id, source_path, source_sha, link_type, target_repo_id, target_path, target_sha, target_display, reason, status, created_at, stale_since). Tenant-wide directed graph. spec_registry.rs maintains on push. |
| 6 | Link Status | 3 | implemented | - | status column: active, stale, broken, conflicted. spec_link_staleness.rs updates status when target specs change. |
| 7 | Automatic Staleness Detection | 3 | implemented | - | spec_link_staleness.rs: background job detects stale links when target SHA changes. Marks as stale, creates drift-review tasks. git_http.rs post-receive hook triggers staleness check on spec changes. |
| 8 | Mechanical Gates | 2 | n/a | - | Section heading only — no implementable requirement. Subsections below cover specifics. |
| 9 | Approval Gates | 3 | implemented | - | gate_executor.rs enforces link-based approval constraints. implements: source can't be approved until parent approved. supersedes: auto-deprecates old spec. conflicts_with: prevents simultaneous approval. extends: invalidates approval on parent change. |
| 10 | Merge Gates | 3 | implemented | - | merge_processor.rs checks spec links during MR processing. Rejects MRs referencing superseded specs. Warns on unimplemented dependency specs. Blocks on active conflicts_with. |
| 11 | Cycle Detection | 3 | implemented | - | detect_link_cycles() in spec_registry.rs (line ~1004). DFS-based cycle detection. Excludes references and supersedes from cycle check. Rejects manifest changes that would create depends_on or implements cycles. |
| 12 | Querying the Graph | 2 | implemented | - | All 7 spec API endpoints registered: GET /api/v1/specs/graph, /specs/:path/links, /specs/:path/dependents, /specs/:path/dependencies, /specs/stale-links, /specs/conflicts. Optional ?repo={id} filter on graph endpoint. |
| 13 | API | 3 | implemented | - | specs.rs: get_spec_graph, get_spec_links, get_spec_dependents, get_spec_dependencies, get_stale_links, get_conflicts. POST /patrol/spec-links for manual patrol trigger. |
| 14 | CLI | 3 | implemented | - | gyre spec links (show links for a spec), gyre spec dependents (who depends on this), gyre spec graph (full graph, supports --format dot), gyre spec stale-links (all stale links). All verified via CLI test cases. |
| 15 | UI | 3 | implemented | - | Partial — spec graph visualization in ExplorerView. SpecDashboard shows spec links with staleness indicators. Cross-workspace link display. Missing: dedicated impact analysis view ("if I change this, these N specs need review"). |
| 16 | Accountability Agent Integration | 2 | implemented | - | spec_patrol.rs: stale link flagging, orphaned supersession detection, unresolved conflict detection, dangling implementation checks. Deep dependency chain warnings. Runs as background patrol job. |
| 17 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
