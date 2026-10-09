# Review — task-068 (Graph Summary & Dry-Run MCP Tools)

Spec: `explorer-implementation.md` §9, §22–23.
Comparison base: `3214c982e21a036834362bdd798d5173daf552ae` → HEAD `50e86a1`.
Commits under review: the 7 attributed in the task frontmatter (92b6aa1, be0f528, 0d0a62f, eb9ad82, cc10a0a, 01121b1, 2dc019d) — verified: every code commit in the range is in the list, the rest are `process:` attribution commits.

## Round 1

### Verified working

- **§9 all five tools callable via MCP protocol.** `search` added to `tool_definitions` (mcp.rs:466) and dispatch (`"search" => handle_graph_search`, mcp.rs:3112); `graph_summary`/`graph_query_dryrun`/`graph_nodes`/`graph_edges` were already registered at base (mcp.rs:413-451, :3032-3035 at base) — this task's delta is the missing fifth tool plus real call-path proof. `mcp_tools_list` (mcp.rs:3231-3236) asserts all five names. New in-process tests drive the real JSON-RPC path (`mcp_post` → `build_router` → `AuthenticatedAgent` extractor → `mcp_handler` dispatch) against a real graph store: `mcp_graph_summary_tool_call`, `mcp_graph_query_dryrun_tool_call`, `mcp_graph_query_dryrun_empty_scope_warning_over_mcp`, `mcp_graph_query_dryrun_focus_scope_envelope_round_trip`, `mcp_graph_nodes_tool_call`, `mcp_graph_edges_tool_call`, `mcp_graph_search_tool_call` — 10/10 pass (`cargo test -p gyre-server --lib mcp_graph`). TCP twins exist in tests/graph_integration.rs (`test_mcp_graph_summary/_dryrun/_nodes/_edges/_search`); loopback is seccomp-blocked in this sandbox, so the in-process twins are the runnable proof — they exercise the same router+auth+handler path minus the socket.
- **SDK wiring consumes the MCP names.** `scripts/explorer-agent.mjs:82-105` allowlists `mcp__gyre__{graph_summary,graph_query_dryrun,graph_nodes,graph_edges,search,node_provenance}` against `mcpServers.gyre = {url: /mcp, Bearer token}` — the §9 contract (SDK subprocess calls tools over MCP HTTP, not just the inline handler) holds end-to-end.
- **§22 graph_summary fields + test_coverage.** All six spec fields present in `GraphSummary` (view_query_resolver.rs:2448-2459). Test coverage is a real multi-source BFS from all `test_node == true` nodes following outgoing `Calls` edges only (`compute_test_reachable` :405, `TEST_REACHABILITY_EDGES = [Calls]` :400, adjacency skips soft-deleted edges :222-224). `mcp_graph_summary_tool_call` seeds test→a Calls edge and asserts test_functions=1, reachable=2, unreachable=1 — real store, real edge. Spec-example arithmetic is consistent with counting test seeds as reachable (spec: 180+85=265=total functions).
- **§23 dryrun envelope + resolution + warnings.** MCP handler now wraps as `{"query": ..., "result": {DryRunResult}}` (mcp.rs:2153-2160), matching the spec §9/§23 JSON example exactly; the round-trip test proves tagged scope enums survive (`scope.node` stays a string, untagged `zoom: "fit"` stays a string) — this kills the serialization-mangling class the integration twin's assertion targets. Domain tests cover every warning class with boundary semantics: empty scope → "Scope matched 0 nodes" (:1811); cluttered scope at exactly 200 no-warn / 201 warns "Scope matched 201 nodes - may be cluttered" (:1814-1818, test asserts both sides); group >20 → "Group 'X' matched N nodes - too broad" (:1965-1971, 25-node warn + 5-node no-warn); unresolved callouts/narrative reported by name with warnings (:2013-2043). `matched_node_names` switched from qualified_name to `name`, matching the spec example's short names.
- **§9 `search` implementation.** `search_graph_nodes` (view_query_resolver.rs:2494-2546): case-insensitive substring across name/qualified_name/file_path/spec_path/doc_comment, soft-deleted excluded, ranked (exact > prefix > substring > qname > other), char-boundary-safe doc truncation. Domain tests assert ranking order, deleted-exclusion, limit, empty/no-match. The MCP handler enforces required fields (`repo_id`, `query`) as tool errors, not panics; cap 50.
- **Inline fallback path consistency.** explorer_ws.rs `execute_tool` "search" arm now delegates to the same domain `search_graph_nodes` (was a duplicated inline filter that only searched name/qname/doc with a byte-sliced `&d[..100]`); the byte-slice exemption line was removed because the violation is gone — a real fix, not an exemption shuffle (mechanical gate passes).
- **Probe results (this sandbox):** `cargo check -p gyre-server --tests` clean; `cargo test -p gyre-server --lib mcp` 78/78 pass; `cargo test -p gyre-domain --lib view_query_resolver` 124/124 pass.
- **RBAC:** all five graph tools are read-only (list_nodes/list_edges/get_node only) and correctly absent from the `needs_write` gate (mcp.rs:3011-3025) — matches handler effects; `check-mcp-write-tools.sh` passes.

### Verdict (round 1)

