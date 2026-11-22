---
title: "Accountability patrol manifest-aware checks: orphans, stale entries, pending-beyond-one-cycle, modified-without-task keyed on the manifest"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / accountability-manifest-patrol / a458cd7cdf30"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

The Accountability agent's patrol gains the four manifest-aware checks from §Accountability Agent: (1) orphan spec files — files under specs/ at repo HEAD not registered in the manifest (reuse the file-scan class of the push-time coverage check in spec_registry.rs; once the manifest itself is registered it must not be flagged); (2) stale manifest entries — manifest entries whose file does not exist at HEAD (today only a sync-time warning log, never a patrol finding); (3) specs with effective requires_approval: true that have been pending for more than one Ralph loop cycle — this needs a real pending-since signal: durably record when a ledger entry's approval_status transitions to pending (e.g. a pending_since timestamp maintained by the sync/approve/reject paths, migration at the next unused sequence number at implementation time, portable across SQLite and PostgreSQL — or equivalent durable state; NOT updated_at, which churns on unrelated writes), and compare it against the patrol's cycle bound (the same one-Ralph-loop-cycle bound spec_lifecycle_patrol already uses — DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS, overridable per request); (4) specs with effective auto_create_tasks: true that were modified but have no corresponding task — rekey the existing modified-without-task defense-in-depth check in spec_lifecycle_patrol.rs from the config's watched prefixes to the manifest's auto_create_tasks, querying both spec_path spellings as the patrol already does. Findings surface through the patrol response and escalate to the workspace orchestrator via MessageKind::Escalation exactly like the existing checks (only with a resolvable workspace — no fabricated scope, per the standing constraint). Boundaries: this extends the spec-lifecycle patrol (spec_lifecycle_patrol.rs); it must not touch spec_patrol.rs (the spec-links graph patrol owned by task-292) or the push-time coverage rejection (task-017).

## Acceptance Criteria

- [ ] A repo with an unregistered specs/orphan.md on disk and a manifest entry pointing at a missing specs/ghost.md: one patrol run returns both findings and emits escalations to the owning workspace orchestrator's event stream.
- [ ] A requires_approval:true spec pending longer than the cycle bound produces a pending-age finding; the same spec pending less than the bound does not — the age comparison is evaluated against the recorded pending-since, not flagged unconditionally.
- [ ] A spec with auto_create_tasks:true whose SHA changed on a push but whose lifecycle task was deleted produces the modified-without-task finding even when the repo's [spec_lifecycle] config watches no prefixes (manifest-keyed, not config-keyed).

## Verification intent

Patrol-run probe with three seeded states: (a) specs/orphan.md on disk but absent from the manifest; (b) manifest entry system/stale.md with no file at HEAD; (c) a requires_approval:true ledger entry pending with a fresh pending-since (minutes old, well under the bound). At the base the patrol returns none of these — spec_lifecycle_patrol has no orphan, stale-entry, or pending-age check at all, and its coverage check is config-prefix-keyed. The probe asserts findings (a) and (b) are returned with real escalation rows on the orchestrator's stream, and that (c) is NOT returned; a check that flags (c) unconditionally (no real timestamp), returns findings without durable escalations, or only mirrors the push-time warning log fails the probe.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "2236f6ecfe981117d57f7990",
    "decision": "gap",
    "requirement_quote": "The Accountability agent's patrol gains manifest-aware checks:\n- Spec files that exist but aren't in the manifest (orphans)\n- Manifest entries whose files don't exist (stale entries)\n- Specs with `requires_approval: true` that have been `pending` for more than one Ralph loop cycle\n- Specs with `auto_create_tasks: true` that were modified but have no corresponding task",
    "detail": "None of the four manifest-aware patrol checks exists as a patrol check. The two live patrols check other contracts: spec_lifecycle_patrol (spec-lifecycle.md's accountability integration, task-204) checks drift-review/implementation task ages and modified-specs-without-tasks — but its coverage check keys on the repo's SpecLifecycleConfig watched prefixes, not the manifest's auto_create_tasks, and it has no orphan, stale-entry, or pending-age check; spec_patrol inspects the spec link graph (stale links, orphaned supersessions, unresolved conflicts), not manifest coverage. Orphan detection (files under specs/ not in the manifest) exists only as push-time warn/reject in check_manifest_coverage, not as an accountability patrol finding; stale manifest entries (entry with no file at HEAD) produce only a sync-time warning log; and there is no pending-age signal at all — the ledger entry has no last_checked/pending-since timestamp from which 'pending for more than one Ralph loop cycle' could be evaluated, so that check cannot even be expressed against current state.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
        "quote": "//! - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle\n//! - `spec-implementation` tasks that have been in Backlog for more than N days\n//! - Specs that were modified but have no corresponding task (should never happen",
        "detail": "The live lifecycle patrol implements spec-lifecycle.md's three checks, not this section's four manifest-aware checks — no orphan check, no stale-entry check, no requires_approval pending-age check."
      },
      {
        "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
        "quote": "        if !config.is_watched(&path) {\n            continue;\n        }",
        "detail": "Even the closest check (modified-without-task) keys on the repo's SpecLifecycleConfig watched prefixes — the path-prefix mechanism this spec's Problem section rejects — rather than the manifest's auto_create_tasks: true, so a manifest entry with auto_create_tasks enabled but outside the config's watched prefixes is never examined."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                \"spec-registry: file under specs/ is not registered in manifest.yaml — \\",
        "detail": "Orphan detection exists only as a push-time warning (or policy-gated rejection) inside check_manifest_coverage/sync — a log line, not an accountability patrol finding escalated to the orchestrator."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                    \"spec-registry: manifest entry has no corresponding file at HEAD\"",
        "detail": "Stale-entry detection (manifest entry with no file) is likewise only a sync-time warning log in sync_spec_ledger; no patrol surfaces it as a finding."
      },
      {
        "file": "crates/gyre-server/src/spec_patrol.rs",
        "quote": "//! - Orphaned supersessions: a spec is superseded but code still references it",
        "detail": "The other patrol inspects the spec link graph (stale/orphaned-supersession/unresolved-conflict links per spec-links.md), not manifest coverage — confirming the four manifest-aware checks in this section have no implementation anywhere (inspected both patrol modules and the patrol endpoints /api/v1/patrol/spec-links and /api/v1/patrol/spec-lifecycle)."
      }
    ]
  }
]
