# Review: task-205 — Manifest-driven Concept Views for the knowledge graph

**Reviewer:** Verifier
**Round:** R1
**Spec ref:** realized-model.md §4 Concept Views, §7 API Surface (concept rows)
**Comparison base:** 66422bd4b99de70536cce8422ec23db5eaec082d
**HEAD reviewed:** 24d1980c0bdcc195dffb7aca09f65153a161cf70
**Status:** complete

---

## Findings

None material. The implementation is real, wired through the actual entry
points, and the regression guards kill the substring-revert mutant. Evidence
for each acceptance criterion below.

### A1 — `concepts:` block parses into `ConceptView`-shaped data; no longer dead

- `SpecManifest.concepts: Vec<ConceptDef>` (`spec_registry.rs:31-33`) with
  `#[serde(default)]`; `ConceptInclude` models the spec's sequence-of-single-key-maps
  shape and `From<&ConceptDef> for gyre_common::graph::ConceptView` flattens
  repeated keys across include entries (aggregation tested:
  `test_parse_concept_without_include_and_split_keys`).
- Unit tests parse both spec-example concepts into the exact expected vectors
  (`test_parse_concepts_spec_examples`) and a manifest with no `concepts:` key
  parses to an empty vec (`test_manifest_without_concepts_parses_empty`).
- `ConceptView` is instantiated from real manifest content in production code:
  `resolve_concept_view` (api/graph.rs:672-687) reads the repo's
  `specs/manifest.yaml` via `spec_registry::read_git_file(&git_bin, &repo.path,
  &repo.default_branch, ...)` — the exact pattern prescribed by the task
  (parity with specs_assist.rs:170-174) — and builds the view via
  `ConceptView::from`. Both REST handlers and both MCP tool scopes flow
  through it.

### A2 — `/graph/concept/:name` is a manifest projection, 404 on undefined

- `get_graph_concept` resolves the repo, delegates to
  `assemble_concept_projection` → `resolve_concept_view` (case-insensitive
  exact match on concept name, `eq_ignore_ascii_case`) → `ConceptView::project`.
  No substring fallback anywhere: unresolvable concept (missing manifest,
  unparseable manifest, or unknown name) returns `ApiError::NotFound`
  (api/graph.rs:594-600).
- Projection semantics (gyre-common/src/graph.rs:268-354) implement the union
  of all five include rules keyed by NodeType, edges included iff both
  endpoints matched, soft-deleted nodes/edges excluded. The endpoint route
  path is matched from RoutesTo/Contains edge metadata `{"path": ...}` —
  which matches the real extractor convention (rust_extractor.rs:474-507
  writes exactly that metadata on RoutesTo/Contains edges with the Endpoint
  node as source). The `include_specs` GovernedBy convention matches the real
  extractors (rust_extractor.rs:385-391, python_extractor.rs:296,
  go_extractor.rs:271, typescript_extractor.rs:363); extractor-created spec
  nodes are Module nodes whose name/qualified_name is the spec path
  (rust_extractor.rs:432-452), which `matches_spec_include` handles via
  path/basename matching.
- Glob matcher is anchored, case-sensitive, `*`-only (DP over pattern/text,
  graph.rs:218-247) — `MergeRequest` ≠ `MergeRequestDependency`, `Gate*` ≠
  `LateGate`, both directions tested (`glob_match_anchoring`). No new crate
  dependency; `serde_json` was already a direct gyre-common dep.

### A3 — workspace endpoint unions per-repo projections

`assemble_workspace_concept_projection` (api/graph.rs:629-663) iterates the
workspace's repos, resolves the concept from each repo's own manifest, skips
repos that don't define it, unions nodes/edges. Integration test
`workspace_graph_concept_unions_manifest_projections` seeds repo-2 in the
*same* workspace (`ws-briefing` — verified against the fixture) with a
pattern-matching node but no concept in its manifest, and asserts it
contributes nothing while repo-1's 4-node projection comes through. Not a
trivially-passing scoping accident.

### A4 — substring regression guard is real (mutation probe)

Ran the decisive probe in an isolated worktree at 24d1980c (private
CARGO_TARGET_DIR /tmp/gyre-mutant-target): replaced
`ConceptView::matches_node`'s include-rule body with the old hollow
case-insensitive substring-on-concept-name behavior. Result (exit 101,
evidence at /tmp/stage/review-evidence/task205-mutation-substring-revert.txt):

