# M30: Knowledge Graph

**Status:** Done
**Milestone:** M30

---

## Summary

M30 implements the Realized Model — a live knowledge graph extracted from source code, giving Gyre continuous architectural self-awareness. Agents and humans can query the graph to understand repo structure, track spec coverage, assess risk, and view architectural change over time.

Related specs:
- [specs/system/realized-model.md](../system/realized-model.md)
- [specs/system/system-explorer.md](../system/system-explorer.md)

---

## M30a — Domain Types, Rust Extractor, and Graph API (PRs #332, #333, #334, #337)

### Domain types (PR #332, TASK-174)

`gyre-domain` gains:

- `GraphNode` — universal AST node with fields: `id`, `repo_id`, `node_type` (`Package`/`Module`/`Type`/`Interface`/`Function`/`Endpoint`/`Table`), `name`, `qualified_name`, `file_path`, `line_start`/`line_end`, `visibility`, `doc_comment`, `spec_path`, `spec_confidence` (`None`/`Low`/`Medium`/`High`), `last_modified_sha`, `last_modified_by`, `complexity`, `churn_count_30d`
- `GraphEdge` — directed relationship between nodes with `edge_type` (`Contains`/`Calls`/`Implements`/`Uses`/`Returns`/`Extends`/`DependsOn`)
- `ArchitecturalDelta` — record of graph changes at a commit: `commit_sha`, `timestamp`, `spec_ref?`, `agent_id?`, `delta_json`
- `GraphPort` trait — `save_node`, `find_nodes_by_repo`, `find_node`, `save_edge`, `find_edges_by_repo`, `delete_nodes_by_repo`, `save_delta`, `find_deltas_by_repo`
- `MemGraphStore` — in-memory implementation for dev/test

### Rust extractor (PR #333, TASK-176)

`RustExtractor` in `gyre-domain` uses the `syn` crate to parse Rust source files and produce graph nodes:

- Parses `Cargo.toml` for `name` and `version` → `Package` node
- Walks source tree, parses each `.rs` file via `syn::parse_file`
- Extracts: `Module` nodes (from `mod` declarations), `Type` nodes (structs + enums), `Interface` nodes (traits), `Function` nodes (fns + methods with visibility)
- Populates `doc_comment` from `#[doc]` attributes / `///` comments
- Sets `visibility` (`public`/`private`/`pub(crate)`)
- Complexity: incremented per `if`/`match`/`while`/`for`/`loop` branch

### Bare repo init fix (PR #334)

Fixes bare repo `git receive.denyCurrentBranch=ignore` so initial commits to bare repos work — prerequisite for extraction tests.

### Graph API endpoints (PR #337, TASK-175)

13 new endpoints under `/api/v1/repos/{id}/graph/`:

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/repos/{id}/graph` | Full knowledge graph — `{repo_id, nodes, edges}` |
| `GET` | `/api/v1/repos/{id}/graph/types` | Type nodes (structs, enums) with edges |
| `GET` | `/api/v1/repos/{id}/graph/modules` | Module nodes with containment edges |
| `GET` | `/api/v1/repos/{id}/graph/node/{node_id}` | Single node + all connected edges |
| `GET` | `/api/v1/repos/{id}/graph/spec/{spec_path}` | Nodes linked to a spec path |
| `GET` | `/api/v1/repos/{id}/graph/concept/{name}` | Case-insensitive substring search across node names |
| `GET` | `/api/v1/repos/{id}/graph/timeline` | Architectural deltas with `?since=`/`?until=` filters |
| `GET` | `/api/v1/repos/{id}/graph/risks` | Risk metrics per node (`churn_rate`, `fan_out`, `fan_in`, `complexity?`, `spec_covered`) |
| `GET` | `/api/v1/repos/{id}/graph/diff` | Graph diff between commits (`?from=`/`?to=`) |
| `POST` | `/api/v1/repos/{id}/graph/link` | Manually link a node to a spec path; **Developer+ required** |
| `GET` | `/api/v1/repos/{id}/graph/predict` | Structural prediction stub (pending full pipeline) |
| `GET` | `/api/v1/workspaces/{id}/graph` | Cross-repo aggregated graph for a workspace |
| `GET` | `/api/v1/workspaces/{id}/briefing` | Narrative summary of recent architectural changes |

Auth fix (PR #340, TASK-185): `POST /api/v1/repos/{id}/graph/link` requires `RequireDeveloper`.

---

## M30b — Push-Triggered Graph Extraction (PR #346)

After every push to a repo's **default branch**, the post-receive hook spawns a background task that:

1. Archives the commit tree via `git archive`
2. Runs `RustExtractor` against the archived source
3. Clears stale graph data for the repo (`delete_nodes_by_repo`)
4. Persists the new node/edge snapshot
5. Records an `ArchitecturalDelta` with node/edge counts and the triggering commit SHA

Extraction errors are logged and swallowed — extraction **never fails a push**. The graph is eventually consistent: it reflects the most recent successful extraction run.

---

## Acceptance Criteria

- [x] `GraphNode`/`GraphEdge`/`ArchitecturalDelta` domain types in `gyre-domain`
- [x] `GraphPort` trait + `MemGraphStore` in `gyre-domain`
- [x] `RustExtractor` parses Package/Module/Type/Interface/Function nodes from Rust source
- [x] All 13 graph API endpoints return valid JSON
- [x] `POST /api/v1/repos/{id}/graph/link` requires Developer+ role (403 for ReadOnly)
- [x] Push to default branch triggers graph extraction in background
- [x] Graph API returns extracted nodes after a push (verified in integration test)
- [x] Extraction failure does not block or fail the git push

---

## Implementation Notes

- `gyre-server/src/graph_extraction.rs`: `extract_and_store_graph()` — called from `git_http.rs` post-receive `tokio::spawn` block
- `RustExtractor` lives in `gyre-domain` (not `gyre-adapters`) — pure domain logic, no I/O
- `syn` + `toml` added as dev-dependencies in `gyre-domain`
- Integration test `test_push_triggers_graph_extraction` in `crates/gyre-server/tests/graph_integration.rs` (18 tests total)
