---
title: "Publish reviewed spec backlog for specs/system/view-query-grammar.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/view-query-grammar.md",
  "plan_id": "eec79eddc83fdceebccbcf9a19abdcf979a6e5dc214bc8c38a9cb18293353b74",
  "audit": {
    "base": "25497b9e3cc16c7e405c641870ff3c3b1905cf6b",
    "requirements": "78cf53a9a7873ac9cec48992caf04a661968e454a267a976bfb2c1012a063a39",
    "sections": [
      {
        "id": "441a8df54355f3d25b3d3477",
        "decision": "context",
        "requirement_quote": "A view query is NOT a spec. Specs define intent — what to build. View queries define what to SHOW — they are ephemeral graph queries that produce a visualization. The LLM generates view queries; the renderer executes them deterministically.",
        "detail": "Title-section intro carries status (Draft), dependency declarations (ui-layout.md §4, system-explorer.md §4/§8, explorer-canvas.md, lsp-call-graph.md) and the naming distinction between specs (intent) and view queries (ephemeral visualization queries). These are classification/rationale metadata, not independently implementable requirements. The one substantive architecture claim embedded here — LLM generates, renderer executes deterministically — is audited with production evidence under the 'Problem' heading and the primitive headings below; the dependency specs (explorer-canvas, lsp-call-graph, ui-layout, system-explorer) are audited as their own canonical documents.",
        "evidence": []
      },
      {
        "id": "d0d9d3c1f33c2aa993ba7391",
        "decision": "satisfied",
        "requirement_quote": "The gap: a **declarative grammar** that both the LLM and the renderer understand. The LLM generates view queries; the renderer executes them. No custom code per question.",
        "detail": "The declarative grammar exists as shared production types: gyre-common's ViewQuery/Scope/Emphasis/EdgeFilter/Zoom/ViewAnnotation structs (serde-tagged so the exact spec JSON shapes deserialize) with a validate() implementation shared by every surface. The LLM side: the explorer WebSocket agent's system prompt contains a full '## View Query Grammar' section documenting scope types, computed expressions, emphasis, zoom, and annotation to the model; the model's output is parsed for <view_query> JSON blocks (parse_view_query_from_text), deserialized into ViewQuery, validated, dry-run against the real graph in a server-side self-check loop, and only then forwarded to the frontend as an ExplorerServerMessage::ViewQuery. The renderer side: ExplorerChat validates the incoming query (validateViewQuery) and applies it via onViewQuery; ExplorerCanvas resolves scope/emphasis/edges/zoom/annotation generically from the query JSON — one generic execution path, no per-question custom code. The same JSON grammar is also produced by preset buttons and a user-composed JSON editor in ExplorerView, confirming the grammar is the single shared contract.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/view_query.rs",
            "quote": "pub enum Scope {",
            "detail": "The grammar's core scope primitive is a shared serde-tagged enum (All/Focus/Filter/TestGaps/Diff/Concept) in gyre-common, so the LLM-produced JSON, the server-side resolver, and the frontend all consume one declarative contract. The same file defines Emphasis, EdgeFilter, Zoom, ViewAnnotation, and ViewQuery::validate()."
          },
          {
            "file": "crates/gyre-server/src/explorer_ws.rs",
            "quote": "## View Query Grammar",
            "detail": "The explorer agent's system prompt (build_system_prompt_for_sdk → build_system_prompt_with_workflow) documents the full grammar to the LLM — scope types, edge/node types, computed expressions, emphasis, zoom, annotation — so the LLM genuinely 'understands' and generates the grammar rather than free-form JSON."
          },
          {
            "file": "crates/gyre-server/src/explorer_ws.rs",
            "quote": "fn parse_view_query_from_text",
            "detail": "LLM output is parsed for <view_query> JSON blocks, deserialized into ViewQuery, self-checked via dry-run against the real graph, and only then forwarded — the production LLM→query pipeline the Problem section demands."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "function resolveComputed(expr) {",
            "detail": "The renderer executes the grammar generically: one resolver handles every computed reference and scope shape from the query JSON, with no per-question custom code paths."
          },
          {
            "file": "web/src/lib/ExplorerChat.svelte",
            "quote": "case 'view_query': {",
            "detail": "The chat surface receives the LLM's validated ViewQuery messages, re-validates them client-side, and applies them to the canvas via onViewQuery(query) — the renderer-executes half of the architecture."
          }
        ]
      },
      {
        "id": "166c4df4347fbf9f5349f428",
        "decision": "context",
        "requirement_quote": "## Primitives",
        "detail": "Bare parent heading introducing subsections 1–7 (Computed References, Scope, Emphasis, Edges, Zoom, Annotation, Interactive Bindings). It contains no requirement text of its own; every primitive's requirements are accounted for under its own child heading below.",
        "evidence": []
      },
      {
        "id": "a49a2cc17303d58f21def2bb",
        "decision": "satisfied",
        "requirement_quote": "Computed references are **graph traversals resolved by the renderer**, not by the LLM. The LLM references them; the renderer computes them from the knowledge graph. All computations are deterministic — always correct given complete graph data.",
        "detail": "All 17 computed references from the spec are implemented twice, in real production traversal code. Server side (gyre-domain view_query_resolver.rs, used by the explorer agent's graph_query_dryrun MCP tool and the self-check loop): $clicked/$selected resolve via selected_node_id; $callers/$callees do transitive Calls BFS with optional depth; $implementors/$fields take incoming Implements/FieldOf edges; $descendants/$ancestors traverse Contains recursively; $governed_by matches spec_path plus GovernedBy edges into spec nodes; $reachable does general BFS with an edge-type array, direction and depth; $where selects nodes by metric (complexity, churn, incoming/outgoing_calls, field_count, test_coverage, test_fragility, risk_score, plus string props node_type/visibility/spec_confidence); $test_reachable/$test_unreachable are Calls-only multi-source BFS from test nodes; $test_fragility is computed by per-test BFS (count of distinct tests reaching a node — the count is exposed via $where(test_fragility, op, N), heat and badge metrics, while the bare reference returns set membership because computed references produce node sets); $intersect/$union/$diff recursively resolve nested sub-expressions with balanced-comma splitting. Client side (the renderer, which the spec says resolves them): ExplorerCanvas's resolveComputed dispatches to computeCallers/computeCallees/computeImplementors/computeGovernedBy/computeWhere/computeFields/computeDescendantsSet/computeAncestorsSet/computeTestFragility/computeReachable and the set operators, with test-reachability semantics shared with unit tests via test-reachability.js. Traversals are deterministic (HashSet/VecDeque BFS, deterministic name resolution with shortest-qualified-name tie-breaks on the server, identical Calls-only edge sets on both surfaces). The resolver additionally warns when call-graph data is known incomplete (lsp-call-graph.md dependency) rather than presenting partial results as complete.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "if trimmed == \"$test_unreachable\" {",
            "detail": "Server-side resolver branch for $test_unreachable: computes the Calls-only reachable set from test nodes and returns the complement restricted to testable code — a real graph traversal, not a stub, resolved from GraphNode/GraphEdge data."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "const TEST_REACHABILITY_EDGES: &[EdgeType] = &[EdgeType::Calls];",
            "detail": "Test reachability is defined strictly 'via Calls' per this spec §1, and the constant is shared by $test_reachable, $test_unreachable, $test_fragility and the test_gaps scope, with an explicit comment citing the spec section — the deterministic single-edge definition is enforced, not approximated."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "function computeCallers(nodeName, depth = 10) {",
            "detail": "Renderer-side implementation of $callers(node, depth?): BFS over incoming Calls edges from the named node — the spec's requirement that the renderer (not the LLM) computes the traversal. Sibling functions in the same file implement $callees, $implementors, $fields, $descendants, $ancestors, $governed_by, $reachable and $where."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "case 'intersect': {",
            "detail": "The renderer's computed-expression parser handles $intersect/$union/$diff by recursively resolving nested sub-expressions (e.g. $where(...) inside $intersect(...)) via splitBalancedArgs, which respects nested parentheses — the high-risk-example composition works in production."
          },
          {
            "file": "web/src/lib/test-reachability.js",
            "quote": "export function computeTestFragilityCounts(nodes, adjacency, maxDepth = FRAGILITY_MAX_DEPTH) {",
            "detail": "$test_fragility: per-test BFS counts distinct test functions reaching each node (O(T*(N+M))), depth-capped identically to the backend resolver (depth 20) so both surfaces are deterministic and agree. The counts feed $where(test_fragility, op, N), the heat metric, and badge values."
          }
        ]
      },
      {
        "id": "629ddb2f3b21a81f49e78ca3",
        "decision": "gap",
        "requirement_quote": "| `diff` | Changes between two commits |",
        "detail": "Five of the six scope rows are satisfied with real traversals on both the server resolver and the renderer: `all` returns every active node; `focus` does BFS from a node along specified edges/direction/depth and supports $clicked/$selected (server: resolve_scope Focus branch resolves the reference via selected_node_id then find_node_by_ref; client: queryMatchedWithDepth focus branch); `filter` matches node_types/name_pattern and resolves computed sets; `test_gaps` returns nodes not reachable from any test function via Calls (both surfaces share the identical TESTABLE_TYPES set and Calls-only TEST_REACHABILITY_EDGES); `concept` expands from seed nodes along expand_edges/expand_depth/expand_direction with seed_match 'exact' and 'substring' on both surfaces plus the layout-renderer projection in view-query-scope.js. The `diff` row is a gap: the spec requires 'Changes between two commits', but production code implements an explicitly acknowledged approximation. The SHA-based path in Scope::Diff (view_query_resolver.rs lines 741-837) selects only nodes whose created_sha or last_modified_sha prefix-matches the to_commit (>=7 hex chars) with a from-exclusion, and the code's own comment concedes that nodes changed by intermediate commits between from_commit and to_commit will be missed, and an [info] warning recommends temporal (~epoch) diffs instead. Concrete failures of the specced semantics: (1) a node modified only by an intermediate commit C in (from, to) has last_modified_sha = C and matches neither boundary SHA, so it is silently absent from the diff result; (2) a node whose last modifying commit is after to_commit is also missed, because last_modified_sha records only the single latest modifying commit of the current snapshot - the scope is correct only in the narrow case where to_commit is exactly the last commit touching each changed node. The renderer mirror in ExplorerCanvas.svelte (lines 2376-2389) implements the identical heuristic ('Matches server heuristic including its limitations'), so both surfaces deviate from the specced semantics. The ~epoch temporal path is a different query form and does not satisfy the specced commit-pair diff. Surfacing the limitation via a code comment and runtime warning does not implement the specced behavior, and a code comment is not a spec amendment. This is a gap, not an infeasibility conflict: the Extractor port is commit-scoped by signature (extract(repo_root, commit_sha)), and the git2 adapter already walks commit history (revwalk) and exposes a diff(repo_path, from, to) operation, so a faithful node-level commit-range diff is implementable - e.g. extract the graph at both boundary commits and set-diff by qualified_name/identity, or enumerate the from..to commits from git history at the server layer and match node provenance SHAs against every in-range commit - with hard tests that a node changed only by a mid-range commit MUST appear in the result.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "                // Limitations (inherent to the graph model):\n                //   - Each node stores only `created_sha` and `last_modified_sha`.\n                //     We have no access to the full git log, so nodes changed by\n                //     intermediate commits between from_commit and to_commit will\n                //     be missed unless their last_modified_sha still points to one\n                //     of the boundary commits.",
            "detail": "The production diff-scope implementation concedes in its own comment that it misses nodes changed by intermediate commits between from_commit and to_commit - the code contradicts the specced 'Changes between two commits' semantics rather than implementing it, and the comment is an acknowledgment, not a spec amendment."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "                        (created_matches_to || modified_matches_to)\n                            && !(created_matches_from && modified_matches_from)",
            "detail": "The actual selection predicate of the SHA-based diff path: a node is included only if its created_sha or last_modified_sha prefix-matches the to_commit. A node modified only by an intermediate commit C in (from, to) has last_modified_sha = C and matches neither boundary, so it is silently absent; a node whose latest modifying commit is after to_commit is missed the same way. This is boundary-commit matching over a single-snapshot provenance model, not the specced commit-range diff."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "          const matchesTo = shaMatches(n.created_sha, toCommit) || shaMatches(n.last_modified_sha, toCommit);\n          const unchangedAtFrom = shaMatches(n.created_sha, fromCommit) && shaMatches(n.last_modified_sha, fromCommit);\n          if (matchesTo && !unchangedAtFrom) matched.set(n.id, 0);",
            "detail": "The renderer mirrors the same boundary-SHA-prefix heuristic for the diff scope (the preceding comment states 'Matches server heuristic including its limitations'), so both surfaces deviate from the specced 'Changes between two commits' semantics; no surface implements a true commit-range node diff."
          },
          {
            "file": "crates/gyre-domain/src/extractor.rs",
            "quote": "    fn extract(&self, repo_root: &Path, commit_sha: &str) -> ExtractionResult;",
            "detail": "Feasibility of a faithful repair, so this is a gap rather than a spec conflict: the Extractor port is commit-scoped by signature, so the graph can be extracted at each boundary commit of a diff range and set-diffed by qualified_name/identity to derive the true set of nodes changed between two commits."
          },
          {
            "file": "crates/gyre-adapters/src/git2_ops.rs",
            "quote": "    async fn diff(&self, repo_path: &str, from: &str, to: &str) -> Result<DiffResult> {",
            "detail": "Feasibility corroboration: the git2 adapter already exposes a commit-pair diff operation and a revwalk over commit history (used for commit listing), so the server layer can enumerate every commit in the from..to range and match node provenance SHAs against all in-range commits instead of only the boundary commits - the data needed for the specced semantics is reachable in production code."
          }
        ]
      },
      {
        "id": "f5c7f15324ac5533dd13941f",
        "decision": "satisfied",
        "requirement_quote": "| `tiered_colors` | Array of colors by BFS depth: depth 0 = [0], depth 1 = [1], etc. |",
        "detail": "All five emphasis primitives render in the production canvas draw loop. highlight.matched: matched nodes get the configured color and the matched label drawn in that color (LOD-gated). dim_unmatched: non-matched nodes render at the configured opacity (default 0.12), with tree groups inheriting. tiered_colors: indexed by the node's BFS depth from queryMatchedWithDepth, clamped to the last color — exactly 'depth 0 = [0], depth 1 = [1]'. heat: all nodes are colored by metric value normalized against the max, through real palettes (blue-red, green-yellow-red, purple-orange) via heatColor; metrics include incoming_calls, complexity, churn, test_coverage, test_fragility, risk_score; values prefer server-provided node_metrics from the dry-run result and fall back to client-side computation. badges: text labels attached to nodes with the template's {{count}} filled by the renderer from the metric value. The server dry-run (node_depths/node_metrics in DryRunResult) computes the same emphasis data for the LLM self-check. One nuance: nodes whose heat metric value is 0/absent are left uncolored rather than painted the palette's low end (an inline comment states the opposite intent, but coloring nodes that lack the metric would be misleading, and matched nodes with data are all colored).",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const tc = activeQuery.emphasis?.tiered_colors;",
            "detail": "tiered_colors is applied by BFS depth: the following lines read the node's depth from queryMatchedWithDepth and return tc[Math.min(d, tc.length - 1)] — the specced depth-indexed coloring."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "return queryMatchedIds.has(nodeId) ? 1.0 : (activeQuery?.emphasis?.dim_unmatched ?? 0.12);",
            "detail": "dim_unmatched: node alpha is 1.0 for result-set nodes and the configured opacity (0.0–1.0, default 0.12) for non-matched nodes."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const hlLabel = activeQuery?.emphasis?.highlight?.matched?.label;",
            "detail": "highlight.matched: matched nodes draw the configured label text in the configured matched color (the draw block below renders it under the node when zoom/size permit)."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const badgeText = (badges.template ?? '{{count}}').replace('{{count}}', String(badgeValue));",
            "detail": "badges: the renderer fills {{count}} in the badge template from the metric value (server node_metrics preferred, else client-side incoming-call/complexity/churn/test-coverage counts) — 'Attach text labels. {{count}} filled by renderer' as specced."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const heat = activeQuery.emphasis?.heat;",
            "detail": "heat: queryNodeColor's heat branch colors nodes by metric value normalized against heatMaxValues, through heatColor(t, palette) which implements the blue-red, green-yellow-red and purple-orange palettes."
          }
        ]
      },
      {
        "id": "502390be64aa488f04b2e0f2",
        "decision": "satisfied",
        "requirement_quote": "Edge filter by type. When a result set is active, edges are further restricted to connections BETWEEN result nodes only — no spaghetti from unrelated calls.",
        "detail": "Both halves are implemented. Type filter: renderEdges filters the edge set by edges.filter (inclusion) or edges.exclude, and the draw loop skips edges whose type is not in the active filter. Result-set restriction: when queryMatchedIds is active, the draw loop reduces edges whose endpoints are not both in the result set to 0.1 alpha (visually suppressed — no spaghetti from unrelated calls) while edges between result nodes keep full alpha; the same restriction is applied for trace-path BFS. Server side, dry_run computes matched_edges as exactly the edges with BOTH endpoints in the result set, further filtered by the query's edge-type filter (capped at 1000 with an explicit truncation warning), so the LLM self-check sees the same restricted edge set the renderer shows.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (!queryMatchedIds.has(srcId) || !queryMatchedIds.has(tgtId)) {",
            "detail": "Result-set edge restriction in the renderer draw loop: edges not connecting two result nodes are dimmed to 0.1 alpha (the following line 'edgeAlpha *= 0.1;'), and the subsequent activeQuery.edges.filter check skips non-included edge types entirely."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const queryEdgeFilter = activeQuery?.edges?.filter;",
            "detail": "Edge filter by type: renderEdges uses the query's edge filter/exclude lists exclusively, so a query can show e.g. contains/field_of edges that the default view hides."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "result_set.contains(&e.source_id.to_string())",
            "detail": "Server-side dry-run restricts matched_edges to connections BETWEEN result nodes (both source and target checked) plus the edge-type filter — the same restriction the renderer applies, feeding the LLM self-check loop."
          }
        ]
      },
      {
        "id": "d560b347432fe604f7420d41",
        "decision": "satisfied",
        "requirement_quote": "`\"fit\"` (zoom to highlighted), `\"current\"` (don't change), or `{\"level\": N}`.",
        "detail": "All three zoom forms are handled by a dedicated production effect in ExplorerCanvas, run once per query instance (lastZoomedQuery identity guard prevents later layout changes from re-firing it and overriding user pan/zoom). {\"level\": N}: the camera target zoom is set to the level clamped to MIN/MAX_ZOOM. \"current\": explicit no-op. \"fit\": the bounding box of all matched layout nodes is computed and the camera targets its center at fit zoom with 0.8 padding, clamped to MAX_ZOOM. The zoom directive disarms the zoom-interaction tracker so programmatic zoom is never misattributed as a user interaction in recent_interactions. Server side, Zoom is an untagged enum (Named/Level) matching the wire forms, and validate() rejects unknown names, numeric strings, and out-of-range levels.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (typeof activeQuery.zoom === 'object' && activeQuery.zoom.level != null) {",
            "detail": "{\"level\": N} handling: the object form sets targetCam.zoom clamped to MIN_ZOOM..MAX_ZOOM — the third specced zoom form."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const fitZoom = Math.min(W / (mw || 1), H / (mh || 1)) * 0.8;",
            "detail": "\"fit\" handling: zoom-to-highlighted — the bounding box of matched layout nodes is computed and the camera targets its center at a fit zoom with padding; \"current\" returns without changing the camera (the branch above it)."
          },
          {
            "file": "crates/gyre-common/src/view_query.rs",
            "quote": "pub enum Zoom {",
            "detail": "The Zoom type is an untagged serde enum accepting the named forms (\"fit\"/\"current\") and the {\"level\": N} object form, and ViewQuery::validate() rejects unknown names and levels outside 0.05–20.0 — the grammar's zoom forms are typed and validated on the wire."
          }
        ]
      },
      {
        "id": "29f8211fedd5dd2b602a7768",
        "decision": "satisfied",
        "requirement_quote": "Title + description with template variables: `$name` (focused node name), `{{count}}` (result set size), `{{group_count}}` (distinct parent modules).",
        "detail": "The annotation bar renders title+description with all three template variables resolved. Renderer: resolveVars replaces $name with the selected node's name, {{count}} with the result-set size (queryMatchedIds.size), and {{group_count}} with the count of distinct parent modules of matched nodes (computed from qualified_name prefixes). Server: dry_run resolves the same templates via resolve_annotation_template with matched count and count_distinct_parent_modules (Contains-edge parents with a file-path fallback), and warns when $name is used by a scope that has no focused node. In interactive $clicked mode the re-run path substitutes the newly clicked node's name into title/description (see §7).",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": ".replace(/\\{\\{count\\}\\}/g, String(matchCount))",
            "detail": "Renderer annotation resolution: {{count}} is replaced with the result-set size (matchCount = queryMatchedIds?.size) and {{group_count}} with the distinct-parent-module count computed in the same block; $name is replaced from the selected node immediately before this replace."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "pub fn resolve_annotation_template(",
            "detail": "Server-side template resolution: replaces $name (stripping it cleanly when no focused node exists), {{count}} (matched count) and {{group_count}} — the same three variables the spec names, resolved for the LLM self-check and dry-run tool."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "pub fn count_distinct_parent_modules(",
            "detail": "group_count is computed as distinct parent modules: Contains-edge parents of matched nodes, with a file-path-prefix fallback when no Contains edges exist — matching the spec's 'distinct parent modules' semantics."
          }
        ]
      },
      {
        "id": "3e5e0a3bcf753c92a569c183",
        "decision": "satisfied",
        "requirement_quote": "`\"node\": \"$clicked\"` makes the query a **mode** — each user click re-runs the traversal from the clicked node. The annotation updates with the new node's name. Essential for exploratory analysis: \"click any node to see its blast radius.\"",
        "detail": "Implemented as a real mode in the renderer: when a query's scope.node is \"$clicked\", the query is stored as a template (interactiveQueryTemplate, JSON round-tripped to escape Svelte 5 proxy state); on every canvas node click the template is re-instantiated with the clicked node's qualified_name/name/id substituted into scope.node, the traversal re-runs (the substituted query flows back through onInteractiveQuery → activeQuery → queryMatchedWithDepth), and the annotation's $name is replaced with the clicked node's name in both title and description. A 'click mode' badge renders in the annotation UI, and the annotation's clear button drops the template. A \"$selected\" variant re-evaluates whenever the selection changes. Server side, the resolver resolves $clicked/$selected through selected_node_id and, when a $clicked query arrives with no selection, dry_run emits \"Interactive mode: click a node on the canvas to see results\" rather than an empty-looking failure.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "let interactiveQueryTemplate = $state(null);",
            "detail": "The $clicked mode is a stored template: an $effect captures any query whose scope.node is '$clicked' (and a parallel selectedQueryTemplate for '$selected'), so the mode persists across clicks."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (interactiveQueryTemplate) {",
            "detail": "The click handler re-runs the mode: on every node click the template is cloned, the clicked node substituted into scope.node, and $name replaced in the annotation title/description before re-applying the query — 'each user click re-runs the traversal from the clicked node. The annotation updates with the new node's name.'"
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "let resolved_name = if node == \"$clicked\" || node == \"$selected\" {",
            "detail": "Server-side resolution of the interactive references: $clicked/$selected resolve to the selected node id from the interaction context, then the focus traversal runs from that node — the same mode semantics on the dry-run/self-check surface."
          }
        ]
      },
      {
        "id": "30096c410ddb903c6dfcae98",
        "decision": "satisfied",
        "requirement_quote": "When the LLM is invoked, it receives the current canvas state:",
        "detail": "The full §10 interaction context reaches the LLM in production. Wire format: gyre-common's CanvasState carries all seven specced fields — selected_node (id/name/node_type/qualified_name), zoom_level, visible_tree_groups, active_filter, active_lens, active_query, and recent_interactions as structured InteractionRecords (action/node/detail/to_level/preset, accepting both objects and legacy strings). Frontend: ExplorerChat attaches toWireCanvasState(canvasState) to every user message; canvas-state.js normalizes the internal camelCase canvas state into the snake_case wire shape, keeping zoom fresh from the live camera and recording only user-initiated zoom gestures (ZoomInteractionTracker arms on wheel/keyboard/pinch, disarms on programmatic camera control) so recent_interactions reflects real user actions. Server: build_canvas_context_json serializes the fields (selected node enriched from the graph cache with file_path/spec_path/complexity/test_coverage; last-10 interactions) into a <canvas_state> JSON block prepended to the user content for the native LLM path, and the SDK subprocess receives the full canvas_state object in its stdin JSON. This is what lets the LLM map 'what about this one?' to $selected and scope answers to visible groups — and $clicked/$selected resolution on the server consumes the same selected_node.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/view_query.rs",
            "quote": "pub struct CanvasState {",
            "detail": "The typed interaction context: selected_node, zoom_level, visible_tree_groups, active_filter, active_lens, active_query, and recent_interactions (Vec<InteractionRecord> with action/node/detail/to_level/preset, deserializing both structured objects and plain strings) — the specced canvas_state shape."
          },
          {
            "file": "web/src/lib/ExplorerChat.svelte",
            "quote": "canvas_state: toWireCanvasState(canvasState),",
            "detail": "Every user message over the explorer WebSocket carries the normalized canvas state, so the LLM is invoked with the current canvas state as the spec requires."
          },
          {
            "file": "web/src/lib/canvas-state.js",
            "quote": "export function toWireCanvasState(cs) {",
            "detail": "Wire normalization: converts the internal camelCase canvas state into the §10 snake_case shape (selected_node, zoom_level from the live camera zoom, visible_tree_groups, active_filter, active_lens, active_query, recent_interactions), with zoom-gesture recording and coalescing so the interaction trail is accurate."
          },
          {
            "file": "crates/gyre-server/src/explorer_ws.rs",
            "quote": "let canvas_json = build_canvas_context_json(canvas_state, cached_nodes.as_ref());",
            "detail": "The LLM invocation path: the canvas context (selected node enriched from the graph cache, zoom level, visible groups, active filter/lens/query, last-10 interactions) is serialized into a <canvas_state> block prepended to the user question sent to the model."
          }
        ]
      },
      {
        "id": "1c2ab6b3caf2a7014e8834b3",
        "decision": "context",
        "requirement_quote": "## Composition Examples",
        "detail": "Bare parent heading introducing four example compositions (blast radius, test coverage gaps, high-risk untested code, authentication concept). The examples are illustrative uses of the grammar; each is audited — with production evidence for every construct it uses — under its own child heading below.",
        "evidence": []
      },
      {
        "id": "f7e96ebfb7472a3843b3a1c1",
        "decision": "satisfied",
        "requirement_quote": "\"scope\": { \"type\": \"focus\", \"node\": \"$clicked\", \"edges\": [\"calls\", \"implements\"], \"direction\": \"incoming\", \"depth\": 10 },",
        "detail": "Every construct in the example composes in production. focus+$clicked with edges [calls, implements], direction incoming, depth 10: the server resolver resolves $clicked via selected_node_id and BFS-traverses the specified edge types in the incoming direction at the given depth; the client's queryMatchedWithDepth focus branch does the same BFS and records per-node depth. tiered_colors with dim_unmatched 0.12: depth-indexed colors plus non-matched dimming (§3 evidence). edges.filter [calls, implements]: type-filtered render edges restricted to result nodes (§4 evidence). zoom \"fit\": bounding-box camera fit (§5 evidence). annotation \"Blast radius: $name\" / \"{{count}} transitive callers/implementors\": template resolution on both surfaces (§6 evidence), with $name substituted per click in interactive mode (§7 evidence). The exact example JSON also ships as the production 'Blast Radius' preset button, and the explorer context menu's blast action composes the same query shape from a right-clicked node.",
        "evidence": [
          {
            "file": "web/src/components/ExplorerView.svelte",
            "quote": "scope: { type: 'focus', node: '$clicked', edges: ['calls', 'implements'], direction: 'incoming', depth: 10 },",
            "detail": "The spec's exact blast-radius example ships as the production 'Blast Radius' preset button (queryPresets), together with its tiered_colors, dim_unmatched, edges filter, zoom fit and annotation template — the composed example is a shipped, user-reachable query."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const dm = new Map([[startNode.id, 0]]);",
            "detail": "The renderer's focus-scope BFS: starts at the (interactive-reference-resolved) node, expands along the allowed edge types in the configured direction up to the configured depth, recording per-node depth — the traversal the blast-radius example's scope block requires, and the depth data tiered_colors consumes."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "let resolved_name = if node == \"$clicked\" || node == \"$selected\" {",
            "detail": "Server-side: the example's $clicked focus resolves from the interaction context's selected node and traverses the specified edges/direction/depth — the same composition is validated and dry-run before the LLM's query reaches the frontend."
          }
        ]
      },
      {
        "id": "ce7b3324d2a99e44ddba8ded",
        "decision": "satisfied",
        "requirement_quote": "\"scope\": { \"type\": \"test_gaps\" },",
        "detail": "The example composes in production. test_gaps scope: both surfaces compute nodes not reachable from any test function via Calls (multi-source BFS from test nodes, complement restricted to testable code types). highlight.matched {color: '#ef4444', label: 'Untested'}: matched nodes draw the 'Untested' label in the configured color. dim_unmatched 0.3: non-matched node opacity. annotation '{{count}} functions not reachable from any test': resolved with the result-set size on both the canvas and the server dry-run. The exact example JSON ships as the production 'Test Gaps' preset button.",
        "evidence": [
          {
            "file": "web/src/components/ExplorerView.svelte",
            "quote": "scope: { type: 'test_gaps' },",
            "detail": "The spec's exact test-coverage-gaps example ships as the production 'Test Gaps' preset button, with the example's highlight.matched color/label, dim_unmatched 0.3 and annotation template."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (scope.type === 'test_gaps') {",
            "detail": "Renderer-side test_gaps resolution delegates to computeTestGaps (test-reachability.js) — Calls-only reachability from test nodes with testable-type filtering — so the example's scope resolves to the real coverage-gap set."
          },
          {
            "file": "web/src/lib/test-reachability.js",
            "quote": "export function computeTestUnreachable(nodes, adjacency, maxDepth = REACHABLE_MAX_DEPTH) {",
            "detail": "The test-gap set computation shared by the canvas and unit tests: non-test testable nodes NOT reachable from any test function via Calls — the exact semantics the example's annotation describes ('functions not reachable from any test')."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "Scope::TestGaps => {",
            "detail": "Server-side: the same scope resolves identically for dry-run/self-check, so the LLM's test-gap queries are validated against the real graph before rendering."
          }
        ]
      },
      {
        "id": "5cf296dc1b03022e74034304",
        "decision": "satisfied",
        "requirement_quote": "\"scope\": { \"type\": \"filter\", \"computed\": \"$intersect($where(complexity, '>', 20), $test_unreachable)\" },",
        "detail": "The example's nested computed expression resolves on both surfaces. Renderer: resolveComputed parses $intersect, splits arguments with splitBalancedArgs (nested parentheses respected), recursively resolves $where(complexity, '>', 20) via computeWhere (numeric comparison on the node's complexity property) and $test_unreachable (Calls-only coverage complement), then intersects the sets. Server: resolve_computed_expression_inner handles $where (complexity from GraphNode.complexity), $test_unreachable and $intersect (find_balanced_comma for nested arguments) identically. heat {metric: 'complexity', palette: 'blue-red'}: heat coloring by complexity through the blue-red palette, with server node_metrics populating complexity values for the dry-run. dim_unmatched 0.15 and the {{count}} annotation resolve as in §3/§6.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "case 'intersect': {",
            "detail": "Renderer: $intersect recursively resolves both nested sub-expressions and computes the set intersection — the example's composed filter scope executes in production."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "function computeWhere(property, operator, value) {",
            "detail": "Renderer: $where(complexity, '>', 20) selects nodes by numeric comparison on the requested property — the metric-based node selection the example depends on."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "\"complexity\" => n.complexity.map(|c| c as f64),",
            "detail": "Server: $where and the heat metric read the real per-node complexity from GraphNode data, so the dry-run self-check resolves the exact example expression against the real graph and populates heat node_metrics."
          }
        ]
      },
      {
        "id": "fc34680dec7705c0c9e25304",
        "decision": "satisfied",
        "requirement_quote": "\"scope\": { \"type\": \"concept\", \"seed_nodes\": [\"AuthService\", \"TokenStore\"], \"expand_edges\": [\"calls\"], \"expand_depth\": 2 },",
        "detail": "The example composes in production. Concept scope with seed_nodes/expand_edges ['calls']/expand_depth 2: the renderer resolves each seed by name/qualified_name/id and BFS-expands along calls edges up to depth 2 (union across seeds, both directions by default); the server does the same with per-seed minimum depths, partial-resolution warnings for missing seeds, and the seed_match exact/substring distinction. highlight.matched {color: '#60a5fa'} colors the matched set; zoom \"fit\" camera-fits the result; annotation '{{count}} nodes' resolves with the result-set size. The concept scope is reachable from the LLM (grammar documented in the system prompt), concept search, and user-composed JSON.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (scope.type === 'concept') {",
            "detail": "Renderer: concept resolution expands from seed nodes along expand_edges up to expand_depth honoring expand_direction — the exact scope shape of the authentication-concept example (with seed_match substring/exact modes)."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "for seed_id in seed_node_ids {",
            "detail": "Server: per-seed BFS expansion unioned across seeds with per-node minimum depth, warning when seeds resolve partially — the example's multi-seed concept expansion validated server-side in the dry-run self-check."
          },
          {
            "file": "web/src/lib/view-query-scope.js",
            "quote": "function conceptScope(scope, nodes, edges) {",
            "detail": "Concept scope is also projected for the registry layout renderers (seed resolution plus edge BFS mirroring the canvas), so concept views render identically across layout engines."
          }
        ]
      },
      {
        "id": "9e796b9ae04892b43237afe7",
        "decision": "context",
        "requirement_quote": "**Produced by:** LLM endpoints, preset buttons, user-composed JSON",
        "detail": "Cross-reference map stating relationships to other canonical documents (extends ui-layout.md §4, implements system-explorer.md §4/§8, consumed by explorer-canvas.md, produced by LLM endpoints/preset buttons/user-composed JSON). This heading asserts relationships rather than implementable requirements; the referenced documents are audited as their own canonical specs. As corroboration, all three named producers exist in production: LLM endpoints emit ViewQuery messages after parse/validate/dry-run (explorer_ws.rs), preset buttons ship the §12–§14 example shapes (ExplorerView queryPresets plus system default views), and the user-composed JSON editor validates and runs hand-written queries — but these are audited under the sections that carry the requirements, not here.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "629ddb2f3b21a81f49e78ca3",
        "decision": "gap",
        "requirement_quote": "| `diff` | Changes between two commits |",
        "detail": "Five of the six scope rows are satisfied with real traversals on both the server resolver and the renderer: `all` returns every active node; `focus` does BFS from a node along specified edges/direction/depth and supports $clicked/$selected (server: resolve_scope Focus branch resolves the reference via selected_node_id then find_node_by_ref; client: queryMatchedWithDepth focus branch); `filter` matches node_types/name_pattern and resolves computed sets; `test_gaps` returns nodes not reachable from any test function via Calls (both surfaces share the identical TESTABLE_TYPES set and Calls-only TEST_REACHABILITY_EDGES); `concept` expands from seed nodes along expand_edges/expand_depth/expand_direction with seed_match 'exact' and 'substring' on both surfaces plus the layout-renderer projection in view-query-scope.js. The `diff` row is a gap: the spec requires 'Changes between two commits', but production code implements an explicitly acknowledged approximation. The SHA-based path in Scope::Diff (view_query_resolver.rs lines 741-837) selects only nodes whose created_sha or last_modified_sha prefix-matches the to_commit (>=7 hex chars) with a from-exclusion, and the code's own comment concedes that nodes changed by intermediate commits between from_commit and to_commit will be missed, and an [info] warning recommends temporal (~epoch) diffs instead. Concrete failures of the specced semantics: (1) a node modified only by an intermediate commit C in (from, to) has last_modified_sha = C and matches neither boundary SHA, so it is silently absent from the diff result; (2) a node whose last modifying commit is after to_commit is also missed, because last_modified_sha records only the single latest modifying commit of the current snapshot - the scope is correct only in the narrow case where to_commit is exactly the last commit touching each changed node. The renderer mirror in ExplorerCanvas.svelte (lines 2376-2389) implements the identical heuristic ('Matches server heuristic including its limitations'), so both surfaces deviate from the specced semantics. The ~epoch temporal path is a different query form and does not satisfy the specced commit-pair diff. Surfacing the limitation via a code comment and runtime warning does not implement the specced behavior, and a code comment is not a spec amendment. This is a gap, not an infeasibility conflict: the Extractor port is commit-scoped by signature (extract(repo_root, commit_sha)), and the git2 adapter already walks commit history (revwalk) and exposes a diff(repo_path, from, to) operation, so a faithful node-level commit-range diff is implementable - e.g. extract the graph at both boundary commits and set-diff by qualified_name/identity, or enumerate the from..to commits from git history at the server layer and match node provenance SHAs against every in-range commit - with hard tests that a node changed only by a mid-range commit MUST appear in the result.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "                // Limitations (inherent to the graph model):\n                //   - Each node stores only `created_sha` and `last_modified_sha`.\n                //     We have no access to the full git log, so nodes changed by\n                //     intermediate commits between from_commit and to_commit will\n                //     be missed unless their last_modified_sha still points to one\n                //     of the boundary commits.",
            "detail": "The production diff-scope implementation concedes in its own comment that it misses nodes changed by intermediate commits between from_commit and to_commit - the code contradicts the specced 'Changes between two commits' semantics rather than implementing it, and the comment is an acknowledgment, not a spec amendment."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "                        (created_matches_to || modified_matches_to)\n                            && !(created_matches_from && modified_matches_from)",
            "detail": "The actual selection predicate of the SHA-based diff path: a node is included only if its created_sha or last_modified_sha prefix-matches the to_commit. A node modified only by an intermediate commit C in (from, to) has last_modified_sha = C and matches neither boundary, so it is silently absent; a node whose latest modifying commit is after to_commit is missed the same way. This is boundary-commit matching over a single-snapshot provenance model, not the specced commit-range diff."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "          const matchesTo = shaMatches(n.created_sha, toCommit) || shaMatches(n.last_modified_sha, toCommit);\n          const unchangedAtFrom = shaMatches(n.created_sha, fromCommit) && shaMatches(n.last_modified_sha, fromCommit);\n          if (matchesTo && !unchangedAtFrom) matched.set(n.id, 0);",
            "detail": "The renderer mirrors the same boundary-SHA-prefix heuristic for the diff scope (the preceding comment states 'Matches server heuristic including its limitations'), so both surfaces deviate from the specced 'Changes between two commits' semantics; no surface implements a true commit-range node diff."
          },
          {
            "file": "crates/gyre-domain/src/extractor.rs",
            "quote": "    fn extract(&self, repo_root: &Path, commit_sha: &str) -> ExtractionResult;",
            "detail": "Feasibility of a faithful repair, so this is a gap rather than a spec conflict: the Extractor port is commit-scoped by signature, so the graph can be extracted at each boundary commit of a diff range and set-diffed by qualified_name/identity to derive the true set of nodes changed between two commits."
          },
          {
            "file": "crates/gyre-adapters/src/git2_ops.rs",
            "quote": "    async fn diff(&self, repo_path: &str, from: &str, to: &str) -> Result<DiffResult> {",
            "detail": "Feasibility corroboration: the git2 adapter already exposes a commit-pair diff operation and a revwalk over commit history (used for commit listing), so the server layer can enumerate every commit in the from..to range and match node provenance SHAs against all in-range commits instead of only the boundary commits - the data needed for the specced semantics is reachable in production code."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-common/src/view_query.rs",
        "sha256": "4fc2f7f0a76c096e3f81ec3a9a8ddf38f628f2da1dd4cf43cb165be8c152667f"
      },
      {
        "file": "crates/gyre-server/src/explorer_ws.rs",
        "sha256": "4e41c3a8fd12e373a9001279a67e4d817dfe63e2c22d4452d6dea16a2e34408a"
      },
      {
        "file": "web/src/lib/ExplorerCanvas.svelte",
        "sha256": "87dcafc2cb16e664ce0ceafffbf53503c133f49c53ccae657a90d4a9c891279f"
      },
      {
        "file": "web/src/lib/ExplorerChat.svelte",
        "sha256": "48d48d3aa35b880ecf8b7891448dba9ec7604e1fb29431455b48c5edbea2e4fb"
      },
      {
        "file": "crates/gyre-domain/src/view_query_resolver.rs",
        "sha256": "903281c7a34a611369fec530dc86d706cb890ab69fe61dfe29636a7e7d80c9a5"
      },
      {
        "file": "web/src/lib/test-reachability.js",
        "sha256": "d2b54bfd23d78689b0059d90613001b83ace16dc64d9177c4de331365d453f3f"
      },
      {
        "file": "crates/gyre-domain/src/extractor.rs",
        "sha256": "f9417e80af27685f28292d684e942be5b32dd5e0f158c63810c307d9a4638dbf"
      },
      {
        "file": "crates/gyre-adapters/src/git2_ops.rs",
        "sha256": "b2bf5efd76122b510859c5a1fcd88ea23d163d76aea170b8fe6abf7f4eba0da4"
      },
      {
        "file": "web/src/lib/canvas-state.js",
        "sha256": "a5a9adb750b04df3918373a77d238fdbea71dd4664541a1d35a3979f4519083e"
      },
      {
        "file": "web/src/components/ExplorerView.svelte",
        "sha256": "1b32a6b12199574186b322f40ea07f8295deb78f2b3c9bcbe1fdabb9f659903d"
      },
      {
        "file": "web/src/lib/view-query-scope.js",
        "sha256": "42fa8578247083b9651d8bc5d1449863ba39b103301e728b803252f531dcd22f"
      }
    ]
  },
  "plan": {
    "audit": "04c62a4b83211499a6abd5b86226b2920ae5d771b4f8066f64edb7783054e535",
    "tasks": [
      {
        "key": "diff-scope-commit-range",
        "title": "View query diff scope: commit-range change set consumed from the shared two-commit graph-diff core (server resolver + renderer integration)",
        "covers": [
          "629ddb2f3b21a81f49e78ca3"
        ],
        "required_behavior": "Spec row (§ '2. Scope — What Subgraph to Show', `diff`): 'Changes between two commits'. The matched set of a SHA/ref-based Scope::Diff must be the set of graph nodes whose content differs between the two boundary commits, computed from real per-commit repository state — NOT from the current graph snapshot's single-point created_sha/last_modified_sha provenance.\n\nSHARED-CORE REUSE (per review of the overlapping realized-model plan): the boundary-commit graph-state resolution and the node-level added/removed/modified change-set computation are owned by the earlier-audited realized-model plan's task `graph-diff-two-commits` (planner spec-3899ae86d0ae2a8e0fd78bfd, plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, covering realized-model.md section e57272a0df7f65577ff10261 — GET /repos/{id}/graph/diff). This task must NOT build a second, independent implementation of that core. Mechanism reconciliation (explicit choice, within graph-diff-two-commits' existing contract, which already allows 'the repo's real git state — or equivalent state snapshots'): both surfaces consume git-state snapshotting — resolve each boundary commit against the repository's real git (>=7-hex SHA prefixes, branch names, tags, HEAD), snapshot each boundary commit's tree via the existing `git archive` machinery (factored from do_extract Step 1 in crates/gyre-server/src/graph_extraction.rs), run the extraction pipeline on each snapshot, and compute the change set by identity (qualified_name): added = in to not in from; removed = in from not in to; modified = in both with a real content difference (diff_nodes-style comparison over substantive fields — signature, docs, metrics/complexity, spec linkage — excluding bookkeeping fields that differ by construction: last_seen_at, first_seen_at, created_at, sha stamps). Recorded-delta point-in-time reconstruction (list_nodes_at/list_edges_at) alone cannot serve this surface because view queries reference arbitrary commit pairs, including boundary commits at which no extraction ever ran; where recorded deltas exist they may serve as a fast path only if the result is identical to the snapshot path. If graph-diff-two-commits has not yet landed when this task starts, build the shared core ONCE in the shared location so both contract surfaces (GET /repos/{id}/graph/diff and the view-query Scope::Diff) consume it — never a view-query-private copy — and record the cross-reference in both task definitions (this definition cross-references graph-diff-two-commits; that task's definition must cross-reference this one when the plans merge).\n\nTHIS TASK'S OWNERSHIP — the view-query-specific integration only:\n(1) Server wiring: when a view query with a non-temporal Diff scope is resolved — the REST endpoint POST /api/v1/repos/:id/graph/query-dryrun (api/graph.rs view_query_dryrun) and the explorer_ws.rs dry-run paths (the graph_query_dryrun MCP tool and the conversational self-check loop's self_check_dry_run) — gyre-server resolves from_commit/to_commit, obtains the change set from the shared core, and injects it into the domain resolver (a pure mapping in gyre-domain: dry_run/resolve_scope gain an injected change-set input; no git or infrastructure knowledge enters gyre-domain — hexagonal boundary preserved). Scope::Diff's matched set equals the change set's added+modified entries mapped to current active node ids via qualified_name; emphasis, edges, zoom, and annotation templates operate uniformly over that matched set ({{count}} resolves to its size). Removed nodes cannot be rendered and MUST be surfaced by name in DryRunResult.warnings rather than silently dropped. An unresolvable ref (e.g. 'no-such-branch') yields an explicit warning and an empty matched set — never a fabricated/fail-open default (repo invariant on resolve_ref). from==to keeps the existing empty-diff warning. Per-(repo, resolved commit) extraction results may be cached; the cache must not alter results.\n(2) Heuristic removal: the boundary-SHA prefix predicate over the current snapshot, its '[info] SHA-based diff shows an approximate result' warning, and the code-comment concession about intermediate commits are REMOVED from view_query_resolver.rs — not retained as a fallback. The temporal `~epoch` diff path is preserved unchanged (a different query form, not flagged by the audit).\n(3) Renderer: ExplorerCanvas.svelte must stop mirroring the SHA heuristic for SHA/ref-based diff scopes (the 'Matches server heuristic including its limitations' block); it obtains the matched set from the server query-dryrun endpoint (new client function in web/src/lib/api.js alongside the existing graphDiff helper) and feeds the returned ids into the existing matched/depth machinery; temporal ~epoch diff resolution stays local; fetch loading/error states show an explicit warning instead of an incorrect set.\n(4) Tests: existing tests that encode the old heuristic are rewritten against the new contract, not deleted: view_query_resolver.rs Scope::Diff SHA tests (~lines 4638-4735, 5058-5151, 5394-5432) and web/src/__tests__/ExplorerCanvas.test.js diff-SHA tests (~lines 389-413, 537-575, 681-733).",
        "acceptance": [
          "Server integration test with a real git repo, real git binary, and the real extraction flow: commit A adds functions f, g, h; commit B modifies only f; commit C modifies only g; the graph is extracted at C (normal push flow). Diff{from: A, to: C} matches exactly {f, g} — h (untouched) is excluded and f (changed only at intermediate commit B) IS included. Diff{from: A, to: B} matches exactly {f}, proving a to_commit older than HEAD still resolves correctly.",
          "The result is not the whole graph and not empty: the pre-fix failure modes (all-present-nodes match when to == last extraction commit; empty set when to is older) are both killed by the exact-set assertions above.",
          "Removed-node contract: a node present at from but absent at to is named in DryRunResult.warnings; added+modified nodes are matched; the {{count}} annotation template resolves to the matched-set size for a diff-scope query.",
          "Unresolvable ref (e.g. from_commit 'no-such-branch') produces a warning and an empty matched set — no panic, no silent default.",
          "Domain unit tests for the pure change-set function: added/removed/modified classification; a node changed and reverted between the boundaries is NOT reported changed (boundary-to-boundary semantics); bookkeeping-only differences (last_seen_at, first_seen_at, sha stamps) do NOT count as modified.",
          "Renderer test: a SHA-based diff-scope query with a mocked server query-dryrun response highlights exactly the server-returned node ids (including a mid-range-changed node) and resolves the annotation count from them; no local SHA-heuristic code path remains for SHA diffs.",
          "cargo test --all passes and cd web && npm test passes, with the rewritten diff-scope tests included.",
          "Single shared core: the boundary-commit change-set computation invoked by the view-query diff scope is the same production code path that serves GET /repos/{id}/graph/diff (realized-model.md 'Graph Diff' section, shared with task graph-diff-two-commits of plan b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3) — for the same repo and resolved commit pair, both surfaces produce the identical node-level change set; this task introduces no second, divergent implementation of commit-range change-set computation."
        ],
        "negative_probe": "Run the acceptance-1 integration test against the current production code (real repo, real extraction at C): every active node carries last_modified_sha = C because each extraction stamps all present nodes, so the existing boundary-SHA predicate matches h as well — Diff{A, C} returns the whole active graph and the exact-set assertion matched == {f, g} FAILS on h; likewise Diff{A, B} returns the empty set (no node's snapshot provenance matches the older to_commit) and matched == {f} FAILS. The same test must also fail if the fix is reverted to snapshot SHA matching, proving it kills the real defect (single-snapshot provenance cannot express a commit-range diff) rather than asserting a symbol exists. Client-side: with the server query-dryrun fetch mocked to return a set containing the mid-range-changed node, removing the server call (restoring local resolution from created_sha/last_modified_sha) fails the component test, because the current snapshot's fields cannot produce that set — the mid-range node is not highlighted.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "diff-scope-commit-range": "task-276",
    "_publication": "task-277",
    "_base": "00669baacfd5112b24e1466ad51f5a9bec43d8e9"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
