---
title: "Spec Links — cycle detection over the tenant-wide forge graph, not just the pushed manifest"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-tenant-cycle-detection / 5a915155d9bc"
depends_on: [task-287]
progress: ready-for-review
commits: ["34b3711d493ddd94ab7cf86ce39eb758837d8c20", "1fb8cfa28c0a18ce4da633c6e609a11b61fe5664", "0680e21a605bb9a07051c106ec7c1141fc212675", "092db23015876d452327beab9f04f5c6961edd77"]
---

## Shipped

Cycle detection runs over the tenant-wide forge graph, not just the pushed manifest. `check_spec_link_cycles` (spec_registry.rs) accepts a `TenantCycleContext`; `build_tenant_cycle_graph` combines every stored row in the durable spec_links table with the pushed manifest's edges, keyed by (repo, path): manifest entries attributed to the source repo, cross-repo/cross-workspace targets resolved through `resolve_manifest_link_target` — the identical resolution `sync_spec_ledger` persists, so the checked graph and the stored graph attribute nodes identically. Rows the push itself replaces (the `replace_for_source(repo, path)` scoping) are excluded from the stored half, so a push that REMOVES a cycle-participating edge is accepted. Unresolved cross-repo rows (target_repo_id NULL) are never re-attributed. Rejection wiring in git_http.rs is unchanged in shape: a rejection undoes the ref updates, emits PushRejected, and records a git_push_rejected audit with gate_name "spec-link-cycles".

### Review-round repair (34b3711d)

The independent review (cc861367) found two defects in the first implementation; both are repaired:

- **Fail closed on storage errors.** `build_tenant_cycle_graph` previously read the durable graph with an `if let Ok(stored)` guard on `list_all` — a storage error (SQLite "database is locked", pool/disk failure) silently dropped the stored half and degraded the gate to the pushed manifest's own edges, re-opening the pre-task-293 scope exactly when the graph was unreadable. It now returns `Result<SpecCycleGraph, String>` and propagates the error (warn! log plus a "spec link cycle check failed: unable to read the stored spec link graph" rejection), so the push is rejected and audited when the graph cannot be read — the same fail-loud contract as the task-264 signing gate.
- **Reject only cycles the push creates.** `detect_graph_cycles` previously scanned every node of the combined graph, so any manifest push was rejected while a stored-only cycle existed anywhere — legacy pre-gate rows (the exact state this task's discovery evidence describes) or ungated mirror-synced edges would block every manifest push in the deployment with no remedy for unrelated repos. Detection is now scoped: the graph tracks which edges the pushed manifest contributed (`SpecCycleGraph.pushed`), and a cycle is reported only when at least one pushed edge participates (each pushed edge u→v is checked with a BFS from v back to u through the combined graph). All three acceptance criteria hold under this scoping: the AC1 second push's B→A edge participates, single-manifest cycles participate, and references/conflicts_with remain excluded.

### Test evidence

Unit tests (spec_registry.rs, real git repos + mem adapters, real post-push `sync_spec_ledger` storing push 1's edge): `tenant_cycle_cross_repo_second_push_rejected` (AC1: A→B accepted, B→A rejected naming repo-a:system/a.md → repo-b:system/b.md), `tenant_cycle_references_back_edge_accepted` and `tenant_cycle_conflicts_with_bidirectional_not_a_cycle` (AC3), `tenant_cycle_single_manifest_cycle_rejected_with_tenant_context` (AC2), `tenant_cycle_push_removing_edge_accepted` (replace semantics; fixture repaired in 1fb8cfa2 after the verifier's attempt 4db732c6 caught a byte-identical-commit fixture bug), `tenant_cycle_unresolved_stored_edge_not_attributed` (no fabricated edges), plus the round-2 regression tests `tenant_cycle_list_all_error_fails_closed` (FailingSpecLinkRepository: list_all errors → push rejected, not silently accepted) and `tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push` (stored A↔B cycle: repo C's acyclic push accepted AND repo A re-declaring the participating edge still rejected — the gate was scoped, not disabled). E2e (git_http.rs): `cross_repo_spec_link_cycle_second_push_rejected_e2e` pushes through the real git-receive-pack endpoint across two repos in one workspace — push 2 returns FORBIDDEN with the cycle named, refs/heads/main rolled back to the pre-push SHA, git_push_rejected audit (gate "spec-link-cycles") and push_rejected workspace message asserted.

This round: `cargo check -p gyre-server --tests` exits 0 with zero warnings (two runs, before and after rustfmt); `cargo fmt -p gyre-server -- --check` clean. Server test execution is deferred to the independent verifier: the build wrapper denies gyre-server test codegen in this sandbox (exit 64, "gyre-server code generation plus the model exceeds 4 GiB" — denial recorded in /tmp/stage/review-evidence). Required probes: `cargo test -p gyre-server --lib spec_registry::tests::tenant_cycle`, `cargo test -p gyre-server --lib spec_registry::tests::cycle_detection`, `cargo test -p gyre-server --lib git_http::tests::cross_repo_spec_link_cycle_second_push_rejected_e2e`, and `cargo test -p gyre-server --lib spec_registry::tests::tenant_cycle_push_removing_edge_accepted` (confirming the 1fb8cfa2 fixture repair under the full suite).

Coverage row 11 (spec-links.md Cycle Detection) is implemented/task-293 with the round-2 source evidence; pending independent review for verified.

## Required behavior

spec-links.md §Cycle Detection: "The forge rejects manifest changes that would create cycles in the spec graph." Push-time rejection is real and the excluded types match the spec (depends_on/implements/extends participate; conflicts_with is bidirectional, references and supersedes excluded), with default-branch pushes undone and a PushRejected event plus audit on rejection — but detect_link_cycles builds its adjacency list exclusively from `manifest.specs` of the single manifest being pushed; the forge's tenant-wide spec graph (the durable spec_links table, other repos' manifests, previously synced links) is never consulted, so a cycle whose edges are declared in two different manifests or repos — the enterprise case the spec's Problem section exists for — is accepted and lives permanently in the forge-maintained graph. Extend check_spec_link_cycles (keeping the existing git_http.rs rejection wiring) to build the combined graph: all stored links in the durable spec_links table plus the pushed manifest's edges, with node identity as (repo, path) — the pushed manifest's entries attributed to the source repo, resolved target repo ids for cross-repo/cross-workspace targets, same-repo targets attributed to the source repo. A push that would create a cycle anywhere in the combined graph is rejected with a message naming the cycle. Cross-repo edges must be resolved to their repos for correct attribution (requires the cross-repo resolution task's fixes).

