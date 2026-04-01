# LSP-Powered Call Graph Extraction

> **Status: Draft.** Depends on: `realized-model.md` (knowledge graph), `explorer-canvas.md` (visualization). Implements: `vision.md` Principle 5 (Feedback Loop), `system-explorer.md` §5 (Flow Traces).

## Problem

The current extractors (tree-sitter for Python/Go/TypeScript, syn for Rust) parse syntax to find declarations and emit `Contains`, `Implements`, and basic `Calls` edges. But call resolution is incomplete — only ~3% of actual calls resolve because:

1. **Cross-module calls** require following `use`/`import` statements to resolve aliases
2. **Trait/interface dispatch** — `state.tasks.find_by_id()` requires knowing the receiver type to find the target
3. **Generics/type parameters** — `impl<T: TaskPort>` means the target depends on monomorphization
4. **Re-exports** — `pub use gyre_common::Id` means `Id::new()` resolves through the re-export chain
5. **Dynamic dispatch** — Python's duck typing, Go's implicit interfaces, TypeScript's structural typing

Without complete call edges:
- The structural lens can't show full causal flow ("what happens when X")
- The LLM can't answer "what is the blast radius of this change"
- Untested paths are invisible — exactly the blind spots humans need to see
- Cross-boundary coupling is unmeasurable

## Solution: Delegate to Language Type Checkers

Each language has a type checker that already solves call resolution completely. Instead of reimplementing type resolution in our extractors, delegate to the tools designed for it.

| Language | Tool | Mechanism |
|---|---|---|
| Rust | `rust-analyzer` | `ra_ap_ide::Analysis::find_all_refs()` or LSP `textDocument/references` |
| Python | `pyright` | `pyright --outputjson` or LSP `textDocument/references` |
| Go | `golang.org/x/tools/go/callgraph` | Direct API — produces the complete call graph in one pass |
| TypeScript | TypeScript compiler API | `ts.findAllReferences()` via `tsc` programmatic API |

### Architecture

```
Push/sync triggers extraction:
  1. First pass: tree-sitter/syn extracts declarations (nodes)
     - Types, functions, interfaces, endpoints, modules
     - Contains, Implements edges
     - This is fast (milliseconds) and runs first

  2. Second pass: LSP/type-checker extracts calls (edges)
     - Start the language's analysis server
     - For each function/method node in the graph:
       - Ask: "find all references to this definition"
       - For each reference: resolve the enclosing function → emit Calls edge
     - Shut down the server
     - This is slower (seconds to tens of seconds) but complete

  3. Merge: combine first-pass nodes/edges with second-pass Calls edges
     - Deduplicate edges
     - Store in graph_store
```

### Why "Find References" Instead of "Walk Bodies"

The current approach walks function bodies looking for call expressions, then tries to resolve the callee. This fails because resolution requires type information.

The inverted approach — "for each known definition, who references it?" — leverages the type checker's full resolution. It catches:
- Trait method calls through dynamic dispatch
- Generic instantiations
- Re-exported symbols
- Closure captures
- Macro-generated calls (Rust)
- Decorator-wrapped calls (Python)
- Monkey-patched methods (Python/JS — to the extent the type checker sees them)

### Per-Language Implementation

#### Rust: `rust-analyzer`

Use `rust-analyzer` as a library (the `ra_ap_*` crates) or via LSP:

**Option A: Library (preferred)**
```rust
// In gyre-domain or a new gyre-analysis crate
use ra_ap_ide::{Analysis, FileId, ReferenceSearchResult};
use ra_ap_project_model::ProjectManifest;

fn extract_call_graph(repo_root: &Path) -> Vec<CallEdge> {
    // Load the Cargo workspace
    let manifest = ProjectManifest::discover(repo_root)?;
    let (host, vfs) = load_workspace(manifest)?;
    let analysis = host.analysis();

    // For each function definition we extracted in pass 1:
    for func_node in &graph_nodes {
        let file_id = vfs.file_id(func_node.file_path)?;
        let offset = find_definition_offset(file_id, func_node.name)?;

        // Find all references to this function
        let refs = analysis.find_all_refs(file_id, offset)?;

        for reference in refs {
            // Resolve the enclosing function at the reference site
            let caller = find_enclosing_function(analysis, reference.file_id, reference.offset)?;
            edges.push(CallEdge { from: caller.node_id, to: func_node.id });
        }
    }
}
```

