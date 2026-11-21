---
title: "Spec Links — approval gates: task-based depends_on blocking, implements approved-at-pinned-SHA, conflict-resolution task on rejected approval, merge-gate warning on the same shared predicate"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-approval-gates / 5a915155d9bc"
depends_on: [task-286]
progress: not-started
commits: []
---

## Required behavior

Approval Gates: three rows deviate from the contract; Merge Gates: the depends_on warning judges "isn't implemented yet" by the target's ledger approval_status (merge_processor.rs Check 3, ~2395-2410), which per controller review feedback (cb0d56cefcb50d54be0121c4) was never the specced signal — and since this task removes the approval-based model, the merge gate must not keep using it. (1) depends_on ("Implementation tasks for source spec are blocked until target spec's implementation tasks are complete"): remove the approval-time DependsOn rejection in approve_spec (it blocks approval of the source and models completeness as the target's approval_status — per spec-lifecycle.md approval precedes decomposition, so approval never implied implementation). Install the specced mechanism: implementation tasks of a spec with an outbound depends_on link (tasks with spec_path == that spec, task_type Implementation / label spec-implementation) cannot be started while any depends_on target's implementation is incomplete. Implementation-complete predicate, SHARED VERBATIM by the merge gate below: the target spec has at least one implementation task (tasks.list_by_spec_path) and all of them are Done — equivalently, incomplete = NOT (at least one implementation task AND all Done), which includes the zero-task case. Enforcement: spawning an agent for, or starting, such a task is refused with an error naming the unsatisfied dependency spec; affected tasks surface as Blocked with a recorded reason; once the target's implementation tasks are all Done, the same task starts successfully (re-evaluated at start/spawn). (2) implements ("cannot be approved until target (parent) spec is approved at the pinned SHA"): in addition to parent Approved, approve_spec's implements gate must compare the link's pinned target_sha to the parent's current ledger SHA and reject on mismatch with a message naming the pinned and current SHAs. (3) conflicts_with ("The forge rejects the second approval and creates a conflict-resolution task"): keep the existing rejection with its specced message, and on rejection create a real durable conflict-resolution Task (tasks repository; workspace/repo-scoped to the spec being approved; labeled conflict-resolution; High priority; describing both conflicting specs and the link). (4) Merge Gates depends_on warning (merge_processor.rs Check 3): replace the approval-status predicate with the EXACT shared predicate from item (1) — emit the warning (never a block: existing StaleSpecWarning event plus warn log, merge proceeds) whenever the referenced spec has an outbound depends_on link whose target does NOT satisfy (at least one implementation task AND all Done). Zero implementation tasks therefore warns: a depends_on target that has never been decomposed (the normal state per spec-lifecycle.md, where approval precedes decomposition) is not implemented and its dependency is not satisfied, so the warning must fire there too; an implementation that narrows item (4) to "has at least one implementation task and not all are Done" reintroduces the silent-warning defect in the most common case and is a contract violation. The superseded-spec rejection (Check 1, merge_processor.rs ~2291) and the conflicts_with block (Check 2, ~2317) remain unchanged — both verified real.

## Acceptance Criteria

- [ ] An implementation task for spec S (depends_on T) cannot be spawned or started while T has an open implementation task; after T's implementation tasks are all Done, the same spawn/start succeeds. Approving S is no longer rejected on depends_on (the gate sits on implementation tasks, per the spec's table).
- [ ] Approving a spec whose implements parent is Approved at a SHA different from the link's pin fails with a message naming both SHAs; approving while the parent is at the pinned SHA succeeds.
- [ ] A rejected second conflicts_with approval leaves a durable conflict-resolution task (label, both spec paths, correct workspace/repo scope) while the approval still fails with the specced message.
- [ ] An MR referencing a spec whose depends_on target is Approved with open (not-all-Done) implementation tasks emits the StaleSpecWarning warning event and the merge proceeds (warn, not block); an MR whose dependency's implementation tasks are all Done emits no warning.
- [ ] Zero-task case: an MR referencing a spec whose depends_on target is Approved with ZERO implementation tasks emits the StaleSpecWarning warning event and the merge proceeds (warn, not block) — the shared predicate's NOT (≥1 task AND all Done) covers the never-decomposed target.

## Verification intent

