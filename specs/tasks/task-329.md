---
title: "§9 MetaSpecPolicy in full: warn_on_drift/block_on_drift/drift_tolerance with per-meta-spec version-count reasoning, real server-side policy storage, and version-named drift warnings"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / merge-gate-drift-policy / b288723d2f73"
depends_on: [task-302]
progress: not-started
commits: ["532681145499d676af92b79f405dec8c678591d8"]
---

## Required behavior

Per §9, the merge-gate drift policy is the MetaSpecPolicy struct, not a single boolean — agent-runtime.md's amendments-table row explicitly retains it ('The MetaSpecPolicy (§9 of reconciliation spec) is retained — it controls merge-time drift enforcement and is stored as a workspace-level setting. Drift detection compares the meta_specs_used array in merge attestations against current required meta-spec versions.'). Today the enforced policy is require_current_meta_spec_set (merge_policy.rs) — strict mode blocks on any set-SHA mismatch (tolerance-0 only), no version-count reasoning exists anywhere, and the workspace settings UI renders the full specced policy and saves it via api.updateWorkspace({meta_spec_policy}) but UpdateWorkspaceRequest has no such field, so serde silently drops the payload and GET never returns it — the §9 policy UI is theater. COORDINATION: active task-302 migrates merge-time drift comparison to the meta_specs_used array (already recorded per merge in MergeAttestation from the authoring agent's prompt-set record — MetaSpecUsed {id, kind, content_hash, version}); build the policy on that versioned basis. Deliverables: (1) Policy shape — MetaSpecPolicy {warn_on_drift: bool = true, block_on_drift: bool = false, drift_tolerance: u32 = 1} persisted per workspace; migration/compat maps existing require_current_meta_spec_set: true to block_on_drift: true with drift_tolerance: 0 (preserving today's block-on-any-drift behavior for existing strict workspaces); both the merge-policy endpoint and the workspace settings surface expose the full shape. (2) warn_on_drift — false suppresses the [WARN] meta-spec-drift gate result (default true keeps today's always-warn); in strict mode (block_on_drift: true) warnings are always recorded regardless of warn_on_drift ('Always on in strict mode'). (3) drift_tolerance version-count reasoning — the merge-queue drift check compares the MR's provenance-recorded governing versions (meta_specs_used: id + version) against the current registry versions per meta-spec: versions_behind = current_version − recorded_version; the MR blocks only when some meta-spec is MORE than drift_tolerance versions behind (0 = block on any drift); within tolerance → warn and proceed. Legacy records with no per-meta-spec versions (hash-only) cannot prove currency: under block_on_drift they are treated as unverifiable and blocked (never fail-open); under warn they warn. (4) Warning naming — the [WARN] output and block reason name the per-meta-spec version transition(s) ('meta-spec-drift: {kind} {name} v{old} -> v{new}') derived from meta_specs_used vs current versions, not only set SHAs. (5) UI wiring fixed — UpdateWorkspaceRequest gains meta_spec_policy (warn_on_drift/block_on_drift/drift_tolerance, tolerance validated ≥ 0), persisted and returned by GET workspace, so the existing WorkspaceSettings save round-trips for real.

## Acceptance Criteria

- [ ] PUT /api/v1/workspaces/{id} with {meta_spec_policy: {warn_on_drift: true, block_on_drift: true, drift_tolerance: 1}} then GET the workspace returns exactly that policy (no silent drop); invalid tolerance (< 0) is rejected 400.
- [ ] With that policy, an MR whose authoring agent's meta_specs_used records a persona at v3 where the registry is at v4 (one version behind) merges with a [WARN] gate result naming 'persona … v3 -> v4'; an MR recorded at v2 (two versions behind) is blocked and requeued with the drift reason.
- [ ] A workspace migrated from require_current_meta_spec_set: true blocks a one-version-behind MR (tolerance 0 compatibility); a default workspace (no policy) warns and merges on any drift.
- [ ] warn_on_drift: false (non-strict) suppresses the [WARN] gate result; warn_on_drift: false with block_on_drift: true still records the warning alongside the block.

## Verification intent

