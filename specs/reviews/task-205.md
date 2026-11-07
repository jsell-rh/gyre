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

---

## Round R2 (independent review, candidate 3aa5d3c860b80871c2298b7c046565fed272bdf1)

**Reviewer:** Independent review agent
**Comparison base:** 4d6759a0bfd637e899bbf65e725f91b07f745c69
**Candidate:** 3aa5d3c860b80871c2298b7c046565fed272bdf1 (branch
`pipeline/task-205/1edcc269f2294a9f90a9cc1a7c555506-1`; all 16 frontmatter
commits verified present and ancestral)

### Diff scope

9 files: gyre-common/src/graph.rs (glob matcher + `ConceptView::project`),
gyre-server spec_registry.rs (ConceptDef parsing), api/graph.rs (endpoint
rewiring), mcp.rs (tool parity), graph_integration.rs (tests), gyre-cli
client.rs (doc comment only), coverage matrix, task-205.md, this review file.
No web/dist delta from base; no script, exemption-file, or port-trait changes;
no new crate dependencies.

### Probes run (evidence: /tmp/stage/review-evidence/task205-r2-*)

- `cargo test -p gyre-common --lib concept` → 4/4 pass
  (`glob_match_anchoring` proves `MergeRequest` ≠ `MergeRequestDependency`,
  `Gate*` ≠ `LateGate`, case sensitivity).
- `cargo test -p gyre-server --lib -- concept spec_registry` → 54/54 pass,
  including the spec-example concept parses and no-`concepts:`-key parse.
- `cargo test -p gyre-server --lib -- api::graph::tests::(graph_concept|
  workspace_graph_concept) mcp::tests::mcp_graph_concept*` → 9/9 pass.
- `cargo test -p gyre-server --test graph_integration -- concept` → 4/4 pass
  on a real TCP listener (sandbox permits `accept(2)` this round).
- **Mutation probe (temporary, restored byte-identical, sha1-verified):**
  reverting `assemble_concept_projection` to the old case-insensitive
  substring matching makes 3 tests FAIL
  (`graph_concept_returns_manifest_projection_not_substring`,
  `workspace_graph_concept_unions_manifest_projections`,
  `mcp_graph_concept_manifest_projection_not_substring` — the last catches
  the `user_authentication_service` substring decoy). The guards are real,
  not self-confirming. `git status` clean after restore; tests re-run green.
- Focused builds: gyre-common, gyre-server (test profile), gyre-cli all
  compile. Workspace-wide `cargo build --all` is wrapper-rejected in this
  sandbox (deferred to verification; the CLI delta is a doc comment and the
  server/common deltas are covered by the test-profile compile).
- Shell gates: check-arch, check-abac-route-registry, check-abac-exempt-
  handlers, check-mcp-write-tools, check-task-commit-attribution,
  check-mem-port-contracts, check-dead-message-kinds, check-in-memory-state-
  stores, check-forged-scope-fields, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-lossy-secret-conversion, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-inert-enforcement,
  check-forwarded-header-trust, check-migration-sql-portability,
  check-migration-versions, check-unbounded-external-http — all pass.
  check-unnamed-tuple-carriers and check-mcp-wrapper-parity fail with
  byte-equivalent output at the base commit (worktree-verified: same 15 and
  same 35 violations respectively, none in files this task touched) —
  pre-existing, not candidate defects. Notably `handle_graph_concept` now
  follows the mcp-parity-preferred pattern (serializes the shared domain
  response).

### Findings

None material.

- Authorization posture unchanged: the removed `require_repo` call was an
  existence check only (repo found → 404 semantics preserved via
  `find_by_id`); tenant/workspace ABAC for both concept routes is enforced
  by the middleware registry (abac_middleware.rs:163, :413), identical to
  sibling graph routes. MCP handler preserves prior scope semantics.
- 404 for undefined concept names, no substring fallback — confirmed by
  three tests plus the mutation probe. `?concept=` substring param on
  `GET /graph` preserved (`test_full_graph_concept_query_param` passes).
- Coverage row realized-model.md §7 flipped to `implemented` by implementer
  commits within task scope (da4ad02a5/c88fc0962), re-anchored to this
  branch's work; attribution gate passes at the 16-SHA frontmatter.

### Verdict

**approved.** Real manifest-driven projection in production code, real
glob matching with anchoring, hard regression guards proven by mutation,
no fakery, no gate weakening, no scope creep.