**Option B: LSP subprocess**
```bash
# Start rust-analyzer in LSP mode, send textDocument/references requests
# Simpler to integrate but slower per-query
```

**Tradeoffs:**
- Library: faster (in-process), but heavy dependency (~50 crates)
- LSP: slower (IPC per request), but no dependency — just needs `rust-analyzer` binary

**Recommendation:** Start with LSP subprocess (simpler integration, test the approach), migrate to library if performance matters.

**Prerequisites:** `rust-analyzer` binary must be available. For Docker agents, include in the container image. For local dev, assume it's installed.

#### Python: `pyright`

Pyright is the fastest Python type checker and has excellent programmatic output:

```bash
# Option A: pyright's JSON output mode
pyright --outputjson src/ > analysis.json
# Parse the output for cross-reference data

# Option B: LSP mode
# Start pyright as LSP server, send textDocument/references requests
```

Alternatively, use `jedi` (pure Python, easier to integrate):
```python
import jedi
script = jedi.Script(source, path=file_path)
refs = script.get_references(line, column)
```

**Recommendation:** `pyright` via LSP. It handles complex Python patterns (metaclasses, decorators, type: ignore) better than jedi.

**Prerequisites:** `pyright` (npm package) or `pyright` binary.

#### Go: `golang.org/x/tools/go/callgraph`

Go has the best tooling for this — a dedicated call graph package:

```go
package main

import (
    "golang.org/x/tools/go/callgraph"
    "golang.org/x/tools/go/callgraph/cha"  // Class Hierarchy Analysis
    "golang.org/x/tools/go/packages"
    "golang.org/x/tools/go/ssa"
    "golang.org/x/tools/go/ssa/ssautil"
)

func extractCallGraph(dir string) *callgraph.Graph {
    pkgs, _ := packages.Load(&packages.Config{Dir: dir}, "./...")
    prog, _ := ssautil.AllPackages(pkgs, 0)
    prog.Build()
    return cha.CallGraph(prog)  // Complete call graph in one call
}
```

This gives the **complete** call graph including interface dispatch, in a single function call. Go's tooling is the gold standard here.

**Recommendation:** Build a small Go binary (`gyre-go-callgraph`) that outputs JSON. The Gyre extractor shells out to it.

**Prerequisites:** Go toolchain installed.

#### TypeScript: `tsc` Compiler API

```typescript
import * as ts from 'typescript';

function extractCallGraph(projectDir: string) {
    const config = ts.readConfigFile('tsconfig.json', ts.sys.readFile);
    const program = ts.createProgram(config.fileNames, config.options);
    const checker = program.getTypeChecker();

    // For each function declaration:
    // Use checker.getSymbolAtLocation() to get the symbol
    // Use ts.FindAllReferences.findReferencedSymbols() for call sites
}
```

Or via LSP (simpler):
```bash
# Start tsserver, send textDocument/references requests
```

**Recommendation:** LSP via `typescript-language-server`. Same pattern as Rust/Python.

**Prerequisites:** `typescript` and `typescript-language-server` npm packages.

## Extraction Pipeline

```
┌──────────────────────────────────────────────────────┐
│                   Push / Sync                         │
├──────────────────────────────────────────────────────┤
│  Pass 1: Syntax extraction (tree-sitter/syn)          │
│    → Nodes: types, functions, interfaces, endpoints   │
│    → Edges: Contains, Implements, basic Calls         │
│    → Time: < 1 second                                 │
│    → Updates graph immediately                        │
├──────────────────────────────────────────────────────┤
│  Pass 2: Semantic extraction (LSP/type-checker)       │
│    → Edges: complete Calls (all call sites resolved)  │
│    → Time: 5-30 seconds (depends on project size)     │
│    → Runs in background, merges when done             │
├──────────────────────────────────────────────────────┤
│  Result: Complete knowledge graph                     │
│    → Structural lens: full causal flow                │
│    → LLM queries: accurate blast radius               │
│    → Evaluative lens: OTLP traces overlay on graph    │
└──────────────────────────────────────────────────────┘
```

Pass 1 and Pass 2 run sequentially but Pass 2 is non-blocking — the graph is usable after Pass 1 with partial call data, and becomes complete when Pass 2 finishes.

## Edge Types Produced

