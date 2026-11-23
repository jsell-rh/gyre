---
title: "Merge-time spec-to-code binding consults the manifest: registered-path check and the requires_approval:false any-SHA exemption"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / merge-gate-manifest-policy / a458cd7cdf30"
depends_on: [task-337]
progress: not-started
commits: []
---

## Required behavior

Spec-to-Code Binding pre-accept validation: (1) the MR's spec_ref path must exist in the manifest; (2) if the referenced spec has `requires_approval: false`, any SHA is accepted. Today verify_spec_ref (crates/gyre-server/src/api/gates.rs:819) only queries approval rows and never reads specs/manifest.yaml: an unregistered path passes whenever an approval row exists, and an MR referencing a requires_approval:false spec — which per §9/§17 is Approved with no approval events at all — fails the require_approved_spec gate with 'no active approval in the ledger'. Extend the merge-time check (merge_processor.rs spec-policy block → verify_spec_ref) with the repo context it needs (the repo is at hand at the merge site; read specs/manifest.yaml at the repo's default-branch HEAD via the existing read_manifest machinery, using the repo's real scoped identity — no fabricated scope): if the spec_ref path has no manifest entry, reject naming the unregistered path; if the entry's effective requires_approval (per-entry override, else defaults) is false, accept regardless of approval rows — any SHA; otherwise the existing per-SHA approval check applies, evaluated against the unified ledger (dependency: building the check on the dual store being replaced would be discarded work). require_spec_ref and require_signed_approval policy semantics are preserved unchanged, and the signed-approval check keeps its own contract.

## Acceptance Criteria

- [ ] With repo policy require_approved_spec=true and a manifest entry declaring requires_approval: false, an MR whose spec_ref pins that spec at a SHA with zero approval rows merges (today it is blocked with 'no active approval in the ledger').
- [ ] An MR whose spec_ref path is not in the repo's manifest is blocked at merge with a reason naming the unregistered path, even when an active approval row for that path exists in the ledger.
- [ ] Registered specs with requires_approval:true keep today's behavior: an unapproved SHA blocks the merge, the approved SHA merges.

## Verification intent

Two-sided merge-queue probe (run_once on seeded state): (a) manifest entry system/exempt.md with requires_approval: false, no approval rows anywhere, MR spec_ref 'specs/system/exempt.md@<sha>', policy require_approved_spec=true — at the base the queue entry fails with 'spec '{path}' at SHA '{sha}' has no active approval in the ledger'; after the fix it must merge. (b) MR spec_ref 'system/ghost.md@<sha>' with a manually inserted ACTIVE approval row but no manifest entry — at the base it merges; after the fix it must be blocked naming the unregistered path. The two sides fail in opposite directions on current code, so a change that merely toggles the approval query (or hardcodes an exemption) cannot pass both.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "a81aa30dd4413677c04273d8",
    "decision": "gap",
    "requirement_quote": "- If the spec has `requires_approval: false`, any SHA is accepted",
    "detail": "The middle bullet is implemented (policy-gated): when a repo's spec policy sets require_spec_ref/require_approved_spec/require_signed_approval, the merge processor calls verify_spec_ref, which rejects merges whose spec_ref (path, sha) has no active approval in the per-SHA ledger — the referenced SHA must be approved. The other two bullets are not implemented: (1) the spec_ref path's existence in the manifest is never checked — verify_spec_ref queries only approval rows and never reads specs/manifest.yaml, so 'path must exist in the manifest' has no dedicated check (an unregistered path merely fails indirectly, and only when the approval policy is on); (2) the requires_approval: false exemption is missing — verify_spec_ref demands an active approval row regardless of the manifest's policy, and nothing in the merge path (merge_processor spec-policy block, verify_spec_ref, verify_signed_spec_ref — all inspected) consults the manifest or the requires_approval flag, so an MR referencing a spec with requires_approval: false — which per §9/§17 is Approved with no approval events — fails the require_approved_spec gate with 'no active approval in the ledger' because no approval rows exist for it.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/gates.rs",
        "quote": "    let for_sha = state\n        .spec_approvals\n        .find_by_spec_sha(path, sha)\n        .await\n        .unwrap_or_default();\n    let has_active_approval = for_sha.iter().any(|a| a.is_active());",
        "detail": "The approved-SHA check is real (bullet 2), but its entire inputs are the per-SHA approval rows: the manifest is never read, so neither the manifest-existence check (bullet 1) nor the requires_approval: false exemption (bullet 3) can be evaluated here."
      },
      {
        "file": "crates/gyre-server/src/api/gates.rs",
        "quote": "            \"spec '{path}' at SHA '{sha}' has no active approval in the ledger\"",
        "detail": "The only failure branch: a requires_approval: false spec with no approval rows gets this rejection when require_approved_spec is on — the specced 'any SHA is accepted' exemption has no code path (grep for requires_approval across the merge/gate modules returns nothing; the flag is only evaluated inside spec_registry's resolver and specs.rs request fixtures)."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "                if policy.require_approved_spec {\n                    if let Err(reason) = crate::api::gates::verify_spec_ref(state, spec_ref).await {",
        "detail": "The pre-accept call site: the inspected chain (this block → verify_spec_ref → spec_approvals.find_by_spec_sha) contains no manifest lookup and no requires_approval branch, so two of the section's three checks are absent from the enforced path."
      }
    ]
  }
]
