---
title: "Spec Links — cycle detection over the tenant-wide forge graph, not just the pushed manifest"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-tenant-cycle-detection / 5a915155d9bc"
depends_on: [task-287]
progress: not-started
commits: ["092db23015876d452327beab9f04f5c6961edd77"]
---

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