- `graph_concept_returns_manifest_projection_not_substring` FAILED (empty projection)
- `mcp_graph_concept_manifest_projection_not_substring` FAILED — returned
  only the decoy `user_authentication_service`; the guard fired in exactly
  the right direction
- `workspace_graph_concept_unions_manifest_projections` FAILED (0 nodes)

Baseline (unmutated, same filters): 9/9 pass. The decoy itself is sound: the
77dd2ee0 fix replaced `OauthAuthenticationProvider` (which legitimately
contains capital-A "Auth" and would fail against *correct* glob behavior)
with `user_authentication_service` — contains the lowercase substring the old
behavior matched, contains no capital-A "Auth", and its qualified name avoids
the `*::auth*` module pattern. Discriminates in both directions.

### A5 — no regressions

- `?concept=` substring filter on `GET /repos/:id/graph` preserved verbatim
  (api/graph.rs:368-393), including the both-endpoints-matched edge filter.
- `/graph/spec/:path` (`get_graph_by_spec`) untouched.
- Both concept routes remain in the ABAC resolver (abac_middleware.rs:167,
  :411) — the replacement of `require_repo` with `find_by_id` + NotFound in
  `get_graph_concept` preserves the old existence-check semantics (the old
  `require_repo` did no scoping either; authorization is middleware ABAC).
  `scripts/check-abac-route-registry.sh` passes.
- `scripts/check-arch.sh` passes (projection logic lives in gyre-common +
  gyre-server; domain imports nothing new).
- Full neighboring modules green at HEAD: api::graph::tests + mcp graph
  tests (30 passed), spec_registry::tests (45 passed), gyre-common lib (98
  passed). `cargo build --all` succeeds.
- MCP parity: `graph_concept` tool reworded to describe projection; undefined
  concept → tool error naming the concept; unknown repo → explicit error
  (previously a silent empty result). `check-mcp-write-tools.sh` OK (tool is
  read-only, correctly not in the write gate).
- Commit attribution: `check-task-commit-attribution.sh` OK; diff file set ==
  listed commits' union, no out-of-scope files.

### Minor observations (non-blocking, no action required)

- `edge_route_path` parses edge metadata as JSON on every projection call;
  the extractor writes compact JSON without spaces (`serde_json::json!` →
  `{"path":...}` with no spaces after `:` or `,` in the default compact
  form). Test fixtures use the same compact form. If a producer ever wrote
  pretty-printed JSON, `value.get("path")` still works since serde_json
  handles whitespace — no issue, just noting the convention coupling.
- The handoff's `list_nodes(repo_id, None)` port-semantics question is
  settled: port doc says `None` = no type filter (gyre-ports/src/graph.rs:19-21).

## Verdict

**complete.** Every acceptance criterion is met by production code wired
through the real entry points; the regression guard demonstrably fails on
substring revert; no scope creep, no gate weakening, no deleted tests, no new
exemptions. Coverage row realized-model.md §4 re-audited to `implemented`.

## Shipped

- `specs/manifest.yaml` `concepts:` blocks now parse into domain
  `ConceptView`s (five include-rule vectors, repeated-key aggregation) via
  `SpecManifest.concepts` + `From<&ConceptDef>`; repos without a concepts
  block parse cleanly.
- `GET /api/v1/repos/:id/graph/concept/:name` and
  `GET /api/v1/workspaces/:id/graph/concept/:name` return the manifest-driven
  union projection (types/traits/modules/endpoints/specs, anchored
  case-sensitive `*`-globs, edge-metadata route paths, GovernedBy spec
  linkage) with 404/no-contribution for undefined concept names — substring
  fallback removed; the `?concept=` substring query param on `/graph` is
  preserved as a distinct surface per spec §7.
- MCP `graph_concept` tool shares the same projection assembly (HSI §11
  parity) for repo and workspace scopes, with explicit errors for undefined
  concepts and unknown repos.
- Glob matcher + projection live in `gyre-common` (`glob_match`,
  `ConceptView::project`); no new crate dependencies; 16 new tests including
  integration tests that a mutation probe confirms fail under substring
  revert.
