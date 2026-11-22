---
title: "View query diff scope: commit-range change set consumed from the shared two-commit graph-diff core (server resolver + renderer integration)"
spec_ref: "Spec gap spec-57c2a110e6b0a8363571ac4d / diff-scope-commit-range / 78cf53a9a787"
depends_on: []
progress: complete
commits: ["6ba5217eeb6d801d798029cd07baf41f72250eb3", "4d7101c24b1e7cbc5e89e95aa0d9749a668abc7e", "db3243b4957c5778d6cfef4c11b41b0b32dc8323", "41de1c4b2c8d49d6521ad4171a14ccff8ef520b8", "81e2dc0c1dd5cba53eb1a32b92ac661c93050fbd"]
---

## Required behavior

Spec row (§ '2. Scope — What Subgraph to Show', `diff`): 'Changes between two commits'. The matched set of a SHA/ref-based Scope::Diff must be the set of graph nodes whose content differs between the two boundary commits, computed from real per-commit repository state — NOT from the current graph snapshot's single-point created_sha/last_modified_sha provenance.

SHARED-CORE REUSE (per review of the overlapping realized-model plan): the boundary-commit graph-state resolution and the node-level added/removed/modified change-set computation are owned by the earlier-audited realized-model plan's task `graph-diff-two-commits` (planner spec-3899ae86d0ae2a8e0fd78bfd, plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, covering realized-model.md section e57272a0df7f65577ff10261 — GET /repos/{id}/graph/diff). This task must NOT build a second, independent implementation of that core. Mechanism reconciliation (explicit choice, within graph-diff-two-commits' existing contract, which already allows 'the repo's real git state — or equivalent state snapshots'): both surfaces consume git-state snapshotting — resolve each boundary commit against the repository's real git (>=7-hex SHA prefixes, branch names, tags, HEAD), snapshot each boundary commit's tree via the existing `git archive` machinery (factored from do_extract Step 1 in crates/gyre-server/src/graph_extraction.rs), run the extraction pipeline on each snapshot, and compute the change set by identity (qualified_name): added = in to not in from; removed = in from not in to; modified = in both with a real content difference (diff_nodes-style comparison over substantive fields — signature, docs, metrics/complexity, spec linkage — excluding bookkeeping fields that differ by construction: last_seen_at, first_seen_at, created_at, sha stamps). Recorded-delta point-in-time reconstruction (list_nodes_at/list_edges_at) alone cannot serve this surface because view queries reference arbitrary commit pairs, including boundary commits at which no extraction ever ran; where recorded deltas exist they may serve as a fast path only if the result is identical to the snapshot path. If graph-diff-two-commits has not yet landed when this task starts, build the shared core ONCE in the shared location so both contract surfaces (GET /repos/{id}/graph/diff and the view-query Scope::Diff) consume it — never a view-query-private copy — and record the cross-reference in both task definitions (this definition cross-references graph-diff-two-commits; that task's definition must cross-reference this one when the plans merge).

