---
title: "Publish reviewed spec backlog for specs/system/lsp-call-graph.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/lsp-call-graph.md",
  "plan_id": "a8e88983c3060cd3af4d90d16e8709208b9361fa1dece0940873c9e8d32bc60d",
  "audit": {
    "base": "8546a781f772f05c823bbf4df6127bec812e2152",
    "requirements": "d96599f1065958758704a5cebced81b0636fed8936fb66d8801d321c839805c0",
    "sections": [
      {
        "id": "529b19ec0e0896db7e0a895c",
        "decision": "context",
        "requirement_quote": "> **Status: Draft.** Depends on: `realized-model.md` (knowledge graph), `explorer-canvas.md` (visualization). Implements: `vision.md` Principle 5 (Feedback Loop), `system-explorer.md` §5 (Flow Traces).",
        "detail": "Document title and status header. Pure metadata (draft status, dependency and implements declarations); no implementable requirement in this heading itself. The substantive requirements live in the child sections audited individually below.",
        "evidence": []
      },
      {
        "id": "2de8d5b8c1c254bdc03c1eea",
        "decision": "context",
        "requirement_quote": "But call resolution is incomplete — only ~3% of actual calls resolve because:",
        "detail": "Problem statement: motivates the spec by listing why syntax-only call resolution fails (cross-module aliases, trait/interface dispatch, generics, re-exports, dynamic dispatch) and what is lost without complete call edges. Rationale only — it describes the pre-existing state and the motivation, not a behavior to implement; the required behavior (delegation to type checkers) is carried by the Solution/Architecture/Per-Language sections audited below.",
        "evidence": []
      },
      {
        "id": "caf85c302dac0c9e2afd15cc",
        "decision": "satisfied",
        "requirement_quote": "Each language has a type checker that already solves call resolution completely. Instead of reimplementing type resolution in our extractors, delegate to the tools designed for it.",
        "detail": "The section requires that Pass 2 call-resolution is delegated to the specced type checkers per language: rust-analyzer (Rust), pyright (Python), golang.org/x/tools/go/callgraph (Go), the TypeScript compiler/tsserver surface (TypeScript). Production wiring: the SubprocessCallGraphExtractor adapter (behind the CallGraphExtractor port) dispatches per Language to exactly these tools — Go shells out to the committed CHA binary built on golang.org/x/tools/go/callgraph, Rust is driven by a native rust-analyzer LSP client, Python by a pyright-langserver LSP driver script, TypeScript by a typescript-language-server LSP driver script. Each tool prints JSON {from,to} edges parsed into CallEdge and resolved to Pass 1 node IDs by gyre_domain::call_graph_resolve.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "Language::Python => match find_script(\"python-callgraph.sh\") {",
            "detail": "The Pass 2 adapter's per-language dispatch: each Language variant routes to that language's type-checker tool (Go -> go-callgraph CHA binary, Rust -> native rust-analyzer LSP client, Python -> pyright driver script, TypeScript -> tsserver driver script), implementing the delegation table."
          },
          {
            "file": "scripts/go-callgraph/main.go",
            "quote": "\"golang.org/x/tools/go/callgraph/cha\"",
            "detail": "The Go tool is built on exactly the specced package (golang.org/x/tools/go/callgraph, CHA), loaded via packages.Load over the repo's ./... packages."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "pub async fn extract_rust_call_edges(repo_path: &Path) -> Result<Vec<CallEdge>> {",
            "detail": "Rust delegates to rust-analyzer out of process: this module owns a minimal LSP client that spawns rust-analyzer, sends textDocument/references for every Pass 1 function definition, and emits CallEdges — no type resolution is reimplemented locally."
          },
          {
            "file": "scripts/ts-callgraph.mjs",
            "quote": "// ts-callgraph.mjs — Extract caller→callee edges from a TypeScript project",
            "detail": "TypeScript delegates to the TypeScript compiler surface via typescript-language-server/tsserver over LSP (documentSymbol + textDocument/references), per the section's tool table row for TypeScript."
          }
        ]
      },
      {
        "id": "79c9fd1ebfd3a17e829d97d2",
        "decision": "satisfied",
        "requirement_quote": "  3. Merge: combine first-pass nodes/edges with second-pass Calls edges\n     - Deduplicate edges\n     - Store in graph_store",
        "detail": "The architecture requires: Pass 1 syntax extraction (nodes + Contains/Implements edges) runs first and updates the graph; Pass 2 starts the language analysis server, asks find-references per function/method node, resolves the enclosing function at each reference site into a Calls edge, shuts the server down; the results are merged, deduplicated, and stored in graph_store. Production wiring in crates/gyre-server/src/graph_extraction.rs::do_extract: Pass 1 extractors run via run_all_extractors (Rust/Go/Python/TypeScript), nodes/edges are diffed and persisted to the GraphPort; then Step 7 spawns Pass 2 in a background tokio task that runs extract_and_persist_call_graph with SubprocessCallGraphExtractor, resolves raw qualified-name edges to Pass 1 node IDs, deduplicates against existing live Calls edges and within the run, persists new edges via graph_store.create_edge, and reconciles (soft-deletes) stale Calls edges. Reference sites are resolved to the enclosing function by each driver (LSP symbol-range containment), matching the specced merge.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "// Fire-and-forget: Pass 2 never blocks the push response.",
            "detail": "Step 7 of do_extract: after Pass 1 nodes/edges are persisted, Pass 2 runs extract_and_persist_call_graph in a spawned background task with the SubprocessCallGraphExtractor, merging the type-checker Calls edges into the same graph_store afterwards."
          },
          {
            "file": "crates/gyre-domain/src/call_graph_resolve.rs",
            "quote": "if existing_calls.contains(&key) || !seen.insert(key) {",
            "detail": "Merge deduplication: a resolved (caller, callee) pair is skipped when it is already a live Calls edge (existing_calls set built from the passed-in Pass 1 edge set) or already emitted in this run (seen set) — the specced 'Deduplicate edges' step."
          },
          {
            "file": "crates/gyre-domain/src/call_graph_resolve.rs",
            "quote": "metadata: Some(gyre_common::call_graph::PASS2_EDGE_METADATA.to_string()),",
            "detail": "Merged Pass 2 edges are constructed as GraphEdge values (provenance-tagged as semantic Pass 2) and persisted through the GraphPort in extract_and_persist_call_graph — the specced 'Store in graph_store' step."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "\"knowledge graph extraction (pass 1) complete\"",
            "detail": "Pass 1 completes and the graph is updated before Pass 2 begins, matching the specced ordering (fast syntax pass first, slower semantic pass second)."
          }
        ]
      },
      {
        "id": "0072f88ee194864c284f52d1",
        "decision": "context",
        "requirement_quote": "The inverted approach — \"for each known definition, who references it?\" — leverages the type checker's full resolution.",
        "detail": "Rationale section: explains why the find-references inversion is preferred over walking call bodies (type information is required for resolution) and lists the patterns it catches (trait dispatch, generics, re-exports, closures, macros, decorators). It is design justification for the mechanism; the mechanism itself is normatively specified and audited under Architecture and Per-Language Implementation. The shipped drivers do follow the inverted model: the Python, TypeScript, and Rust drivers each send textDocument/references for each Pass-1-visible definition and attribute reference sites to the enclosing function, and Go uses the spec-prescribed one-pass CHA API which resolves interface dispatch internally.",
        "evidence": [
          {
            "file": "scripts/python-callgraph-pyright.mjs",
            "quote": "// 4. For each Pass-1-visible definition, send textDocument/references at the",
            "detail": "The Python driver literally implements the inverted model the section rationalizes: for each known definition, ask the type checker who references it, then map reference sites to the enclosing definition."
          }
        ]
      },
      {
        "id": "a6dec9d9a49f5da91a31f520",
        "decision": "context",
        "requirement_quote": "### Per-Language Implementation\n\n#### Rust: `rust-analyzer`",
        "detail": "Parent heading with no body text of its own; its four child sections (Rust, Python, Go, TypeScript) carry the per-language requirements and are each audited separately below. Counting their requirements again here would double-count.",
        "evidence": []
      },
      {
        "id": "121f4785bba4c31b9e05d84b",
        "decision": "satisfied",
        "requirement_quote": "**Recommendation:** Start with LSP subprocess (simpler integration, test the approach), migrate to library if performance matters.",
        "detail": "The section allows library (ra_ap_* crates) or LSP subprocess, recommending the LSP subprocess first. Production implements the recommended Option B: crates/gyre-adapters/src/call_graph_rust.rs is a native rust-analyzer LSP client — it discovers the rust-analyzer binary (RUST_ANALYZER_BIN override, then PATH), performs the initialize handshake, didOpens every .rs file, sends textDocument/references at each Pass 1 function definition position, resolves the enclosing function at each reference site, emits CallEdges, and shuts the server down. It honors the section's prerequisite contract: when the binary is missing the run degrades to zero edges with a warning, never an error (local dev assumes it installed). The whole extraction is bounded by INIT_TIMEOUT/REQUEST_TIMEOUT/OVERALL_BUDGET so a wedged server cannot stall Pass 2. No ra_ap_* library dependency was added, matching the recommendation.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "pub async fn extract_rust_call_edges(repo_path: &Path) -> Result<Vec<CallEdge>> {",
            "detail": "Public entry point of the rust-analyzer LSP subprocess client (Option B); the module header documents that no ra_ap_* crates are involved and the type checker runs out of process over JSON-RPC/stdio."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "extract_with_binary(repo_path, find_rust_analyzer()).await",
            "detail": "Binary discovery honors the prerequisite contract: find_rust_analyzer checks RUST_ANALYZER_BIN then PATH; a missing binary degrades to Ok(vec![]) with a warning instead of erroring."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "| Rust       | native rust-analyzer LSP client (`call_graph_rust`) |",
            "detail": "The adapter's tool table records the production mechanism for Rust: the native rust-analyzer LSP client, exactly the specced LSP-subprocess approach."
          }
        ]
      },
      {
        "id": "cc8681cc2a31adf3959f24d1",
        "decision": "satisfied",
        "requirement_quote": "**Recommendation:** `pyright` via LSP. It handles complex Python patterns (metaclasses, decorators, type: ignore) better than jedi.",
        "detail": "Production implements the recommendation: pyright via LSP. scripts/python-callgraph.sh is the wrapper the adapter invokes; it locates pyright-langserver via PYRIGHT_LANGSERVER_BIN or PATH and execs scripts/python-callgraph-pyright.mjs, which starts pyright-langserver --stdio, didOpens every .py file, sends textDocument/references for each Pass-1-visible definition, skips import-binding sites, resolves the enclosing definition for each reference (module-level references attribute to the module), and emits deduplicated Calls edges as JSON. Prerequisite handling matches the section: a missing pyright prints a warning to stderr and an empty JSON array so Pass 2 degrades gracefully. The adapter's Python path dispatches to this wrapper (crates/gyre-adapters/src/call_graph.rs), and the pinned pyright is shipped in the server container image.",
        "evidence": [
          {
            "file": "scripts/python-callgraph.sh",
            "quote": "# using the pyright language server (pyright-langserver --stdio).",
            "detail": "The Python Pass 2 wrapper drives pyright as an LSP server over stdio — the section's recommended mechanism (not jedi, not --outputjson batch mode)."
          },
          {
            "file": "scripts/python-callgraph-pyright.mjs",
            "quote": "// 4. For each Pass-1-visible definition, send textDocument/references at the",
            "detail": "The driver sends textDocument/references per definition and maps reference sites to Calls edges (step 5 of its documented approach), implementing the find-references model for Python."
          },
          {
            "file": "scripts/python-callgraph.sh",
            "quote": "pyright-langserver not found on PATH",
            "detail": "Prerequisite contract: when pyright is not installed the wrapper warns and emits '[]' with exit 0, so Pass 2 degrades instead of failing the push — matching the section's Prerequisites note that pyright must be available."
          }
        ]
      },
      {
        "id": "119d9bb96be92bba661994f5",
        "decision": "satisfied",
        "requirement_quote": "**Recommendation:** Build a small Go binary (`gyre-go-callgraph`) that outputs JSON. The Gyre extractor shells out to it.",
        "detail": "Production follows the recommendation exactly. scripts/go-callgraph/main.go is a small Go binary using golang.org/x/tools/go/packages + ssa + callgraph/cha (with ssa.InstantiateGenerics) that computes the complete call graph in one cha.CallGraph(prog) call, filters edges to project-local functions via the module path, deduplicates, and prints a JSON array of {from,to} objects — the documented output contract. The compiled binary is committed at scripts/go-callgraph/go-callgraph, the adapter shells out to it (find_go_callgraph_binary: GO_CALLGRAPH_BIN override, workspace-relative scripts/go-callgraph/go-callgraph, then gyre-go-callgraph/go-callgraph on PATH), and the server container image installs it as /usr/local/bin/gyre-go-callgraph with GO_CALLGRAPH_BIN set — the specced binary name. Prerequisite (Go toolchain) is honored: the image ships the Go toolchain because packages.Load shells out to `go list`.",
        "evidence": [
          {
            "file": "scripts/go-callgraph/main.go",
            "quote": "cg := cha.CallGraph(prog)",
            "detail": "The complete call graph (including interface dispatch) is computed via the specced golang.org/x/tools/go/callgraph CHA API in a single call, after packages.Load and SSA build."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "fn find_go_callgraph_binary() -> Option<PathBuf> {",
            "detail": "The Gyre extractor shells out to the Go binary: discovery order is GO_CALLGRAPH_BIN, the workspace's scripts/go-callgraph/go-callgraph, then gyre-go-callgraph/go-callgraph on PATH — the specced 'shell out from Go extractor' integration."
          },
          {
            "file": "Dockerfile",
            "quote": "COPY scripts/go-callgraph/go-callgraph /usr/local/bin/gyre-go-callgraph",
            "detail": "The committed CHA binary is installed into the server image under the specced name gyre-go-callgraph, alongside the Go toolchain copied from the golang image (the section's 'Go toolchain installed' prerequisite)."
          }
        ]
      },
      {
        "id": "dfdc019a5470098226a1404e",
        "decision": "satisfied",
        "requirement_quote": "**Recommendation:** LSP via `typescript-language-server`. Same pattern as Rust/Python.",
        "detail": "Production implements the recommendation: scripts/ts-callgraph.mjs starts typescript-language-server --stdio as an LSP subprocess, didOpens every .ts/.tsx/.js/.jsx source file (.svelte skipped with a warning since tsserver cannot analyze it), requests textDocument/documentSymbol per file, sends textDocument/references for every top-level function/class/interface definition, resolves the enclosing function/method by symbol-range containment, deduplicates, emits the JSON edge array, and shuts the server down — the same pattern as Rust/Python. Prerequisites are honored with graceful degradation: TS_CALLGRAPH_SERVER/TS_CALLGRAPH_TSSERVER overrides, node_modules/.bin and PATH discovery, and a missing server prints a warning plus '[]' with exit 0. The adapter's TypeScript path dispatches to this driver.",
        "evidence": [
          {
            "file": "scripts/ts-callgraph.mjs",
            "quote": "// 2. Start `typescript-language-server --stdio` as an LSP subprocess.",
            "detail": "The TypeScript driver uses the recommended mechanism: typescript-language-server over LSP (wrapping tsserver), the same find-references pattern as the Rust and Python drivers."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "Language::TypeScript => match find_script(\"ts-callgraph.mjs\") {",
            "detail": "The Pass 2 adapter dispatches TypeScript extraction to the typescript-language-server driver script, wiring the specced tool into the extraction pipeline."
          }
        ]
      },
      {
        "id": "28caa783e92302a9d90c86c0",
        "decision": "satisfied",
        "requirement_quote": "Pass 1 and Pass 2 run sequentially but Pass 2 is non-blocking — the graph is usable after Pass 1 with partial call data, and becomes complete when Pass 2 finishes.",
        "detail": "Production wiring matches the pipeline diagram. Push/sync triggers: the git push path (git_http.rs) and the mirror sync path (mirror_sync.rs) both invoke extract_and_store_graph after the reference update. Pass 1 (run_all_extractors: tree-sitter for Go/Python/TypeScript, syn for Rust) extracts nodes plus Contains/Implements/basic-Calls edges and persists them with the incremental diff before Pass 2 starts, so the graph is usable immediately with partial call data. Pass 2 then runs in a fire-and-forget tokio::spawn background task: the language type checkers produce the complete Calls edges, which are resolved, deduplicated, persisted, and stale Calls edges reconciled when done. All extraction errors are logged and swallowed — extraction never fails a push.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "crate::graph_extraction::extract_and_store_graph(",
            "detail": "Push trigger: after a successful git push (reference update), the push handler invokes the extraction pipeline — the pipeline diagram's 'Push / Sync' entry point."
          },
          {
            "file": "crates/gyre-server/src/mirror_sync.rs",
            "quote": "crate::graph_extraction::extract_and_store_graph(",
            "detail": "Sync trigger: mirror synchronization also runs the extraction pipeline, covering the 'Sync' half of the trigger."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "\"knowledge graph extraction (pass 1) complete\"",
            "detail": "Pass 1 completes and the graph store is updated (nodes upserted, edges diffed, delta recorded) before Pass 2 begins — the graph is usable after Pass 1 with partial call data."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "// Fire-and-forget: Pass 2 never blocks the push response.",
            "detail": "Pass 2 is non-blocking: it runs in a spawned background task that merges the complete Calls edges into the graph store when done, exactly the specced sequential-but-non-blocking behavior."
          }
        ]
      },
      {
        "id": "1c20952c55f116d6b1839a84",
        "decision": "gap",
        "requirement_quote": "| `Implements` | Syntax (impl blocks, class inheritance) | 1 | ~95% (misses implicit Go interfaces) |",
        "detail": "The Edge Types table defines what the extraction pipeline produces per edge type, with stated sources and coverage. Verified against all four Pass 1 extractors: Contains (all four extractors), Calls (Pass 2 type checkers for all four languages plus Pass 1 syntax Calls for Rust/TypeScript), FieldOf (all four), GovernedBy (all four, from spec comments), and DependsOn (Rust from Cargo.toml [dependencies]; Go package-level edges derived from import paths with go.mod supplying the module path) are produced. Three rows are not met by the code at this base: (1) Implements — the row's source is 'Syntax (impl blocks, class inheritance)': Rust emits Implements for `impl Trait for Type`, but class inheritance produces no Implements edges in either Python or TypeScript. Python's extract_class reads the class name, emits a Type node and a Contains edge, and walks the body for fields/methods — it never reads the class definition's superclass argument_list, so `class Admin(User)` yields no Implements edge. TypeScript's emit_class reads only the name and body fields — the tree-sitter heritage clause (`extends`/`implements`) is never inspected, and the file contains zero occurrences of 'heritage', 'extends', 'implements', or EdgeType::Implements, so `class X extends Y` and `class X implements Y` produce no Implements edges. (2) Returns — the row claims 'Syntax (function signatures), 100%': only the Rust extractor emits Returns edges (deferred_returns/resolve_returns from syn signatures); the Go, Python, and TypeScript extractors contain no EdgeType::Returns, so their function signatures produce no Returns edges. (3) RoutesTo — 'Syntax (framework-specific patterns)': Rust (handler annotations), Go (net/http route patterns), and TypeScript (Express/Next.js detection) emit RoutesTo, but Python extracts Flask/FastAPI-decorated functions as Endpoint nodes with only a Contains edge from the parent (extract_endpoint_node) — no RoutesTo edge is ever created for Python (zero EdgeType::RoutesTo in python_extractor.rs). Cross-plan ownership note: the realized-model.md staged plan's graph-edge-emitters task commits only to the TypeScript `implements` keyword — not `extends`, not Python superclasses, not Returns for Go/Python/TypeScript, not Python RoutesTo; none of the other active staged plans touch these rows (grep across /tmp/stage/other-plans for heritage/superclass/EdgeType::Returns returns no commitment), so after that plan executes the Implements row remains unmet for TS class inheritance (extends) and Python inheritance, and the Returns/RoutesTo rows remain unmet for the non-Rust languages.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "let edge = self.ctx.make_edge(EdgeType::Implements, from_id, to_id);",
            "detail": "The Implements row is met for its Rust half: `impl Trait for Type` blocks produce Implements edges (impl → trait). The unmet half is class inheritance in Python and TypeScript."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "fn extract_class(",
            "detail": "Inspected the entire extract_class function: it resolves the class name, creates the Type node, emits a Contains edge (module → class), and walks the body for annotated fields, __init__ assignments, methods, and decorated definitions. It never reads the class_definition's argument_list (superclass list); grep for 'superclass', 'inheritance', 'bases', and class-related 'argument_list' in this file returns nothing — `class Admin(User)` produces no Implements edge to the User Type node."
          },
          {
            "file": "crates/gyre-domain/src/typescript_extractor.rs",
            "quote": "// Extract fields from class body (public_field_definition / property_declaration).",
            "detail": "emit_class reads only the class name and body fields; the tree-sitter heritage clause (`extends`/`implements`) is never read. Grep for 'heritage', 'extends', 'implements', and EdgeType::Implements in typescript_extractor.rs returns zero matches — neither `class Admin extends User` nor `class X implements Y` produces an Implements edge."
          },
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "edge_type: EdgeType::Returns,",
            "detail": "The Returns row's only producer: resolve_returns resolves deferred (function, return-type) pairs collected from Rust signatures into Returns edges. The Go, Python, and TypeScript extractors contain no EdgeType::Returns anywhere (verified by grep), so the row's claimed 'Syntax (function signatures)' source produces nothing for three of the four languages."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "let meta = serde_json::json!({ \"path\": route_path }).to_string();",
            "detail": "extract_endpoint_node (Flask/FastAPI route decorators, Click commands, Celery tasks) creates the Endpoint node and a single Contains edge from the parent module/class — no RoutesTo edge is emitted (zero EdgeType::RoutesO matches in python_extractor.rs), so the RoutesTo row has no Python producer while Go/TypeScript/Rust all have one."
          }
        ]
      },
      {
        "id": "8f23787a005d9d190346d3cb",
        "decision": "satisfied",
        "requirement_quote": "With complete call edges, the LLM can answer:",
        "detail": "The section requires that the graph plus its query surface lets the LLM perform blast-radius, coupling, dead-code, and spec-change-impact analyses over Calls/Implements/GovernedBy edges. The production MCP tool surface and view-query grammar provide this over real stored Pass 1+Pass 2 edges: graph_summary reports node/edge counts, top functions by calls, per-pass Calls edge counts, and call_graph_complete from the durable Pass 2 completion marker; graph_query_dryrun resolves view queries against the live graph and warns 'Call graph incomplete — blast radius may be underreported' when a calls-traversing query runs before Pass 2 has completed (under-claiming rather than over-claiming); graph_nodes and graph_edges expose the node and edge sets. The specced analyses map to real query primitives: blast radius — focus scope traversing `calls` edges (plus $callers/$callees/$implementors computed expressions); coupling — incoming_calls/outgoing_calls heat metrics computed by counting live Calls edges per node; dead code — function nodes with no inbound Calls derivable from graph_nodes cross-referenced with graph_edges (RoutesTo-marked endpoints excluded), with test-reachability computed through Calls-edge chains; spec-change impact — $governed_by(...) computed expression and governed_by edge traversal to nodes linked to a spec.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Get a condensed summary of a repo's knowledge graph",
            "detail": "The graph_summary MCP tool description: node/edge counts, top functions by calls, and call_graph metrics (call_edges_pass1, call_edges_pass2, call_graph_complete from the durable Pass 2 marker) — the LLM-facing blast-radius/completeness surface."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "Warns 'Call graph incomplete — blast radius may be underreported'",
            "detail": "graph_query_dryrun's blast-radius guard: calls-traversing queries warn when Pass 2 has not completed, so LLM blast-radius answers derived from the query surface cannot silently over-claim."
          },
          {
            "file": "crates/gyre-common/src/view_query.rs",
            "quote": "\"$governed_by(\",",
            "detail": "The view-query grammar's computed expressions include $governed_by (spec-change impact), alongside $callers/$callees/$implementors (blast radius) — the query primitives for the section's four LLM analyses."
          },
          {
            "file": "crates/gyre-domain/src/view_query_resolver.rs",
            "quote": "/// Count incoming Calls edges to a node.",
            "detail": "Coupling analysis is a real resolver primitive: incoming and outgoing Calls edge counts back the incoming_calls/outgoing_calls heat metrics over live stored edges."
          }
        ]
      },
      {
        "id": "7e49b802ebd53a6b4bfabf0e",
        "decision": "satisfied",
        "requirement_quote": "All acceptable for a background job triggered on push/sync. Pass 1 (syntax) returns instantly; Pass 2 (semantic) completes within a minute even for large projects.",
        "detail": "The section's per-language numbers are explicitly approximate estimates (~3-5s loads, ~20s/~30s totals); the implementable requirement is that Pass 2 runs as a bounded background job whose duration is observable against those targets. Production satisfies this: Pass 2 runs in the fire-and-forget background task (never blocking the push), each tool is latency-bounded (60s TOOL_TIMEOUT for the script tools; the rust-analyzer client is bounded by INIT_TIMEOUT/REQUEST_TIMEOUT and a 180s OVERALL_BUDGET that returns partial edges rather than stalling — a deliberate anti-wedge cap looser than the spec's ~1 minute estimate, since a wedged server must never stall the background task), per-language durations are observed on the gyre_graph_pass2_duration_seconds Prometheus histogram, and each run persists a durable CallGraphRunRecord with the total duration so regressions against the §9 targets are visible. Note made for transparency, not judged a contradiction: the Rust hard budget (180s) exceeds the spec's 'within a minute' estimate; the spec's figures are estimates and the code logs observed durations against them while degrading safely at the bound.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/metrics.rs",
            "quote": "pub fn observe_pass2_duration(&self, language: &str, seconds: f64) {",
            "detail": "Per-language Pass 2 durations are observed on the gyre_graph_pass2_duration_seconds histogram, making the spec's per-language performance targets checkable against observed numbers."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "/// Maximum time a Pass 2 tool may run before it is killed and Pass 2 degrades",
            "detail": "Pass 2 tool runs are bounded (TOOL_TIMEOUT, killed and degraded to no additional edges), so the background job cannot run unbounded — the section's 'background job' requirement with a hard safety bound."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "// Fire-and-forget: Pass 2 never blocks the push response.",
            "detail": "Pass 2 executes as a background job triggered on push/sync, matching the section's framing of the performance budget as acceptable-for-a-background-job."
          }
        ]
      },
      {
        "id": "268a894a66a410cacc9d37df",
        "decision": "satisfied",
        "requirement_quote": "**Phase 1: Go** (simplest — `callgraph` package gives complete graph in one call)",
        "detail": "All four phases shipped. Phase 1 (Go): the gyre-go-callgraph CHA binary exists at scripts/go-callgraph (source + committed binary), the adapter shells out to it, and it is installed in the server image — validated by the adapter's Go integration tests. Phase 2 (Rust): the native rust-analyzer LSP subprocess client (call_graph_rust.rs) with find-references queries (task-073). Phase 3 (Python): the pyright LSP driver (python-callgraph.sh + python-callgraph-pyright.mjs) validated against real and stub pyright servers (task-074). Phase 4 (TypeScript): the typescript-language-server driver (ts-callgraph.mjs) validated against a recorded-protocol stub and real-server runs (task-075). Each phase is wired through the same SubprocessCallGraphExtractor dispatch and the shared resolver.",
        "evidence": [
          {
            "file": "scripts/go-callgraph/main.go",
            "quote": "\"golang.org/x/tools/go/callgraph/cha\"",
            "detail": "Phase 1: the Go binary built on the specced callgraph package exists and is shelled out to by the extractor (see the Go per-language section for the full wiring)."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "pub async fn extract_rust_call_edges(repo_path: &Path) -> Result<Vec<CallEdge>> {",
            "detail": "Phase 2: the Rust rust-analyzer LSP integration ships as the native LSP client (task-073), the spec's 'most impactful for Gyre itself' phase."
          },
          {
            "file": "scripts/python-callgraph.sh",
            "quote": "# using the pyright language server (pyright-langserver --stdio).",
            "detail": "Phase 3: the Python pyright LSP integration ships as the wrapper + driver pair (task-074); Phase 4 (TypeScript) ships as scripts/ts-callgraph.mjs (task-075), dispatched by the same adapter (Language::TypeScript => find_script(\"ts-callgraph.mjs\"))."
          }
        ]
      },
      {
        "id": "9476a77de45d2786274c547f",
        "decision": "gap",
        "requirement_quote": "The extraction host (server or agent container) must have the language toolchains installed:",
        "detail": "The section requires the extraction host to have all four toolchains — rust-analyzer (Rust), pyright (Python), Go toolchain (Go), and node+typescript+typescript-language-server (TypeScript) — and says Docker images should include them. In the current architecture extraction runs server-side only (graph_extraction.rs in gyre-server, invoked from the git push and mirror-sync paths), so the operative extraction host is the server. The shipped server container image (root Dockerfile) includes the Go toolchain, the committed go-callgraph binary (installed as /usr/local/bin/gyre-go-callgraph with GO_CALLGRAPH_BIN set), node, and pinned pyright 1.1.414 with PYRIGHT_LANGSERVER_BIN set — but it contains no rust-analyzer and no typescript-language-server (grep for both in the Dockerfile returns zero matches; the image's own comment still says 'Rust/TS toolchains are task-073/075 scope', stale because tasks 073/075 shipped the driver code without updating the image). Consequence: in the containerized deployment, Rust and TypeScript Pass 2 silently degrade to zero Calls edges (the graceful-degradation contract logs a warning and returns no edges, so no error surfaces and the call graph is silently incomplete for those languages). The Nix-built agent image (docker/gyre-agent/flake.nix) includes none of the four toolchains either (nodejs, bash, curl, git, util-linux, cacert only), though agents do not run extraction in the current architecture. Missing behavior: rust-analyzer and typescript-language-server (+typescript) must be added to the extraction host's container image (or the deployment must otherwise provision them) for the specced prerequisites to hold in the shipped deployment.",
        "evidence": [
          {
            "file": "Dockerfile",
            "quote": "# go-callgraph CHA binary. Rust/TS toolchains are task-073/075 scope.",
            "detail": "The server image's runtime stage comment scopes in only the Go (and later Python) toolchains. Grep confirms zero occurrences of 'rust-analyzer' and 'typescript-language-server' anywhere in the Dockerfile: the Rust (task-073) and TypeScript (task-075) drivers shipped, but their required binaries were never added to the image, so those languages' Pass 2 degrade to zero edges in containerized deployments."
          },
          {
            "file": "Dockerfile",
            "quote": "COPY scripts/python-callgraph.sh scripts/python-callgraph-pyright.mjs /app/scripts/",
            "detail": "For contrast, the Python prerequisites are fully shipped in the image (node, pinned pyright 1.1.414 as pyright-langserver, driver scripts, PYRIGHT_LANGSERVER_BIN), showing the intended pattern that the Rust/TS halves do not follow."
          },
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "pkgs.nodejs_22",
            "detail": "The Nix-built agent image contents (nodejs_22, bashInteractive, curl, git, util-linux, cacert, gyre scripts, node deps): none of the four specced extraction toolchains (rust-analyzer, pyright, Go, typescript-language-server) are included — the spec's 'For Docker agent containers, these should be included in the base image' is unmet for the agent image, though extraction currently runs only on the server."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "extract_with_binary(repo_path, find_rust_analyzer()).await",
            "detail": "The Rust driver resolves the rust-analyzer binary only via RUST_ANALYZER_BIN/PATH discovery; with no rust-analyzer in the server image and no RUST_ANALYZER_BIN set, find_rust_analyzer returns None and the run degrades to zero edges — the prerequisite is genuinely load-bearing, not hypothetical."
          }
        ]
      },
      {
        "id": "a476f16f61d04aa3dea76796",
        "decision": "context",
        "requirement_quote": "**Complements:** OTLP trace capture (`gate_executor.rs` TraceCapture gate) — static graph + dynamic traces = complete picture",
        "detail": "Cross-reference section: declares what this spec implements (system-explorer.md §5 Flow Traces), enables (explorer-canvas.md structural lens, LLM blast-radius queries), depends on (realized-model.md storage, the Pass 1 extractors), and complements (the OTLP TraceCapture gate). No independent implementable requirement beyond the cross-references; the dependencies it names are audited in their own canonical specs. The complement it names does exist in production (the TraceCapture gate runs in gate_executor.rs), and the depends-on storage (GraphPort) and Pass 1 extractors are the ones audited above.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "GateType::TraceCapture => run_trace_capture_gate(&state, &gate, &mr_id, &result_id).await,",
            "detail": "The complement this section names — the OTLP TraceCapture gate — exists in production gate execution, so the static-graph-plus-dynamic-traces relationship is grounded in real code (its own fidelity is owned by the observability specs)."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "1c20952c55f116d6b1839a84",
        "decision": "gap",
        "requirement_quote": "| `Implements` | Syntax (impl blocks, class inheritance) | 1 | ~95% (misses implicit Go interfaces) |",
        "detail": "The Edge Types table defines what the extraction pipeline produces per edge type, with stated sources and coverage. Verified against all four Pass 1 extractors: Contains (all four extractors), Calls (Pass 2 type checkers for all four languages plus Pass 1 syntax Calls for Rust/TypeScript), FieldOf (all four), GovernedBy (all four, from spec comments), and DependsOn (Rust from Cargo.toml [dependencies]; Go package-level edges derived from import paths with go.mod supplying the module path) are produced. Three rows are not met by the code at this base: (1) Implements — the row's source is 'Syntax (impl blocks, class inheritance)': Rust emits Implements for `impl Trait for Type`, but class inheritance produces no Implements edges in either Python or TypeScript. Python's extract_class reads the class name, emits a Type node and a Contains edge, and walks the body for fields/methods — it never reads the class definition's superclass argument_list, so `class Admin(User)` yields no Implements edge. TypeScript's emit_class reads only the name and body fields — the tree-sitter heritage clause (`extends`/`implements`) is never inspected, and the file contains zero occurrences of 'heritage', 'extends', 'implements', or EdgeType::Implements, so `class X extends Y` and `class X implements Y` produce no Implements edges. (2) Returns — the row claims 'Syntax (function signatures), 100%': only the Rust extractor emits Returns edges (deferred_returns/resolve_returns from syn signatures); the Go, Python, and TypeScript extractors contain no EdgeType::Returns, so their function signatures produce no Returns edges. (3) RoutesTo — 'Syntax (framework-specific patterns)': Rust (handler annotations), Go (net/http route patterns), and TypeScript (Express/Next.js detection) emit RoutesTo, but Python extracts Flask/FastAPI-decorated functions as Endpoint nodes with only a Contains edge from the parent (extract_endpoint_node) — no RoutesTo edge is ever created for Python (zero EdgeType::RoutesTo in python_extractor.rs). Cross-plan ownership note: the realized-model.md staged plan's graph-edge-emitters task commits only to the TypeScript `implements` keyword — not `extends`, not Python superclasses, not Returns for Go/Python/TypeScript, not Python RoutesTo; none of the other active staged plans touch these rows (grep across /tmp/stage/other-plans for heritage/superclass/EdgeType::Returns returns no commitment), so after that plan executes the Implements row remains unmet for TS class inheritance (extends) and Python inheritance, and the Returns/RoutesTo rows remain unmet for the non-Rust languages.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "let edge = self.ctx.make_edge(EdgeType::Implements, from_id, to_id);",
            "detail": "The Implements row is met for its Rust half: `impl Trait for Type` blocks produce Implements edges (impl → trait). The unmet half is class inheritance in Python and TypeScript."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "fn extract_class(",
            "detail": "Inspected the entire extract_class function: it resolves the class name, creates the Type node, emits a Contains edge (module → class), and walks the body for annotated fields, __init__ assignments, methods, and decorated definitions. It never reads the class_definition's argument_list (superclass list); grep for 'superclass', 'inheritance', 'bases', and class-related 'argument_list' in this file returns nothing — `class Admin(User)` produces no Implements edge to the User Type node."
          },
          {
            "file": "crates/gyre-domain/src/typescript_extractor.rs",
            "quote": "// Extract fields from class body (public_field_definition / property_declaration).",
            "detail": "emit_class reads only the class name and body fields; the tree-sitter heritage clause (`extends`/`implements`) is never read. Grep for 'heritage', 'extends', 'implements', and EdgeType::Implements in typescript_extractor.rs returns zero matches — neither `class Admin extends User` nor `class X implements Y` produces an Implements edge."
          },
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "edge_type: EdgeType::Returns,",
            "detail": "The Returns row's only producer: resolve_returns resolves deferred (function, return-type) pairs collected from Rust signatures into Returns edges. The Go, Python, and TypeScript extractors contain no EdgeType::Returns anywhere (verified by grep), so the row's claimed 'Syntax (function signatures)' source produces nothing for three of the four languages."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "let meta = serde_json::json!({ \"path\": route_path }).to_string();",
            "detail": "extract_endpoint_node (Flask/FastAPI route decorators, Click commands, Celery tasks) creates the Endpoint node and a single Contains edge from the parent module/class — no RoutesTo edge is emitted (zero EdgeType::RoutesO matches in python_extractor.rs), so the RoutesTo row has no Python producer while Go/TypeScript/Rust all have one."
          }
        ]
      },
      {
        "id": "9476a77de45d2786274c547f",
        "decision": "gap",
        "requirement_quote": "The extraction host (server or agent container) must have the language toolchains installed:",
        "detail": "The section requires the extraction host to have all four toolchains — rust-analyzer (Rust), pyright (Python), Go toolchain (Go), and node+typescript+typescript-language-server (TypeScript) — and says Docker images should include them. In the current architecture extraction runs server-side only (graph_extraction.rs in gyre-server, invoked from the git push and mirror-sync paths), so the operative extraction host is the server. The shipped server container image (root Dockerfile) includes the Go toolchain, the committed go-callgraph binary (installed as /usr/local/bin/gyre-go-callgraph with GO_CALLGRAPH_BIN set), node, and pinned pyright 1.1.414 with PYRIGHT_LANGSERVER_BIN set — but it contains no rust-analyzer and no typescript-language-server (grep for both in the Dockerfile returns zero matches; the image's own comment still says 'Rust/TS toolchains are task-073/075 scope', stale because tasks 073/075 shipped the driver code without updating the image). Consequence: in the containerized deployment, Rust and TypeScript Pass 2 silently degrade to zero Calls edges (the graceful-degradation contract logs a warning and returns no edges, so no error surfaces and the call graph is silently incomplete for those languages). The Nix-built agent image (docker/gyre-agent/flake.nix) includes none of the four toolchains either (nodejs, bash, curl, git, util-linux, cacert only), though agents do not run extraction in the current architecture. Missing behavior: rust-analyzer and typescript-language-server (+typescript) must be added to the extraction host's container image (or the deployment must otherwise provision them) for the specced prerequisites to hold in the shipped deployment.",
        "evidence": [
          {
            "file": "Dockerfile",
            "quote": "# go-callgraph CHA binary. Rust/TS toolchains are task-073/075 scope.",
            "detail": "The server image's runtime stage comment scopes in only the Go (and later Python) toolchains. Grep confirms zero occurrences of 'rust-analyzer' and 'typescript-language-server' anywhere in the Dockerfile: the Rust (task-073) and TypeScript (task-075) drivers shipped, but their required binaries were never added to the image, so those languages' Pass 2 degrade to zero edges in containerized deployments."
          },
          {
            "file": "Dockerfile",
            "quote": "COPY scripts/python-callgraph.sh scripts/python-callgraph-pyright.mjs /app/scripts/",
            "detail": "For contrast, the Python prerequisites are fully shipped in the image (node, pinned pyright 1.1.414 as pyright-langserver, driver scripts, PYRIGHT_LANGSERVER_BIN), showing the intended pattern that the Rust/TS halves do not follow."
          },
          {
            "file": "docker/gyre-agent/flake.nix",
            "quote": "pkgs.nodejs_22",
            "detail": "The Nix-built agent image contents (nodejs_22, bashInteractive, curl, git, util-linux, cacert, gyre scripts, node deps): none of the four specced extraction toolchains (rust-analyzer, pyright, Go, typescript-language-server) are included — the spec's 'For Docker agent containers, these should be included in the base image' is unmet for the agent image, though extraction currently runs only on the server."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph_rust.rs",
            "quote": "extract_with_binary(repo_path, find_rust_analyzer()).await",
            "detail": "The Rust driver resolves the rust-analyzer binary only via RUST_ANALYZER_BIN/PATH discovery; with no rust-analyzer in the server image and no RUST_ANALYZER_BIN set, find_rust_analyzer returns None and the run degrades to zero edges — the prerequisite is genuinely load-bearing, not hypothetical."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-adapters/src/call_graph.rs",
        "sha256": "71054c64624059526ab79cda3ad34dd2f0ac24d32f806687ba5a5cec176db2d2"
      },
      {
        "file": "scripts/go-callgraph/main.go",
        "sha256": "28c27f3469be430358001df8f477a02209a20591fcecb7b500886b7115956ada"
      },
      {
        "file": "crates/gyre-adapters/src/call_graph_rust.rs",
        "sha256": "10d26967819513a6f88d87d009702775fbfb4606f84dac45dcfdb3aa38d57365"
      },
      {
        "file": "scripts/ts-callgraph.mjs",
        "sha256": "ac68e292b9577a7fa95931ab68c994afaa86dcfb086fcadd75fcc8389c07ad8e"
      },
      {
        "file": "crates/gyre-server/src/graph_extraction.rs",
        "sha256": "1a64bacf9185670f8c577fe0debc0a0fb25db65a52464003071fd876ad1c6ba0"
      },
      {
        "file": "crates/gyre-domain/src/call_graph_resolve.rs",
        "sha256": "23d1aae89b1cfaaef5c4691af7e48883693b521a289bedb57342da29970cda84"
      },
      {
        "file": "scripts/python-callgraph-pyright.mjs",
        "sha256": "524fc92572c1f541a25a676c0a825a7a516fc36d0b49cedb88aa06cab847131e"
      },
      {
        "file": "scripts/python-callgraph.sh",
        "sha256": "1870504213c23b4079a83db6d7381100dc3c9b540eb7f3bfc23db5c9fed1badc"
      },
      {
        "file": "Dockerfile",
        "sha256": "f6ce5b5ec90a5c42bd6ba215c442b5bb55f33aa53204e95031fa551db57905dc"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "fd5036bd1272469fc55a2b304ac10a7db63c95a6d5f546b37a3c2c6c86307149"
      },
      {
        "file": "crates/gyre-server/src/mirror_sync.rs",
        "sha256": "15cb67a582493a14a61263993e9b59018a237727a141988043870476c798f577"
      },
      {
        "file": "crates/gyre-domain/src/rust_extractor.rs",
        "sha256": "fdb82ae7cba34d5c1989f23953ebee8ddba1929b528a7458f27464384f23a76e"
      },
      {
        "file": "crates/gyre-domain/src/python_extractor.rs",
        "sha256": "38ee8a4119eb4b0e1ed2024d428571209cf7b12f994ee73a2cdfe0b394162d2d"
      },
      {
        "file": "crates/gyre-domain/src/typescript_extractor.rs",
        "sha256": "f96e17530cb9fb010b161d9d1362082ee01ad13b0ffd7e4ba2b6ce52ba53f4b9"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "7172cd8e5748f5baa065ddca60b1fc8f45d369ed891767deb436ad0a9949c23e"
      },
      {
        "file": "crates/gyre-common/src/view_query.rs",
        "sha256": "4fc2f7f0a76c096e3f81ec3a9a8ddf38f628f2da1dd4cf43cb165be8c152667f"
      },
      {
        "file": "crates/gyre-domain/src/view_query_resolver.rs",
        "sha256": "a30f081298cbc3adbec3d215a785bd55a05f056645efa91ddba8cdd86aafdd35"
      },
      {
        "file": "crates/gyre-server/src/metrics.rs",
        "sha256": "9421cb80d2ab16086895c835a70be53a9cef2e7e1b6cdca8dc1bda02dffdc401"
      },
      {
        "file": "docker/gyre-agent/flake.nix",
        "sha256": "d714ef1b0493604998e58b1865fd68de2875105f0521dc63798229f248351b10"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "c4b7e05809c8be5cd04d754fbcd53619bb8832ac29c355222a5c031972ac1ef5"
      }
    ]
  },
  "plan": {
    "audit": "865fa9bc6710ac1f252a52156238c4a60a16bad34866aa7d3d0f9a162a587290",
    "tasks": [
      {
        "key": "edge-rows-inheritance-returns-routesto",
        "title": "Close the unmet Edge Types table rows: class-inheritance Implements (Python superclasses, TypeScript extends), Returns for Go/Python/TypeScript, and Python RoutesTo",
        "covers": [
          "1c20952c55f116d6b1839a84"
        ],
        "required_behavior": "The 'Edge Types Produced' table (specs/system/lsp-call-graph.md) names 'Syntax (impl blocks, class inheritance)' as the source of the Implements row, 'Syntax (function signatures)' as the 100%-coverage source of the Returns row, and 'Syntax (framework-specific patterns)' as the source of the RoutesTo row. Only the Rust extractor currently meets these rows for class inheritance and Returns, and (with Go/TS) RoutesTo. Close the remaining unmet parts in the Pass 1 extractors, all in gyre-domain (hexagonal boundary: no adapters/infrastructure imports).\n\n(1) Implements - class inheritance. Python: extract_class (python_extractor.rs, currently reads only the class name, body fields/methods) must read the class_definition's superclass list (tree-sitter-python superclasses field / argument_list) so `class Admin(User)` emits an Implements edge from the Admin Type node to the User node. For each base name: resolve through the run-scoped name index; because extraction is file-by-file and the base class may be extracted after the derived class, resolution must be order-independent (collect (class_id, base_name) pairs during the pass and resolve them after all files are extracted, mirroring the Rust extractor's deferred_returns/resolve_returns pattern); resolution tries an exact qualified name, then a suffix match. No fabricated targets: a base that resolves to no known node (stdlib/external import, generic marker like Generic) yields no edge. Multiple inheritance emits one edge per resolvable base.\n\nTypeScript: emit_class (typescript_extractor.rs, currently reads only name and body) must inspect the class heritage clause and emit an Implements edge from the class Type node to the base-class node for `extends Base` heritage, resolved through the same run-scoped index with the same order-independent deferred resolution and no-fabricated-targets rules; it may share the resolution machinery with the Python superclass work. Only plain identifier bases that resolve to a known node need edges (expression bases like mixin calls resolve to nothing and yield no edge).\n\nCROSS-PLAN OWNERSHIP BOUNDARY (per review feedback 1c20952c55f116d6b1839a84:ts-extends-implements-unowned): the TypeScript `implements Interface` clause is owned SOLELY by the realized-model.md staged plan's graph-edge-emitters task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json), whose required_behavior item (4) and acceptance criterion commit to exactly the `implements` keyword - not `extends`. This task owns the `extends` clause and MUST NOT parse the `implements` clause or emit TS Implements edges from it, so the two plans never duplicate edge emission for the same clause. After both plans execute, the Implements row is met for Rust impl blocks, Python class inheritance, and both TS heritage clauses.\n\n(2) Returns - function signatures. The Go, Python, and TypeScript extractors must emit Returns edges (function/method node -> return-type node) from syntax, following the Rust extractor's deferred_returns/resolve_returns pattern (rust_extractor.rs): defer (function_id, return_type_name) during the pass; resolve after all files via the run-scoped name index (exact match, then suffix match); skip unresolved names (primitives like string/int/void, external types) with no fabricated edge; dedup (function, type) pairs. Go parses the function_declaration/method_declaration result; Python parses the function_definition's return annotation; TypeScript parses the return type annotation.\n\n(3) RoutesTo - framework patterns. The Python extractor must emit a RoutesTo edge for Flask/FastAPI route-decorated functions, mirroring the TS/Rust/Go pattern: the route Endpoint node gets a RoutesTo edge to the handler function node (metadata carrying path and method), with deferred resolution when the handler node is not yet extracted (extract_endpoint_node at python_extractor.rs:694 currently emits only the Endpoint node and a Contains edge; the method parameter is unused). The handler function node must still be emitted as a Function node so the edge has a real target.\n\nAll new edges flow through the existing make_edge/stable-ID machinery and persist via GraphPort like the extractors' existing edges; extraction must remain error-free on files without these constructs.",
        "acceptance": [
          "A Python fixture with `class Admin(User):` yields an Implements edge from the Admin Type node to the User node when extracted by the real Python extractor; a fixture where User's file is processed after Admin's file (reversed extraction order) yields the same edge (order-independence); `class Foo(ExternalBase)` where no node named ExternalBase exists yields no Implements edge (no fabricated target).",
          "A TypeScript fixture with `class Admin extends User` yields an Implements edge Admin->User with both extraction orders tested; `class Admin extends Unknown` yields no edge; the task adds no behavior for the `implements` clause (owned by the realized-model plan's graph-edge-emitters task).",
          "Go, Python, and TypeScript fixtures each define a function with a user-defined return type in the same repo; extraction yields a Returns edge from the function node to the return-type node in each language, and a function returning a primitive/unknown type yields no Returns edge.",
          "A Python fixture with `@app.route(\"/api/users\")` above `def list_users(...)` yields an Endpoint node, a handler Function node for list_users, and a RoutesTo edge from the endpoint node to the handler node carrying path/method metadata.",
          "The new edges round-trip through real extraction into the stored graph and are returned by the graph API used by the existing extractor tests (same integration path as Contains/FieldOf edges today), with merge/dedup unchanged.",
          "All existing extractor and graph_extraction tests still pass; scripts/check-arch.sh still passes (new code stays in gyre-domain)."
        ],
        "negative_probe": "Run the real extractors over fixture repos and assert the specific edges above (Python and TS `class Admin(User)`/`class Admin extends User` -> Implements Admin->User in both extraction orders; Go/Python/TS function with user-defined return type -> Returns edge; `@app.route` fixture -> RoutesTo endpoint->handler), and assert the absent-edge cases (unresolvable base, primitive return type). The real defect that must make this fail at base 8546a781: an enumeration of every EdgeType:: construction in the four extractors shows EdgeType::Implements is constructed only at rust_extractor.rs:1160 (impl blocks), EdgeType::Returns only at rust_extractor.rs:845 (deferred_returns/resolve_returns), and EdgeType::RoutesTo in typescript_extractor.rs:299, go_extractor.rs:648, rust_extractor.rs:493 but never in python_extractor.rs - python extract_class never reads the superclass argument_list and typescript emit_class never reads the heritage clause. A reverted build therefore stores zero of the asserted Implements/Returns/RoutesTo edges and the probe finds none; asserting merely that the edge kind exists in EdgeType's string mapping (it does today) proves nothing and is not an acceptable probe.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "server-image-pass2-toolchains",
        "title": "Provision the Pass 2 toolchains in the server container image: rust-analyzer, typescript-language-server + typescript, and the ts-callgraph driver script",
        "covers": [
          "9476a77de45d2786274c547f"
        ],
        "required_behavior": "The Prerequisites section requires the extraction host to have all four language toolchains and says Docker images should include them. Extraction runs server-side (graph_extraction.rs in gyre-server, invoked from git push and mirror-sync), so the operative extraction host is the server container image (root Dockerfile). At base it includes the Go toolchain, the committed go-callgraph binary (GO_CALLGRAPH_BIN), node 22, and pinned pyright 1.1.414 (PYRIGHT_LANGSERVER_BIN) - but no rust-analyzer, no typescript-language-server, and not even the TS driver script, so Rust and TypeScript Pass 2 silently degrade to zero Calls edges in the containerized deployment. Close the gap in the Dockerfile: (1) install a pinned rust-analyzer binary in the runtime image (e.g. a rustup component in a build stage, copied into the final image) and wire it the way the adapters discover it - crates/gyre-adapters/src/call_graph_rust.rs find_rust_analyzer() checks RUST_ANALYZER_BIN then PATH - so set ENV RUST_ANALYZER_BIN (and/or place it on PATH), matching how GO_CALLGRAPH_BIN and PYRIGHT_LANGSERVER_BIN are wired; (2) install typescript-language-server and the typescript package via a pinned npm install in the existing node/pyright stage pattern, and wire them per the discovery order documented in scripts/ts-callgraph.mjs: set ENV TS_CALLGRAPH_SERVER to the typescript-language-server entry point and ENV TS_CALLGRAPH_TSSERVER to the typescript lib dir (node is already in the image); (3) COPY scripts/ts-callgraph.mjs into /app/scripts alongside the already-copied python-callgraph.sh / python-callgraph-pyright.mjs (Dockerfile line 91 copies only the Python drivers; without the driver script the server's find_script('ts-callgraph.mjs') returns None and TS Pass 2 skips even with the language server installed); (4) update the stale comment 'Rust/TS toolchains are task-073/075 scope' (tasks 073/075 shipped the driver code without updating the image) to reflect the shipped state; (5) keep pins explicit (no floating latest tags) consistent with pyright@1.1.414 and golang:1.24. The graceful-degradation contract itself (warn + zero edges, never fail a push) must not change; the fix makes the toolchains present so degradation no longer fires for Rust/TS in the shipped image. The Nix-built agent image (docker/gyre-agent/flake.nix) is out of scope: agents do not run extraction in the current architecture. Do not alter the Pass 2 driver code or the adapters' discovery logic - the image must satisfy the existing discovery contract.",
        "acceptance": [
          "docker build of the root Dockerfile succeeds, and the built runtime image contains a working rust-analyzer binary at the RUST_ANALYZER_BIN path and a working typescript-language-server plus typescript at the TS_CALLGRAPH_SERVER / TS_CALLGRAPH_TSSERVER paths (verifiable by docker run <image> <binary> --version).",
          "The image contains /app/scripts/ts-callgraph.mjs (and still the Python drivers and gyre-go-callgraph), so the server's find_script discovery resolves all three Pass 2 drivers inside the container.",
          "Running Rust Pass 2 inside the built image (extract_call_edges on a fixture Rust repo with at least one known cross-function call site, via the server's push-triggered extraction or the SubprocessCallGraphExtractor entry point) returns the expected non-zero Calls edges - not the zero-edge degradation result.",
          "Running TypeScript Pass 2 inside the built image on a fixture TS repo with a known call site likewise returns non-zero Calls edges.",
          "The Dockerfile no longer claims Rust/TS toolchains are out of scope; all toolchain additions are pinned; the image still builds with the non-root user and existing Go/pyright wiring unchanged."
        ],
        "negative_probe": "Build the server image and run Rust and TypeScript Pass 2 extraction inside it against fixture repos each containing one known call site, asserting the expected Calls edge appears in each result. The real defect that must make this fail at base 8546a781: the Dockerfile installs no rust-analyzer and no typescript-language-server (grep for both returns only the stale 'task-073/075 scope' comment at line 73) and never copies scripts/ts-callgraph.mjs (line 91 copies only the Python drivers), so inside the container find_rust_analyzer() returns None (call_graph_rust.rs logs 'rust-analyzer binary not found' and returns no edges) and TS Pass 2 either skips at find_script or warns 'typescript-language-server not found' (ts-callgraph.mjs) - zero Calls edges for both languages with no error surfaced. A reverted build therefore produces zero edges and the probe fails; grepping the Dockerfile for toolchain names without executing the drivers in the built image is not an acceptable probe.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "edge-rows-inheritance-returns-routesto": "task-322",
    "server-image-pass2-toolchains": "task-323",
    "_publication": "task-324",
    "_base": "52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