**needs-revision** — one minor finding (F1). All §9/§22/§23 behavior is real, enforced through the MCP protocol path, and covered by tests that fail when the behavior breaks. Repair is a one-line assertion restore; after that the task meets the spec.

## Round 2 (F1 repair verification)

Comparison base 66422bd4 (per controller scope); task source at HEAD 0b74e20 identical to 4093cd3 (only `process:` attribution commits after). The only source change since round 1's verified HEAD 50e86a1 is the one-line fb9893c repair in mcp.rs; `view_query_resolver.rs`, `explorer_ws.rs`, `graph_integration.rs`, and the exemptions file are byte-identical to round 1, so round-1 evidence for those files carries.

### Repair verification (F1)

- `gyre_search` assertion restored at mcp.rs:3230; `mcp_tools_list` passes at HEAD.
- **Mutation probe (isolated copy, private target dir):** removing the gyre_search json! block from `tool_definitions()` makes `mcp::tests::mcp_tools_list` panic with `assertion failed: names.contains(&"gyre_search")`. The restored assertion genuinely guards registration.
- **Repair side effect independently checked:** fb9893c replaced the `gyre_agent_complete` assertion line rather than adding alongside. Net vs base, `gyre_agent_complete` lost its (redundant) `mcp_tools_list` line but keeps two dedicated tests that fail if its registration or dispatch breaks: `agent_complete_tool_schema_includes_summary` (mcp.rs:3795, `.expect("gyre_agent_complete must be in tools list")` on the real `tools/list` path) and `agent_complete_unknown_agent_returns_error` (mcp.rs:3822, real `tools/call` dispatch). Both pre-existed at base (66422bd4:3713/3740), both pass at HEAD. Coverage-neutral swap, not a meaningful-test deletion.
- No gate weakening: the only scripts/ delta is the byte-slice exemption-line removal for explorer_ws.rs:3437, backed by a real fix (the `&d[..100]` slice is gone; `execute_tool` "search" delegates to domain `search_graph_nodes`). `check-byte-slice-truncation.sh`, `check-mcp-write-tools.sh`, `check-task-commit-attribution.sh` all pass.

### Probes at HEAD (this sandbox, 2026-10-09)

- `cargo test -p gyre-server --lib mcp` → 78/78 pass (includes mcp_tools_list, ten mcp_graph* tool-call tests over the real router→auth→dispatch path, both agent_complete tests). Private CARGO_TARGET_DIR.
- `cargo test -p gyre-server --lib mcp_graph` → 10/10 pass.
- `cargo test -p gyre-domain --lib view_query_resolver` → 124/124 pass.
- Listener-based `graph_integration.rs` tests cannot run here (accept() fails with Errno 95 — no loopback listeners in this sandbox); those run in the controller's gates. The in-process `oneshot` tests are the local equivalent for the MCP protocol path.
- Evidence recorded under `/tmp/stage/review-evidence/task-068-r2/` (commands, results, mutation probe detail incl. a first mis-cwd'd mutation attempt that was immediately restored and redone correctly).

### Findings

- [x] **F1 (minor, RESOLVED by fb9893c): deleted meaningful test assertion — `gyre_search` had zero test coverage repo-wide.** Commit 01121b1 removed `assert!(names.contains(&"gyre_search"))` from `mcp_tools_list` (mcp.rs) and replaced it with the five §9 graph-tool assertions. `gyre_search` (entity full-text search, M22.7) remained registered (mcp.rs:329) and dispatched (mcp.rs:3094), but no test referenced it. **Repair verified:** fb9893c restored the assertion at HEAD mcp.rs:3230. Mutation probe in an isolated copy: removing the gyre_search entry from `tool_definitions()` fails `mcp_tools_list` with `assertion failed: names.contains(&"gyre_search")` — the restored assertion has teeth. Side effect of the repair checked: it swapped the `gyre_agent_complete` line for `gyre_search` rather than adding alongside; this is coverage-neutral, not a meaningful-test deletion — `gyre_agent_complete` registration+dispatch remain enforced by two dedicated pre-existing tests (`agent_complete_tool_schema_includes_summary`, `.expect("gyre_agent_complete must be in tools list")` on the real tools/list path, mcp.rs:3795; `agent_complete_unknown_agent_returns_error`, mcp.rs:3822; both present unchanged at base 66422bd4:3713/3740 and still passing at HEAD).

### Non-findings (checked, no gap)

- Inline explorer_ws `graph_query_dryrun` returns bare `DryRunResult` while the MCP handler returns the envelope — not a gap: the spec's envelope shape is the §9 MCP-tool response; the inline path's only consumer is the self-check loop, which consumes the typed struct fields directly (explorer_ws.rs:2976-2981), not JSON.
- `/mcp` route is not in the ABAC registry (falls through middleware) — pre-existing at base, outside this task's diff.

## Verdict

**complete** — F1 resolved with evidence; no new findings. All round-1 verified behavior (§9 five tools over MCP incl. the envelope round-trip, §22 graph_summary six fields + real BFS test coverage, §23 dryrun resolution + boundary-tested warnings) stands, re-probed at HEAD where source changed. Source at final HEAD f134e8a is byte-identical to the evidence HEAD 0b74e20 (f134e8a touches only this review file), so round-2 probes remain valid.