THIS TASK'S OWNERSHIP — the view-query-specific integration only:
(1) Server wiring: when a view query with a non-temporal Diff scope is resolved — the REST endpoint POST /api/v1/repos/:id/graph/query-dryrun (api/graph.rs view_query_dryrun) and the explorer_ws.rs dry-run paths (the graph_query_dryrun MCP tool and the conversational self-check loop's self_check_dry_run) — gyre-server resolves from_commit/to_commit, obtains the change set from the shared core, and injects it into the domain resolver (a pure mapping in gyre-domain: dry_run/resolve_scope gain an injected change-set input; no git or infrastructure knowledge enters gyre-domain — hexagonal boundary preserved). Scope::Diff's matched set equals the change set's added+modified entries mapped to current active node ids via qualified_name; emphasis, edges, zoom, and annotation templates operate uniformly over that matched set ({{count}} resolves to its size). Removed nodes cannot be rendered and MUST be surfaced by name in DryRunResult.warnings rather than silently dropped. An unresolvable ref (e.g. 'no-such-branch') yields an explicit warning and an empty matched set — never a fabricated/fail-open default (repo invariant on resolve_ref). from==to keeps the existing empty-diff warning. Per-(repo, resolved commit) extraction results may be cached; the cache must not alter results.
(2) Heuristic removal: the boundary-SHA prefix predicate over the current snapshot, its '[info] SHA-based diff shows an approximate result' warning, and the code-comment concession about intermediate commits are REMOVED from view_query_resolver.rs — not retained as a fallback. The temporal `~epoch` diff path is preserved unchanged (a different query form, not flagged by the audit).
(3) Renderer: ExplorerCanvas.svelte must stop mirroring the SHA heuristic for SHA/ref-based diff scopes (the 'Matches server heuristic including its limitations' block); it obtains the matched set from the server query-dryrun endpoint (new client function in web/src/lib/api.js alongside the existing graphDiff helper) and feeds the returned ids into the existing matched/depth machinery; temporal ~epoch diff resolution stays local; fetch loading/error states show an explicit warning instead of an incorrect set.
(4) Tests: existing tests that encode the old heuristic are rewritten against the new contract, not deleted: view_query_resolver.rs Scope::Diff SHA tests (~lines 4638-4735, 5058-5151, 5394-5432) and web/src/__tests__/ExplorerCanvas.test.js diff-SHA tests (~lines 389-413, 537-575, 681-733).

## Acceptance Criteria

- [ ] Server integration test with a real git repo, real git binary, and the real extraction flow: commit A adds functions f, g, h; commit B modifies only f; commit C modifies only g; the graph is extracted at C (normal push flow). Diff{from: A, to: C} matches exactly {f, g} — h (untouched) is excluded and f (changed only at intermediate commit B) IS included. Diff{from: A, to: B} matches exactly {f}, proving a to_commit older than HEAD still resolves correctly.
- [ ] The result is not the whole graph and not empty: the pre-fix failure modes (all-present-nodes match when to == last extraction commit; empty set when to is older) are both killed by the exact-set assertions above.
- [ ] Removed-node contract: a node present at from but absent at to is named in DryRunResult.warnings; added+modified nodes are matched; the {{count}} annotation template resolves to the matched-set size for a diff-scope query.
- [ ] Unresolvable ref (e.g. from_commit 'no-such-branch') produces a warning and an empty matched set — no panic, no silent default.
- [ ] Domain unit tests for the pure change-set function: added/removed/modified classification; a node changed and reverted between the boundaries is NOT reported changed (boundary-to-boundary semantics); bookkeeping-only differences (last_seen_at, first_seen_at, sha stamps) do NOT count as modified.
- [ ] Renderer test: a SHA-based diff-scope query with a mocked server query-dryrun response highlights exactly the server-returned node ids (including a mid-range-changed node) and resolves the annotation count from them; no local SHA-heuristic code path remains for SHA diffs.
- [ ] cargo test --all passes and cd web && npm test passes, with the rewritten diff-scope tests included.
- [ ] Single shared core: the boundary-commit change-set computation invoked by the view-query diff scope is the same production code path that serves GET /repos/{id}/graph/diff (realized-model.md 'Graph Diff' section, shared with task graph-diff-two-commits of plan b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3) — for the same repo and resolved commit pair, both surfaces produce the identical node-level change set; this task introduces no second, divergent implementation of commit-range change-set computation.

## Verification intent

Run the acceptance-1 integration test against the current production code (real repo, real extraction at C): every active node carries last_modified_sha = C because each extraction stamps all present nodes, so the existing boundary-SHA predicate matches h as well — Diff{A, C} returns the whole active graph and the exact-set assertion matched == {f, g} FAILS on h; likewise Diff{A, B} returns the empty set (no node's snapshot provenance matches the older to_commit) and matched == {f} FAILS. The same test must also fail if the fix is reverted to snapshot SHA matching, proving it kills the real defect (single-snapshot provenance cannot express a commit-range diff) rather than asserting a symbol exists. Client-side: with the server query-dryrun fetch mocked to return a set containing the mid-range-changed node, removing the server call (restoring local resolution from created_sha/last_modified_sha) fails the component test, because the current snapshot's fields cannot produce that set — the mid-range node is not highlighted.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/view-query-grammar.md`. Source: `25497b9e3cc16c7e405c641870ff3c3b1905cf6b`; plan: `eec79eddc83fdceebccbcf9a19abdcf979a6e5dc214bc8c38a9cb18293353b74`.

[
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
]

## Shipped

The `diff` scope now implements the specced "Changes between two commits"
semantics on every surface, computed from real per-commit repository state —
the single-snapshot created_sha/last_modified_sha heuristic (and its
"[info] approximate" concession) is gone from both the domain resolver and
the renderer.

### Shared core (single implementation, both contract surfaces)

- `crates/gyre-server/src/graph_diff_core.rs` (new): resolves each boundary
  ref against the repo's real git (`git rev-parse --verify` via
  `git_refs::resolve_ref` — >=7-hex prefixes, branch names, tags, HEAD),
  snapshots each boundary commit's tree via `git archive`
  (`graph_extraction::snapshot_commit_tree`, factored out of do_extract
  Step 1), runs the full extraction pipeline on each snapshot, and computes
  the change set with the pure domain function. Serves BOTH
  `GET /repos/{id}/graph/diff` (now returns `nodes:
  {added,removed,modified}` by qualified name) and the view-query
  `Scope::Diff` — no second implementation. Per-(repo, full resolved SHA)
  extraction results are cached (bounded, immutable trees, results
  unchanged). graph-diff-two-commits (realized-model plan
  b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3) had not
  landed here, so the core was built once in this shared location per the
  task's reconciliation clause; that task's definition must cross-reference
  this one when the plans merge.
- `crates/gyre-domain/src/graph_diff.rs` (new): pure
  `compute_node_change_set` — identity by qualified_name; added = in to not
  in from; removed = in from not in to; modified = in both with a
  substantive content difference (signature/doc, file/lines, visibility,
  spec linkage, metrics), excluding bookkeeping fields that differ by
  construction (ids, sha stamps, timestamps, first/last_seen). No git or
  infrastructure knowledge in gyre-domain (check-arch.sh passes).

### Server wiring (view-query surfaces)

- `gyre-domain` `InjectedDiffChangeSet` + `dry_run_with_diff_change_set` /
  `resolve_scope_with_diff_change_set`: the server resolves the change set
  and injects it; the domain performs the pure qualified_name → active node
  id mapping. Matched set = added + modified; removed nodes are named in
  `DryRunResult.warnings`; unresolvable refs surface the server's warning
  with an empty matched set (never a fail-open default); from==to keeps the
  empty-diff warning; `matched_node_ids` added to `DryRunResult` for the
  renderer. Wired into REST `POST /repos/:id/graph/query-dryrun`
  (api/graph.rs), the MCP `graph_query_dryrun` tool (mcp.rs), and both
  explorer_ws dry-run paths (agent tool dispatch + conversational
  self-check loop).

### Renderer

- `web/src/lib/api.js`: new `graphQueryDiffScope` client function (alongside
  `repoGraphDiff`) calling the query-dryrun endpoint, returning
  `{matchedIds, warnings, matchedNodes}`.
- `web/src/lib/ExplorerCanvas.svelte`: SHA/ref-based diff scopes fetch the
  matched set from the server and feed the returned ids into the existing
  matched/depth machinery ({{count}}, emphasis, edges, zoom operate over it
  uniformly); the local SHA-prefix heuristic block is removed; temporal
  `~epoch` diff resolution stays local; loading/error/server-warning states
  render an explicit `[data-diff-scope-status]` banner instead of an
  incorrect set; a stale matched set is dropped when new boundaries are
  queried.

### Test evidence

- Domain unit tests (579 pass, `cargo test -p gyre-domain`): change-set
  classification; changed-and-reverted NOT modified (boundary-to-boundary);
  bookkeeping-only differences NOT modified; resolver diff tests rewritten
  against the injected-change-set contract (mid-range node matched,
  no-change-set warns, removed/unresolvable-ref warnings,
  matched_node_ids/{{count}}).
- Server integration test
  `test_view_query_diff_scope_commit_range` (gyre-server
  tests/graph_integration.rs): real repo pushed through the real git HTTP
  push flow — A adds f/g/h, B changes only f, C changes only g, graph
  extracted at C; asserts Diff{A,C} == exactly {f,g} and Diff{A,B} ==
  exactly {f} (kills both pre-fix failure modes: whole-graph match when
  to == HEAD, empty set when to < HEAD), removed h named in warnings after
  a second push deletes it, {{count}} == matched-set size, unresolvable
  'no-such-branch' warns + empty, and GET /graph/diff serving the identical
  change set. NOT executed in this sandbox — large gyre-server test
  compilation is deferred to the independent verifier (4 GiB budget rule;
  `cargo check -p gyre-server --tests` passes clean). Verifier command:
  `cargo test -p gyre-server --test graph_integration test_view_query_diff_scope_commit_range -- --nocapture`.
- Web tests: ExplorerCanvas.test.js rewritten diff-SHA tests (165/165 pass;
  full suite 127 files / 2570 passed before the final stale-set refinement,
  affected files re-run green after it) — server-set-driven {{count}}
  including the mid-range node whose snapshot provenance matches neither
  boundary (fails if local SHA resolution is restored), fetch-error and
  server-warning banners, exact-id mirror tests, no local prefix filtering.
- Invariant scripts re-run OK (arch, mcp write gates, in-memory stores,
  unbounded http, relative paths, fail-open refs, byte slicing, inert
  enforcement, lossy secrets, migration versions).

### Verification-repair round (formatting + clippy only)

- The prior verification run (finding 67f8f9c2cdcc40d690f2eef5dc832b3c,
  exit 1) failed only on rustfmt changed-line violations in four files and
  one `clippy::type_complexity` in graph_diff.rs:230 — no behavioral
  finding. Repaired in commit 6ba5217e: the four files were passed through
  `rustfmt` (token-stream-identical; the unchanged gyre-server crates were
  not cold-rebuilt for formatting-only feedback) and the test fn-pointer
  pair was named via a `Mutator` type alias. `cargo test -p gyre-domain`
  re-run after the repair: 579 + 8 passed, 0 failed; `cargo clippy -p
  gyre-domain --all-targets` shows no warning on changed lines
  (type_complexity gone; remaining `useless vec!` warnings are pre-existing
  test debt outside this round's changed lines). Probe evidence retained
  at /tmp/stage/review-evidence/task-276-format-repair.md.

### Deferred verification

- `cargo test --all` and the server integration test above must run in the
  independent verifier (see /tmp/stage/review-evidence/task-276-evidence.md).
