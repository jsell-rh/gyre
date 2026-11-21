---
title: "Spec Links — cross-repo link validation: same-workspace format resolution, linking-user read access, target-manifest existence, tenant-scoped re-resolution"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-cross-repo-resolution / 5a915155d9bc"
depends_on: []
progress: not-started
commits: ["5afb12e079b522701b1e24a263634a5500d8b205"]
---

## Required behavior

spec-links.md §Cross-Repo and Cross-Workspace Links: "The link is validated: Target workspace exists and the linking user has read access / Target repo exists within that workspace / Target spec path exists in the target repo's manifest / Target SHA matches (or is flagged as stale)". Four requirements are not delivered: (1) same-workspace cross-repo targets (@{repo_name}/{spec_path}, an explicitly specced format) never resolve — sync_spec_ledger passes None as workspace to resolve_repo_by_name, which returns None whenever workspace_id is None, and the staleness checker's re-resolution explicitly refuses CrossRepo links; (2) the linking user's read access is never validated (resolution runs post-push with no caller identity in scope); (3) a link whose repo resolves is marked "active" with no check that the target repo's manifest contains the target path; (4) resolve_unresolved_cross_workspace_links queries find_by_slug under the literal fabricated tenant id "default" (also violating the repo's fabricated-scope-defaults invariant). Deliver: (1) resolve same-workspace cross-repo links by passing the source workspace id (already a sync_spec_ledger parameter) to resolve_repo_by_name at push-time sync, and re-resolve unresolved CrossRepo links in the staleness pass by looking up the source link's repo → workspace. (2) Thread the pushing principal's identity (available as the authenticated agent id in the git_http push handler) into sync_spec_ledger; when a cross-workspace target resolves, validate the linking principal has read access to the target workspace (workspace membership/role lookup; system principals exempt); on failure store the link "unresolved" with target_repo_id NULL and a warning naming the principal and target — never "active". (3) When the target repo resolves, read the target repo's current specs/manifest.yaml (repo storage path + the same git-show mechanism sync already uses) and require the target path to be declared; otherwise store the link "unresolved" with a warning naming the missing path. (4) Derive the tenant for cross-workspace re-resolution from the source link (source repo → workspace → tenant) instead of the literal "default"; if the scope cannot be determined, skip and log, never fabricate. All shared sync entry points (git_http, api/repos.rs, mirror_sync) get the behavior via sync_spec_ledger.

## Acceptance Criteria

- [ ] A manifest in repo A (workspace W) declaring depends_on @B/system/auth.md, where repo B exists in W, resolves to B's repo id with status "active", and a subsequent change to B's target spec marks A's link stale.
- [ ] A cross-workspace link pushed by a principal without read access to the target workspace is stored "unresolved" (not active), with the denial named in logs.
- [ ] A link whose target repo resolves but whose target path is absent from the target repo's manifest is stored "unresolved" with the missing path named.
- [ ] In a tenant whose id is not "default", an unresolved cross-workspace link whose target workspace exists in that tenant re-resolves on the next staleness pass (no fabricated "default" tenant lookup).

## Verification intent

In workspace W of tenant T (T ≠ literal "default"), push to repo A a manifest with link @B/path.md where B exists in W: before the fix the link is stored "unresolved" with target_repo_id NULL forever (push-time resolution passes None as workspace; the staleness job refuses CrossRepo re-resolution); after the fix GET /api/v1/specs/{source}/links shows target_repo_id = B and status "active", and pushing a SHA change to B's target spec marks A's link stale — on current code that staleness never fires for this link. Second probe: declare a link to @B/nonexistent.md (path not in B's manifest) — before the fix it is stored "active"; after, "unresolved".

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "ff74a147e0ca35a407836447",
    "decision": "gap",
    "requirement_quote": "The forge resolves cross-repo links by querying the tenant-wide spec graph. The link is validated:\n- Target workspace exists and the linking user has read access\n- Target repo exists within that workspace\n- Target spec path exists in the target repo's manifest\n- Target SHA matches (or is flagged as stale if the spec has moved on)",
    "detail": "Target formats (@{workspace_slug}/{repo_name}/{spec_path}, @{repo_name}/{spec_path}, bare path) parse correctly, cross-workspace workspace+repo existence is resolved tenant-scoped, and target-SHA drift is flagged stale against the ledger. Four validation requirements are not delivered: (1) same-workspace cross-repo links (@{repo_name}/{spec_path}, an explicitly specced format) never resolve -- sync_spec_ledger passes None as the workspace to resolve_repo_by_name, which returns None whenever workspace_id is None, so every such link is stored \"unresolved\" with target_repo_id NULL even when the repo exists, and the staleness job's re-resolution explicitly leaves CrossRepo links unresolved; (2) the linking user's read access is never validated -- link resolution runs post-push with no caller identity in scope; (3) target spec path existence in the target repo's manifest is never validated -- a link whose repo resolves is marked \"active\" without checking the target repo's manifest contains the path; (4) the staleness checker's cross-workspace re-resolution queries under a hardcoded fabricated tenant id \"default\", so in any other tenant the workspace slug never resolves (also violating the repo's fabricated-scope-defaults invariant).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                        let resolved_repo_id = resolve_repo_by_name(\n                            repos, workspaces, tenant_id,\n                            None, // same workspace — we'll look up by name across all\n                            repo_name,\n                        )\n                        .await;",
        "detail": "Same-workspace cross-repo resolution at push time passes None as workspace_id; the comment claims 'look up by name across all' but the callee cannot."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "    let ws_id = workspace_id?;\n    repos",
        "detail": "resolve_repo_by_name immediately returns None when workspace_id is None (`let ws_id = workspace_id?;`), so every @{repo_name}/path link is permanently unresolved despite the repo existing -- 'Target repo exists within that workspace' is never positively validated for this format."
      },
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "quote": "            crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {\n                // For cross-repo same-workspace links, we'd need the source workspace.\n                // Without it, we can't resolve. Leave as unresolved.",
        "detail": "The staleness checker (which the spec says resolves NULL target_repo_id links later) explicitly refuses to re-resolve same-workspace cross-repo links, leaving them unresolved forever."
      },
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "quote": "                let ws = state\n                    .workspaces\n                    .find_by_slug(&Id::new(\"default\"), workspace_slug)",
        "detail": "Cross-workspace re-resolution fabricates the tenant scope as the literal id \"default\" instead of the source link's tenant, so workspaces in any other tenant never resolve."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                        let status = if resolved_repo_id.is_some() {\n                            \"active\".to_string()\n                        } else {\n                            \"unresolved\".to_string()",
        "detail": "A cross-repo link whose repo resolves is marked \"active\" with no check that the target spec path exists in the target repo's manifest -- only repo existence is consulted, never the target manifest; the linking user's read access appears nowhere in this path."
      }
    ]
  }
]