## Acceptance Criteria

- [ ] Pushing manifest edge A→B (depends_on) in repo 1 succeeds; then pushing manifest edge B→A (depends_on) in repo 2 is rejected with the cycle named, the ref update undone, and a PushRejected event emitted.
- [ ] Single-manifest cycles (A depends_on B depends_on A in one push) are still rejected exactly as today — no regression.
- [ ] Excluded link types never trigger rejection: a references-typed B→A second push is accepted, and conflicts_with bidirectionality is not reported as a cycle.

## Verification intent

The two-push cross-repo cycle: before the fix both pushes land cleanly (each push only sees its own manifest's edges) and GET /api/v1/specs/graph shows the A→B→A cycle living permanently in the forge graph; after the fix the second push is rejected at receive time. The companion assertion — a references-typed second push still accepted — proves the detection was widened rather than the gate made indiscriminate.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "8772dba48e530c04a3d2f953",
    "decision": "gap",
    "requirement_quote": "The forge rejects manifest changes that would create cycles in the spec graph:\n- A `depends_on` B `depends_on` C `depends_on` A -> rejected",
    "detail": "Push-time rejection is real and the excluded types match the spec (conflicts_with bidirectional, references and supersedes excluded; depends_on/implements/extends participate), with default-branch pushes undone and a PushRejected event plus audit on rejection. The gap is detection scope: detect_link_cycles builds its adjacency list exclusively from the single manifest being pushed -- the forge's tenant-wide spec graph (other repos/manifests, the durable spec_links table) is never consulted. A cycle whose edges are declared in two different manifests or repos -- the enterprise case this spec's Problem section exists for -- is therefore accepted: each push only sees its own edges, so A→B in repo 1 and B→A in repo 2 both land cleanly and the cycle lives permanently in the forge-maintained graph.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "    let mut adj: HashMap<String, Vec<String>> = HashMap::new();\n    for entry in &manifest.specs {\n        for link in &entry.links {\n            match link.link_type {",
        "detail": "Cycle detection adjacency is built only from `manifest.specs` of the one manifest being pushed; links already in the forge graph (other repos/manifests, the durable spec_links table) are never consulted, so cross-manifest cycles are not detected."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {\n            if let Err(rejection) =\n                crate::spec_registry::check_spec_link_cycles(&repo_path, &update.new_sha).await\n            {",
        "detail": "The rejection wiring itself is real: a failing cycle check on a default-branch push undoes the ref updates (rejection side confirmed; only the detection scope is deficient)."
      }
    ]
  }
]