Approve spec T (ledger status Approved) while leaving one of T's spec-implementation tasks InProgress, then merge an MR referencing S which depends_on T: before the fix Check 3 sees approval_status == Approved and stays silent, so the merge proceeds with no warning despite the unimplemented dependency; after the fix the warning event is emitted and the merge still proceeds (asserting a block here would be wrong — the spec says warning). Second probe (zero-task target, the narrowed-predicate hazard): approve T and give it ZERO implementation tasks, then merge an MR referencing S — on an implementation using the narrowed "≥1 task and not all Done" predicate no warning is emitted and this probe fails; on the shared predicate the StaleSpecWarning event is emitted and the merge proceeds. Third probe: attempt to spawn an agent on S's implementation task while T's tasks are incomplete — before the fix the spawn succeeds (the old gate only blocked approval of S, never tasks); after, the spawn is refused with an error naming T, and completing T's tasks unblocks it.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "dcd440115bac4622a351c857",
    "decision": "gap",
    "requirement_quote": "| `implements` | Source spec cannot be approved until target (parent) spec is approved at the pinned SHA |\n| `depends_on` | Implementation tasks for source spec are blocked until target spec's implementation tasks are complete |\n| `conflicts_with` | Both specs cannot have `approval_status: approved` simultaneously. The forge rejects the second approval and creates a conflict-resolution task. |",
    "detail": "Two rows are really enforced: supersedes (approving the source automatically marks every superseded target Deprecated in the ledger, with a deprecation event) and extends (a changed target marks the extending spec drifted and resets its approval to Pending, in both the outbound and inbound sync paths). Three rows deviate from the contract: (1) depends_on -- the gate blocks APPROVAL of the source spec, not its implementation tasks, and judges \"implementation complete\" by the target's ledger approval_status; per spec-lifecycle.md approval precedes decomposition, so an approved target with open/incomplete implementation tasks passes the gate -- the specced signal (target spec's implementation tasks complete) is never consulted (no Task query anywhere in the gate). (2) implements -- the gate checks only the parent's current approval_status and never compares the approval to the link's pinned target_sha, so a source can be approved against a parent version it does not pin (\"approved at the pinned SHA\" is not verified). (3) conflicts_with -- the second approval is rejected with the specced message, but no conflict-resolution task is created anywhere in the rejection path (the handler returns InvalidInput immediately; a repository-wide search for conflict-resolution task creation finds none).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "                SpecLinkType::DependsOn => {\n                    // TASK-016: Target spec's implementation must be complete (approved)\n                    // before the depending spec can be approved.\n                    // spec-links.md §Approval Gates: \"Implementation tasks for source spec\n                    // are blocked until target spec's implementation tasks are complete.\"\n                    if let Some(target) = state.spec_ledger.find_by_path(&link.target_path).await? {",
        "detail": "depends_on gate: blocks approval of the source spec and models 'implementation complete' as the target's approval_status -- the specced mechanism (blocking implementation tasks until the target's implementation tasks are complete) is absent."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "                SpecLinkType::Implements => {\n                    // Parent spec must be approved before this spec can be approved.\n                    if let Some(parent) = state.spec_ledger.find_by_path(&link.target_path).await? {\n                        if parent.approval_status != ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': implements '{}' which is not yet approved\",\n                                spec_path, link.target_path",
        "detail": "implements gate: checks only parent.approval_status != Approved; the link's pinned target_sha is never compared, so 'approved at the pinned SHA' is not enforced."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "                    {\n                        if conflicting.approval_status == ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': conflicts with '{}' which is already approved — resolve the conflict first\",\n                                spec_path, link.target_path",
        "detail": "conflicts_with gate: rejects the second approval but immediately returns the error -- no conflict-resolution task is created, contrary to 'The forge rejects the second approval and creates a conflict-resolution task.'"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "                if target_entry.approval_status != ApprovalStatus::Deprecated {\n                    target_entry.approval_status = ApprovalStatus::Deprecated;\n                    target_entry.updated_at = now;\n                    let _ = state.spec_ledger.save(&target_entry).await;",
        "detail": "supersedes row is satisfied: approving the superseding spec marks the target Deprecated in the ledger (plus a StaleSpecWarning deprecation event)."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                            source_entry.drift_status = \"drifted\".to_string();\n                            source_entry.approval_status = ApprovalStatus::Pending;\n                            source_entry.updated_at = now;",
        "detail": "extends row is satisfied: when the target changes, the extending spec is marked drifted and its approval is reset to Pending (both in the outbound sync path shown and the inbound path)."
      }
    ]
  },
  {
    "id": "cb0d56cefcb50d54be0121c4",
    "decision": "gap",
    "requirement_quote": "- If the spec `depends_on` another spec that isn't implemented yet, the MR gets a warning (not a block - the code might be valid, but the dependency isn't satisfied yet)",
    "detail": "Two of the three bullets are real: the superseded-spec rejection (Check 1 rejects the merge with exactly the specced message \"spec '{path}' has been superseded by '{new_path}'. Update your spec_ref.\") and the conflicts_with block (Check 2 blocks, bidirectionally, when the conflicting spec is approved). The depends_on-warning bullet is a gap: 'isn't implemented yet' is judged by the target spec's ledger approval status, not by implementation-task completeness. The spec's own vocabulary denotes task completeness (Approval Gates: \"until target spec's implementation tasks are complete\"), and per spec-lifecycle.md approval precedes decomposition, so approval never implied implementation -- the warning stays silent exactly in the common case the bullet exists for (target spec approved, implementation tasks incomplete). This corrects the prior cycle's 'satisfied' disposition per the controller review feedback; the warning event itself (StaleSpecWarning, warn-not-block) is correctly shaped. Per controller review feedback this includes the zero-task case: a depends_on target that has never been decomposed (zero implementation tasks — the normal state per spec-lifecycle.md, where approval precedes decomposition) is not implemented and its dependency is not satisfied, so the warning must fire there too; the approval-status predicate stays silent for every Approved target regardless of implementation-task state, including that most common case.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "                if let Ok(Some(target)) = state.spec_ledger.find_by_path(&link.target_path).await {\n                    if target.approval_status != crate::spec_registry::ApprovalStatus::Approved {\n                        warn!(",
        "detail": "Check 3 predicate: the depends_on warning fires only when the target's approval_status != Approved. A target that is Approved with open implementation tasks produces no warning, so 'isn't implemented yet' is approximated by approval rather than judged by implementation completeness."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "            if link.link_type == crate::spec_registry::SpecLinkType::Supersedes\n                && link.target_path == spec_path\n            {\n                let reason = format!(\n                    \"spec '{}' has been superseded by '{}'. Update your spec_ref.\",\n                    spec_path, link.source_path\n                );",
        "detail": "Superseded-spec bullet is satisfied: the merge is rejected with the specced message whenever the referenced spec has an inbound supersedes link."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "                        let reason = format!(\n                            \"spec '{}' conflicts with approved spec '{}' — resolve the conflict first\",\n                            spec_path, other_spec_path",
        "detail": "Conflicts_with bullet is satisfied: the merge is blocked when the referenced spec conflicts with an approved spec (checked in both directions)."
      }
    ]
  }
]