| Edge | Source | Pass | Coverage |
|---|---|---|---|
| `Contains` | Syntax (declarations) | 1 | 100% |
| `Implements` | Syntax (impl blocks, class inheritance) | 1 | ~95% (misses implicit Go interfaces) |
| `Calls` | Type checker (find-references) | 2 | 100% |
| `DependsOn` | Package manifests (Cargo.toml, go.mod) | 1 | 100% |
| `FieldOf` | Syntax (struct fields) | 1 | 100% |
| `Returns` | Syntax (function signatures) | 1 | 100% |
| `RoutesTo` | Syntax (framework-specific patterns) | 1 | ~90% (framework-dependent) |
| `GovernedBy` | Spec comments / manifest | 1 | Manual |

## LLM Integration

With complete call edges, the LLM can answer:

**Blast radius:**
> "If I change `TaskPort::find_by_id` to return `Result<Task>` instead of `Option<Task>`, what breaks?"
> → Follow `Implements` to all implementors (SqliteTaskRepo, MemTaskRepo)
> → Follow `Calls` to all callers (23 call sites across 4 crates)
> → Follow `GovernedBy` to affected specs (platform-model.md, search.md)
> → "14 functions in gyre-server, 6 in gyre-domain, 3 in gyre-adapters would need updating. 2 specs are affected."

**Coupling analysis:**
> "Can I safely extract the search module into its own crate?"
> → Count incoming `Calls` edges from other modules (coupling)
> → Count outgoing `Calls` edges to other modules (dependencies)
> → "SearchService has 4 incoming callers and 2 outgoing dependencies. Low coupling — safe to extract."

**Dead code detection:**
> "Which functions have zero incoming Calls edges?"
> → Query the graph for function nodes with no inbound `Calls`
> → Cross-reference with endpoint `RoutesTo` edges (entry points don't need callers)
> → "12 functions appear unused. 3 are test helpers. 9 may be dead code."

**Impact of spec changes:**
> "If I update the search spec to add semantic search, what types and functions are involved?"
> → Follow `GovernedBy` to find all nodes linked to the search spec
> → Follow `Calls` outward to find dependencies
> → "SearchService, SearchQuery, FullTextPort are directly governed. SearchService calls IndexManager and VectorStore which would also need changes."

## Performance Considerations

- **Rust workspace** (~1800 nodes): rust-analyzer loads in ~3-5 seconds, find-references queries ~10ms each. Total: ~20 seconds for full call graph.
- **Python project** (~5300 nodes): pyright indexes in ~5-10 seconds, queries ~5ms each. Total: ~30 seconds.
- **Go project** (~2000 nodes): `callgraph.Graph` computes the entire call graph in ~2-5 seconds. Fastest of all.
- **TypeScript project** (~3000 nodes): tsc loads in ~3-8 seconds, queries ~10ms each. Total: ~30 seconds.

All acceptable for a background job triggered on push/sync. Pass 1 (syntax) returns instantly; Pass 2 (semantic) completes within a minute even for large projects.

## Implementation Phases

**Phase 1: Go** (simplest — `callgraph` package gives complete graph in one call)
- Build `gyre-go-callgraph` binary
- Shell out from Go extractor
- Validate with a real Go project

**Phase 2: Rust** (most impactful for Gyre itself)
- LSP subprocess approach first
- Shell out to `rust-analyzer` with find-references queries
- Validate with the Gyre codebase

**Phase 3: Python** (needed for kartograph)
- LSP subprocess with pyright
- Validate with kartograph

**Phase 4: TypeScript** (needed for frontend codebases)
- LSP subprocess with tsserver
- Validate with the Gyre web UI

Each phase is independent — they can run in parallel as sub-agents.

## Prerequisites

The extraction host (server or agent container) must have the language toolchains installed:
- Rust: `rust-analyzer` binary
- Python: `pyright` (via npm or pip)
- Go: Go toolchain (for `go/callgraph`)
- TypeScript: `node`, `typescript`, `typescript-language-server`

For Docker agent containers, these should be included in the base image. For local development, they're assumed to be installed.

## Relationship to Other Specs

**Implements:** `system-explorer.md` §5 (Flow Traces — complete structural causality)
**Enables:** `explorer-canvas.md` structural lens (full "Trace from here"), LLM blast-radius queries
**Depends on:** `realized-model.md` (knowledge graph storage), existing tree-sitter/syn extractors (Pass 1)
**Complements:** OTLP trace capture (`gate_executor.rs` TraceCapture gate) — static graph + dynamic traces = complete picture
