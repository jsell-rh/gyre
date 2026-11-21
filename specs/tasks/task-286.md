---
title: "Spec Links — enforce SHA pinning: no link enters or stays in the graph without a target_sha"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-sha-pinning / 5a915155d9bc"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

spec-links.md §Link Types: "Every link is SHA-pinned ... When the linked spec changes, the link becomes stale and the forge flags it." Today SpecLink.target_sha is Option<String> with no validation anywhere in the push/sync path, an unpinned link is stored with status "active", and both staleness paths (the push-time outbound check in sync_spec_ledger and the daily job in spec_link_staleness.rs, which does `let Some(pinned_sha) = &link.target_sha else { continue; }`) skip unpinned links, so such a link can never be flagged no matter how the target changes. Deliver: (1) Push-time manifest validation mirroring the existing check_spec_link_cycles wiring in git_http.rs: a default-branch push whose specs/manifest.yaml declares a link without target_sha is rejected — ref update refused, PushRejected event plus audit, error naming the source spec path and the link target. (2) sync_spec_ledger must never store a declared link as "active" without a pin on any entry point: the non-rejecting entry points (api/repos.rs repo import/re-sync, mirror_sync.rs, where content is already present) skip the link and log a warning naming it instead of storing an active unpinned row. (3) One-time backfill so every stored link is stale-aware: existing durable rows with empty target_sha are pinned to the target spec's current ledger SHA (e.g. in the staleness pass), after which a target SHA change marks them stale through the normal paths. (4) Amend the forge's own specs/manifest.yaml, whose declared links (e.g. depends_on system/source-control.md, references system/spec-lifecycle.md) currently carry no target_sha: add real blob-SHA pins (data amendment, not a contract change) so the repo's own next default-branch push passes its own gate.

## Acceptance Criteria

- [ ] Pushing a manifest whose link omits target_sha to the default branch is rejected with an error naming the source path and link target; the ref is not updated and a PushRejected event is recorded.
- [ ] After the backfill, every row in the durable spec_links table has a non-empty target_sha, and advancing a target spec's SHA subsequently marks a formerly-unpinned link stale via both the push-time inbound check and the daily staleness job.
- [ ] All links in specs/manifest.yaml declare target_sha values equal to the target specs' actual blob SHAs at that commit, and a push of the forge's own manifest is accepted.

## Verification intent

Push a manifest declaring a depends_on link without target_sha: before the fix the push lands and the link is stored status "active" and is silently skipped by every staleness check forever; after the fix the receive-pack rejects the push (probe asserts the rejection and that GET /api/v1/specs/{path}/links contains no active unpinned link). Second probe: take a legacy stored unpinned link, advance the target spec's SHA, run the staleness job — the link must transition to "stale"; on current code it stays "active" because of the `else { continue; }` skip.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "675c0dc7be4a127cb63039f3",
    "decision": "gap",
    "requirement_quote": "Every link is SHA-pinned. Not \"this spec relates to identity-security.md\" but \"this spec relates to identity-security.md@abc123.\" When the linked spec changes, the link becomes stale and the forge flags it.",
    "detail": "The six typed link variants and most per-type enforcement are real (implements/conflicts_with checked at approval, supersedes deprecation on approval, extends drift handling, references deliberately unenforced). But the section's foundational contract -- every link SHA-pinned so a changed target forces the link stale -- is not enforced: the manifest link schema parses target_sha as optional with no validation anywhere in the push/sync path, an unpinned link is stored with status \"active\", and both staleness paths (push-time outbound check and the daily staleness job) skip links with no pinned target_sha, so such a link can never be flagged no matter how the target changes. The table's depends_on row (\"Implementation tasks blocked until dependency spec's implementation is complete\") is also not implemented as stated -- the forge blocks approval of the source spec instead of blocking implementation tasks and models completeness as approval status; that deviation is fully accounted under Approval Gates (dcd440115bac4622a351c857) to avoid double counting.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "    /// SHA the link was pinned to. Stale if target spec SHA advances.\n    pub target_sha: Option<String>,",
        "detail": "Manifest link schema: target_sha is Option<String>. No code path validates that a declared link carries a SHA, so 'every link is SHA-pinned' is unenforced and unpinned links are accepted into the graph."
      },
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "quote": "        // Only check links that have a pinned target_sha and are currently active.\n        let Some(pinned_sha) = &link.target_sha else {\n            continue;\n        };",
        "detail": "Daily staleness job: links without a pinned target_sha are skipped with `continue`, so they can never be marked stale -- the 'when the linked spec changes, the link becomes stale and the forge flags it' guarantee does not hold for them."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "            // Staleness detection: if the link has a pinned target_sha but the target\n            // spec's current SHA in the ledger differs, mark the link as stale.\n            if let Some(pinned_sha) = &link.target_sha {",
        "detail": "Push-time outbound staleness check likewise only runs `if let Some(pinned_sha) = &link.target_sha`; unpinned links stay \"active\" forever."
      }
    ]
  }
]
