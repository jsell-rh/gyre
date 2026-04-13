# Coverage: Realized System Model

**Spec:** [`system/realized-model.md`](../../system/realized-model.md)
**Last audited:** 2026-04-13 (full audit — verified against graph.rs, graph_extraction.rs, rust/ts/py/go extractors, graph.rs port, sqlite/graph.rs adapter, migrations)
**Coverage:** 8/9 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Problem statement — no implementable requirement. |
| 2 | Core Insight | 2 | n/a | - | Design rationale — no implementable requirement. |
| 3 | Design | 2 | n/a | - | Section heading only — no implementable requirement. |
| 4 | 1. The Knowledge Graph | 3 | implemented | - | GraphNode struct (gyre-common/src/graph.rs) with all spec fields: id, repo_id, node_type, name, qualified_name, file_path, line_start/end, visibility, doc_comment, spec_path, spec_confidence (None/Low/Medium/High), last_modified_sha/by/at, created_sha/at, complexity, test_coverage, churn_count_30d. GraphEdge with source/target/edge_type/metadata. All 9 node types and 11 edge types defined. DB tables: graph_nodes, graph_edges, graph_deltas (migration 000038+). |
| 5 | 2. Extraction Pipeline | 3 | implemented | - | LanguageExtractor trait (gyre-domain/src/extractor.rs). 4 built-in extractors: RustExtractor (syn-based AST parsing), TypeScriptExtractor, PythonExtractor, GoExtractor. Auto-detects languages from manifests. graph_extraction.rs orchestrates pipeline on push. ExtractionResult with nodes/edges/errors. Spec linkage via provenance-based and name-based heuristics. |
| 6 | 3. Architectural Timeline | 3 | implemented | - | ArchitecturalDelta with nodes_added/removed/modified, edges_added/removed (gyre-common/src/graph.rs). graph_deltas table stores serialized deltas per commit. GET /repos/:id/graph/timeline endpoint with ?since=&until= params. GET /repos/:id/graph/diff endpoint for two-commit comparison. Time-travel query support (migration 000038). |
| 7 | 4. Concept Views | 3 | implemented | - | GET /repos/:id/graph/concept/:concept_name endpoint. view_query_resolver.rs for concept projections. Concept definitions from spec manifest. Built-in views (Architecture Overview, Test Coverage Gaps, Hot Paths, Blast Radius, Spec Coverage, Ungoverned Risk). workspace-level concept search: GET /workspaces/:id/graph/concept/:name. |
| 8 | 5. Risk Metrics | 3 | implemented | - | GET /repos/:id/graph/risks endpoint. Computed from knowledge graph and git history: churn_rate, coupling_score, spec_coverage, complexity, fan_in/fan_out, agent_contention, staleness. Updated on each push via extraction pipeline. |
| 9 | 6. Narrative Generation | 3 | task-assigned | task-152 | Stubbed — graph.rs line ~254: "LLM-synthesized narrative (stubbed for now)." Template-based narrative generator not yet implemented. LLM narrative function not implemented. Briefing integration uses raw delta data, not human-readable summaries. |
| 10 | 7. API Surface | 3 | implemented | - | All 14 spec endpoints registered in mod.rs: GET /repos/:id/graph, /graph/types, /graph/modules, /graph/node/:id, /graph/spec/:path, /graph/concept/:name, /graph/timeline, /graph/risks, /graph/diff, POST /graph/link, POST /graph/predict. Workspace: GET /workspaces/:id/graph, /graph/concept/:name. Plus /graph/query-dryrun bonus endpoint. |
| 11 | 8. Storage | 3 | implemented | - | SQLite adapter (sqlite/graph.rs) + Postgres adapter. graph_nodes, graph_edges, graph_deltas tables. GraphRepository port trait (gyre-ports/src/graph.rs). MemGraph in-memory adapter for tests. Migrations: 000023 (test_coverage), 000038 (time_travel), 000043 (test_node). |
| 12 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