Arm 1 (round-trip): PUT the workspace with meta_spec_policy then GET it — at the base the field is silently discarded (UpdateWorkspaceRequest has no meta_spec_policy field; grep finds no meta_spec_policy anywhere in Rust) while the web UI (WorkspaceSettings.svelte:131-143,899) renders and saves it, so GET returns nothing and the probe fails. Arm 2 (tolerance semantics): with block_on_drift + drift_tolerance 1, enqueue the one-version-behind MR and assert it merges with a [WARN]; at the base the strict boolean (require_current_meta_spec_set) blocks BOTH the one-behind and two-behind MRs on any set-SHA mismatch — tolerance cannot be expressed anywhere in the enforcement path (merge_policy.rs holds a single bool; no version-count reasoning exists), so the one-version-behind MR is wrongly blocked and the probe fails.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
  {
    "id": "d0c7637a52d62fe7a81b940e",
    "decision": "gap",
    "requirement_quote": "    /// How many versions behind before blocking (if block_on_drift is true).",
    "detail": "The opinionated default and strict mode are real: the merge processor checks every merge-queue cycle for meta-spec drift (the MR's provenance-attested set SHA vs the workspace's current effective set SHA), records a [WARN] meta-spec-drift gate result and merges (warn, don't block), and a per-workspace opt-in strict policy converts drift into a hard block (entry requeues with the drift reason until reconciled or relaxed) via GET/PUT /workspaces/:id/merge-policy. The gap is the specced MetaSpecPolicy shape: the enforced policy is a single boolean (require_current_meta_spec_set). warn_on_drift is not configurable (warnings are always recorded — acceptable as the default is true and 'always on in strict mode'), but drift_tolerance — how many versions behind before blocking, default 1 — does not exist anywhere in the enforcement path: strict mode blocks on any set-SHA mismatch (tolerance-0 behavior only), and no version-count reasoning is computed. Worse, the workspace settings UI renders the full specced policy (warn_on_drift/block_on_drift/drift_tolerance checkboxes and tolerance input) and saves it via api.updateWorkspace({meta_spec_policy: ...}) — but UpdateWorkspaceRequest has no meta_spec_policy field, so the server silently drops the payload (serde ignores unknown fields) and GET never returns it: the §9 policy UI is theater, and a workspace wanting tolerance-1 blocking has no mechanism. The drift warning also names set SHAs rather than 'persona backend-developer v3 -> v4' (per-meta-spec version naming), since the recorded provenance is a set hash.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/merge_policy.rs",
        "quote": "pub require_current_meta_spec_set: bool,",
        "detail": "The entire enforced §9 policy: one boolean. No warn_on_drift toggle, no block_on_drift separate from the single strict flag, no drift_tolerance — the specced MetaSpecPolicy struct's tolerance semantics ('versions behind before blocking') have no server-side representation."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "if policy.require_current_meta_spec_set {",
        "detail": "The strict-mode enforcement is real (drift → hard block with requeue reason; default warn-and-proceed via record_meta_spec_drift_warning) — the behavior half of §9 works, but only at tolerance 0."
      },
      {
        "file": "web/src/components/WorkspaceSettings.svelte",
        "quote": "warn_on_drift: warnOnDrift,",
        "detail": "The settings UI collects the full specced MetaSpecPolicy (warn_on_drift/block_on_drift/drift_tolerance, lines 131-133 and the form at ~1128-1151) and saveDriftPolicy PUTs it as meta_spec_policy on the workspace — fields the server's UpdateWorkspaceRequest does not define, so the save is silently discarded and drift_tolerance can never take effect."
      },
      {
        "file": "crates/gyre-server/src/api/workspaces.rs",
        "quote": "pub struct UpdateWorkspaceRequest {",
        "detail": "The workspace update request enumerates name/description/budget/max_repos/max_agents_per_repo/trust_level/llm_model/compute_target_id — no meta_spec_policy field (grep over all crates finds no meta_spec_policy anywhere in Rust): the UI's §9 policy payload is dropped by serde's unknown-field tolerance, and the workspace response never carries it back."
      }
    ]
  }
]
