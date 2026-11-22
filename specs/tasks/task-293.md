---
title: "Spec Links — cycle detection over the tenant-wide forge graph, not just the pushed manifest"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-tenant-cycle-detection / 5a915155d9bc"
depends_on: [task-287]
progress: ready-for-review
commits: ["092db23015876d452327beab9f04f5c6961edd77", "0680e21a605bb9a07051c106ec7c1141fc212675", "1fb8cfa28c0a18ce4da633c6e609a11b61fe5664", "34b3711d493ddd94ab7cf86ce39eb758837d8c20", "824c68eefc565f0f45f11e92dcbb01d7e87ea0e4"]
---

## Shipped

Cycle detection runs over the tenant-wide forge graph, not just the pushed manifest. `check_spec_link_cycles` (crates/gyre-server/src/spec_registry.rs) takes a `TenantCycleContext`; `build_tenant_cycle_graph` combines every stored row in the durable spec_links table with the pushed manifest's edges, keyed by (repo, path): manifest entries attributed to the source repo, cross-repo/cross-workspace targets resolved through `resolve_manifest_link_target` — the identical resolution `sync_spec_ledger` persists, so the checked graph and the stored graph attribute nodes identically. Rows the push itself replaces (`replace_for_source` scoping) are excluded from the stored half, so a push that REMOVES a cycle-participating edge is accepted. Storage errors reading the durable graph fail closed (push rejected). Only cycles in which at least one edge contributed by the pushed manifest participates are reported, so stored-only legacy cycles do not block unrelated repos. Rejection wiring in git_http.rs is unchanged in shape: ref updates undone, PushRejected event, git_push_rejected audit (gate_name "spec-link-cycles").

### Review round 2 (34b3711d)

Fail-closed on `list_all` storage errors and push-scoped rejection — two scope corrections from the independent review (full detail in coverage row 11, specs/coverage/system/spec-links.md).

### Review round 3 (finding 9f245c50) — deferred-cycle bypass closed

The round-2 code dropped unresolved cross-repo stored rows (target_repo_id NULL) from the combined graph, but the daily staleness job deterministically activates exactly those rows once the target repo's manifest declares the target path — so a cycle could arrive as push 1 (edge stored unresolved because the target path was not yet declared), push 2 (target repo declares the path and adds the back-edge; both pushes accepted), then staleness activation, landing permanently in the forge graph with no push ever rejected. Both halves of the reviewer's "either closes it" are implemented:

- **Push-time attribution (primary).** `attribute_unresolved_stored_link` (spec_registry.rs) attributes an unresolved stored row exactly when the staleness pass will activate it — the same source-scope derivation (source repo → workspace → tenant, never a fabricated scope), the same target-display disambiguation, and the same target-manifest requirement, with the pushed manifest standing in for the target repo's current manifest when the target repo is the one being pushed (activation is deterministic given the push's own content). Access-denied rows never activate and are never attributed (durable kv denial marker; new `TenantCycleContext.kv` field wired from `state.kv_store` in git_http.rs). The step-2 push of the reviewer's reproduction is now rejected with the cycle named.
- **Activation gate (backstop).** `resolve_unresolved_cross_workspace_links` (spec_link_staleness.rs) refuses to flip a pending row active when that would create a cycle in the durable graph (`link_activation_creates_cycle` over the attributed graph, shared `bfs_path` helper refactored out of `detect_graph_cycles`); the row stays unresolved with a warning naming the cycle. No activation path — including a linkless push that merely declares the target path, or mirror-synced state changes that bypass the push gate — can land a cycle the push-time gate would have rejected.

### Test evidence

Unit (spec_registry.rs, real git repos + mem adapters, real post-push `sync_spec_ledger` storing push 1's edge): `tenant_cycle_cross_repo_second_push_rejected` (AC1: A→B accepted, B→A rejected naming both repo-attributed nodes), `tenant_cycle_references_back_edge_accepted` and `tenant_cycle_conflicts_with_bidirectional_not_a_cycle` (AC3), `tenant_cycle_single_manifest_cycle_rejected_with_tenant_context` (AC2), `tenant_cycle_push_removing_edge_accepted` (replace semantics), `tenant_cycle_unresolved_stored_edge_not_attributed` (unchanged: its pushed manifest contributes no edges, so conditional attribution changes no outcome), round-2 regressions `tenant_cycle_list_all_error_fails_closed` and `tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push`, and the round-3 regression `tenant_cycle_deferred_unresolved_back_edge_rejected` — the reviewer's three-step reproduction: push 1 stores B→A unresolved (asserted status "unresolved", target_repo_id None, display "@A/system/a.md" so the deferred state is genuinely reproduced), push 2 (repo A declares system/a.md and links A→B) MUST be rejected naming repo-a:system/a.md and repo-b:system/b.md — it returns Ok on the pre-repair candidate. Staleness (spec_link_staleness.rs): `staleness_refuses_activation_that_creates_cycle` (pending B→A with stored active A→B stays unresolved after `run_once`), with the existing activation tests proving activation still works when no cycle would be created. E2e (git_http.rs): `cross_repo_spec_link_cycle_second_push_rejected_e2e` through the real git-receive-pack endpoint (FORBIDDEN, ref rolled back, audit + workspace message asserted).

This round's probes: `cargo check -p gyre-server --tests` exit 0 with zero warnings, `cargo fmt -p gyre-server -- --check` clean, no clippy findings in the new code. Server test execution is deferred to the independent verifier per the 4 GiB build-wrapper budget — the wrapper denial, full source trace, and the required cargo test probes are recorded in /tmp/stage/review-evidence/review-round3-deferred-cycle-repair.txt.

Coverage row 11 (spec-links.md Cycle Detection) carries the concrete source evidence; pending independent review for verified.

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
    "detail": "Push-time rejection is real and the excluded types match the spec (conflicts_with bidirectional, references and supersedes excluded; depends_on/implements/extends participate), with default-branch pushes undone and a PushRejected event plus audit on rejection. The gap is detection scope: detect_link_cycles builds its adjacency list exclusively from the single manifest being pushed -- the forge's tenant-wide spec graph (the durable spec_links table, other repos' manifests, or even previously synced links of the same repo) is never consulted. A cycle whose edges are declared in two different manifests or repos -- the enterprise case this spec's Problem section exists for -- is therefore accepted: each push only sees its own edges, so A→B in repo 1 and B→A in repo 2 both land cleanly and the cycle lives permanently in the forge-maintained graph.",
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
