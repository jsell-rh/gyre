---
title: "The manifest is a spec: specs/manifest.yaml triggers lifecycle hooks and requires approval"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / manifest-self-registration / a458cd7cdf30"
depends_on: [task-334]
progress: not-started
commits: []
---

## Required behavior

Manifest Rule 2: "The manifest itself is a spec. Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval." Nothing currently treats the manifest as a spec: the repo's own manifest does not list itself (31 entries, no self-entry), `should_create_lifecycle_task` finds no manifest entry for it ('specs/manifest.yaml' normalizes to 'manifest.yaml', which no manifest declares), and the ledger has no row for it — manifest changes create no lifecycle task and require no approval. Deliver: (1) register specs/manifest.yaml as an entry in the repository's own specs/manifest.yaml (path `manifest.yaml`, title, owner, default requires_approval: true so the approval requirement flows from the existing machinery) and in the gyre-cli bootstrap starter manifest (crates/gyre-cli/src/bootstrap/starter-manifest.yaml) so bootstrapped repos get the same rule; (2) make the self-entry flow end-to-end through the generic machinery: ledger sync creates/updates a ledger row for manifest.yaml with its git blob SHA and resets approval_status to pending on content change (sync_spec_ledger already iterates manifest entries and resolves `specs/{path}` — confirm no special-case excludes the self-entry), the post-receive lifecycle hook creates a drift-review task for manifest.yaml modifications (manifest_entry_for_path normalization already matches a `manifest.yaml` entry), and the push-time coverage scan stays consistent (manifest.yaml is registered, so the existing exclusion produces no orphan warning); (3) any code that special-cases the manifest path in these paths must remain correct with the self-entry present. Depends on manifest-supersedes-lifecycle-config because rule 2 must hold regardless of the [spec_lifecycle] config block — the manifest must be authoritative for its own file's hook gating before self-registration guarantees that changes to it always trigger the hooks.

## Acceptance Criteria

- [ ] Pushing a modification to specs/manifest.yaml in a repo whose manifest lists itself creates a spec-drift-review lifecycle task whose spec_path points at the manifest, and the manifest's ledger row transitions approval_status to pending (old approvals stale on the new SHA).
- [ ] The repository's own specs/manifest.yaml contains a self-entry and GET /api/v1/specs lists the manifest as a registered spec with its current SHA and approval status.
- [ ] gyre bootstrap starter-kit repos register their manifest the same way (starter-manifest.yaml carries the self-entry).

## Verification intent

E2E: repo with self-registered manifest; push a commit modifying only specs/manifest.yaml (e.g. adding an unrelated entry). At the base no lifecycle task is created (no manifest entry matches 'manifest.yaml', so the manifest-mode gate returns false) and no ledger row exists for the manifest; the probe asserts (a) a drift-review task with spec_path specs/manifest.yaml exists in task storage and (b) the ledger row for manifest.yaml is pending. Both assertions fail on current code — proving the hook and ledger actually treat the manifest as a spec rather than the test merely passing a structural check.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "dc58139efeac521a97fffb3d",
    "decision": "gap",
    "requirement_quote": "2. **The manifest itself is a spec.** Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval.",
    "detail": "Rule 2 is not implemented at all: nothing treats specs/manifest.yaml as a spec. The repo's own manifest does not list itself as an entry, no code special-cases the manifest path in the lifecycle hook (in manifest mode, should_create_lifecycle_task creates a task only when the changed file has a manifest entry — 'specs/manifest.yaml' normalizes to 'manifest.yaml', which no manifest declares), and the manifest is not in the ledger, so manifest changes neither create lifecycle tasks nor require approval. Rules 1 and 3 (reject unregistered spec files; reject removing a manifest entry while keeping the file) share one mechanism — check_manifest_coverage at push time — which does reject with 403 FORBIDDEN, but only when the repo's spec policy sets enforce_manifest, and that flag defaults to false (warn-only); §Ledger Sync on Push explicitly permits 'reject push (or warn, policy-dependent)', so the policy gate itself is spec-compliant, but rules 1 and 3 as written ('the forge will reject') hold only for repos that opt in. Rule 4 (manifest as single source of truth over the spec_lifecycle config block) is only partially implemented — accounted in detail under the Spec Lifecycle integration section.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
        "detail": "Manifest-mode lifecycle task creation requires the changed path to have a manifest entry; 'specs/manifest.yaml' has no entry in any manifest (the repo's own specs/manifest.yaml does not list itself — verified by grep), and no special-case for the manifest path exists anywhere in process_spec_lifecycle (inspected the full hook: parse_name-status → should_create_lifecycle_task → classify → create task; the manifest file is diffed like any other file and dropped here). So changes to the manifest trigger no lifecycle hook and require no approval — rule 2 is missing."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            let enforce = state\n                .spec_policies\n                .get_for_repo(&repo_id)\n                .await\n                .map(|p| p.enforce_manifest)\n                .unwrap_or(false);",
        "detail": "Rules 1/3 push rejection is policy-gated and defaults to false (warn-only): a missing policy row or enforce_manifest=false means unregistered spec files and entry-removed-but-file-present states only log a warning, so the specced rejection is opt-in per repo rather than forge behavior."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "                return (StatusCode::FORBIDDEN, rejection).into_response();",
        "detail": "When enforce_manifest is set, the push IS rejected (403 with the unregistered-file list and ref updates undone), so the enforcement mechanism itself is real — the gap for rules 1/3 is only the default-off policy gate, while rule 2 has no implementation at all."
      }
    ]
  }
]
