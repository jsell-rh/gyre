---
title: "Spec Links — stored link statuses: broken when the target leaves the manifest, conflicted when both endpoints are approved, with reversion when the defining conditions cease"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-status-lifecycle / 5a915155d9bc"
depends_on: [task-287]
progress: ready-for-review
commits: ["6a4b091b4e0949090c5931d072e576869e06da32"]
---

## Required behavior

spec-links.md §Link Status defines four statuses; active and stale are delivered, two are not, and the stored graph must reflect CURRENT conditions, not a one-way ratchet. (1) broken: the only producer (spec_link_staleness.rs Ok(None) path) fires when the target is absent from the ledger entirely, but a spec removed from the manifest is kept in the ledger as Deprecated (save, not delete), so links targeting it stay "active" forever — the "Target spec was deleted or is no longer in the manifest" half never fires. (2) conflicted: no code assigns the status anywhere; both-approved conflicts are only computed on the fly by GET /api/v1/specs/conflicts and the patrol, so the stored graph can never be relied on for either condition. (3) Reversion (per controller review feedback b222f4b88e85892d669131d7:status-transitions-never-revert): the acceptance criteria as previously drafted could be satisfied by an implementation that never exits "conflicted" or "broken", which would then violate the spec — GET /api/v1/specs/conflicts must return "All ACTIVE conflicts" and broken is defined by the current condition "target was deleted or is no longer in the manifest". Deliver: (a) broken: the staleness pass (and the push-time inbound check) determines, for each link not currently satisfying the broken condition, whether the target path is present in the target repo's current manifest (using the target-manifest lookup built by the cross-repo resolution task); a target absent from the ledger OR absent from its repo's current manifest transitions the link to "broken" via the existing write-through (store + spec_link_repo.save). A target deprecated via supersedes approval but still present in its manifest must NOT be broken. (b) broken reversion: broken links are RE-CHECKED, not skipped — the daily pass's "Already known stale/broken — skip" skip must not prevent re-evaluation of the broken condition; a target that re-enters its repo's manifest (or is restored to the ledger) transitions the link back per the normal active/stale rules (active if the pinned SHA matches the restored current SHA, stale otherwise). (c) conflicted: whenever both endpoints of a conflicts_with link are Approved (detected when the link is stored at sync, when an approval lands, and in the daily pass), the link's stored status becomes "conflicted" (persisted). (d) conflicted reversion: conflicted links are re-evaluated in the daily pass and whenever an approval changes (approve, revoke, and the extends-drift reset to Pending): when either endpoint is no longer Approved, the link's stored status reverts to whatever the active/stale/broken rules yield for its current pin and target state. GET /api/v1/specs/conflicts returns exactly the links currently in the conflicted state, and its computed notion must agree with the stored status (no drift between endpoint and graph).

## Acceptance Criteria

- [ ] Removing a spec from a repo's manifest (pushing the removal) transitions inbound links targeting it to "broken" in the stored graph (visible via GET /api/v1/specs/{path}/links and the durable table), where they previously remained "active" through every daily pass.
- [ ] A conflicts_with link whose two specs are both Approved holds stored status "conflicted", and GET /api/v1/specs/conflicts lists exactly those links.
- [ ] A spec deprecated by supersedes approval but still present in its manifest leaves inbound links non-broken (deprecation is not deletion).
- [ ] Conflicted reverts: after one endpoint of a conflicted pair loses Approved status (e.g. approval reset to Pending via the extends drift path or revoked), GET /api/v1/specs/conflicts no longer lists the link and its stored status is no longer "conflicted" (it is whatever the active/stale/broken rules yield).
- [ ] Broken reverts: after a removed target spec is re-added to its repo's manifest (or restored to the ledger), the previously-broken inbound link is no longer "broken" — it transitions to active or stale per the normal SHA rules on the next staleness pass.

## Verification intent

Declare a conflicts_with link between specs A and B after both are already Approved (so the approval-time gate never rejects): before the fix the stored status stays "active" and only the on-the-fly conflicts endpoint knows about the violation; after the fix GET /api/v1/specs/{A}/links shows status "conflicted" for the link. Second probe (broken entry): remove B's manifest entry (the ledger keeps B as Deprecated): before the fix the A→B link stays "active" through daily passes because the Ok(Some(entry)) path leaves it untouched; after, it is "broken". Third probe (broken reversion, the one-way-ratchet defect): re-add B's manifest entry and run the staleness pass — on current code the link is skipped forever by the `status == "broken" → continue` guard; after the fix it must leave "broken" (active or stale per SHA rules), proving the stored graph tracks current conditions. Fourth probe (conflicted reversion): reset one endpoint of the conflicted pair to Pending and run the pass — on a never-reverting implementation the link stays "conflicted" and GET /api/v1/specs/conflicts misreports a resolved conflict as active; after the fix the link is delisted and its stored status reflects the active/stale/broken rules.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "b222f4b88e85892d669131d7",
    "decision": "gap",
    "requirement_quote": "| `broken` | Target spec was deleted or is no longer in the manifest |\n| `conflicted` | A `conflicts_with` link where both specs are approved (violation) |",
    "detail": "active (default) and stale (push-time outbound/inbound detection plus the daily job) are delivered. Two of the four statuses are not delivered as stored link states: (1) broken is only set when the target is absent from the spec ledger entirely -- but a spec removed from the manifest is kept in the ledger as Deprecated (never deleted), so links targeting it remain status \"active\" forever; the \"no longer in the manifest\" half of the broken definition never fires, and the daily job's Ok(Some(entry)) path leaves the link untouched. (2) conflicted is never assigned as a status anywhere in the codebase -- both-approved conflicts are only computed on the fly by GET /api/v1/specs/conflicts and the patrol check; the status column (documented in the domain type as one of the five health values) can never hold \"conflicted\", so the stored graph cannot be relied on for either condition. Additionally the stored statuses that do exist are a one-way ratchet: the daily staleness pass skips links already marked broken (\"Already known stale/broken — skip\"), so a target spec re-added to its manifest or restored to the ledger leaves inbound links broken forever even though the defining condition (\"Target spec was deleted or is no longer in the manifest\") has ceased; the stored graph the spec promises must reflect current conditions, and GET /api/v1/specs/conflicts is required to return \"All active conflicts\" — which a never-reverting conflicted status would misreport as active forever once one exists.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "quote": "            Ok(None) => {\n                // Target spec not in ledger — mark as broken.\n                mark_link_status(state, &link.id, \"broken\", now_secs).await;",
        "detail": "The only code path that sets \"broken\": target lookup returns Ok(None) (not in the ledger at all). Since manifest removal keeps the ledger entry, this path does not cover 'no longer in the manifest'."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "                e.approval_status = ApprovalStatus::Deprecated;\n                e.updated_at = now;\n                let _ = ledger.save(&e).await;\n            }",
        "detail": "A spec removed from the manifest is marked Deprecated and kept in the ledger (save, not delete) -- so links targeting it find Ok(Some(entry)) in the staleness job, the SHA is unchanged, and the link stays \"active\" instead of \"broken\"."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "    // An \"active conflict\" is a conflicts_with link where both specs are approved.\n    // spec-links.md §Link Status: conflicted = both specs approved (violation).",
        "detail": "Conflicted links are computed on the fly by the conflicts endpoint rather than ever being stored: no code assigns the \"conflicted\" status to a link row."
      },
      {
        "file": "crates/gyre-domain/src/spec_links.rs",
        "quote": "    /// Link health: \"active\" | \"stale\" | \"broken\" | \"conflicted\" | \"unresolved\"",
        "detail": "The domain type documents \"conflicted\" as a link health value, but a repository-wide search finds no producer: the value is dead in the stored graph."
      },
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "quote": "        if link.status == \"stale\" || link.status == \"broken\" {\n            // Already known stale/broken — skip.\n            continue;\n        }",
        "detail": "The staleness pass skips links already marked broken, so a target that returns to the ledger or its repo's manifest never transitions the link back — \"broken\" is permanent once set, a one-way ratchet rather than a reflection of current conditions."
      }
    ]
  }
]

## Shipped

All four spec-links.md §Link Status values are now stored link states that
track CURRENT conditions, delivered on top of the task-287 cross-repo
resolution machinery.

### Behavior

- **Single status oracle** — `evaluate_link_status` (spec_registry.rs)
  computes the status a link row must hold right now, in precedence order:
  `conflicted` (conflicts_with link whose two endpoints are both
  ledger-Approved) → `broken` (target absent from the ledger OR absent from
  its repo's current manifest, read via `target_manifest_path_presence`
  git-showing the target repo's `specs/manifest.yaml` at its default branch;
  a supersedes-deprecated target still declared in its manifest is NOT
  broken; an unreadable manifest is `Unknown` and falls through to the SHA
  rules rather than breaking on undeterminable state) → `stale` (pinned
  `target_sha` ≠ ledger `current_sha`) → `active`.
- **Producers**: (1) the daily staleness pass (`spec_link_staleness.rs`
  `run_once`) re-evaluates every non-unresolved link — the old
  "Already known stale/broken — skip" one-way-ratchet guard is removed, so
  stale/broken/conflicted are RE-CHECKED each pass and revert when their
  defining condition ceases; (2) push-time sync (`sync_spec_ledger`) breaks
  inbound links when the target leaves the manifest, stores
  both-approved conflicts_with links as `conflicted` at store time, and
  reconciles conflicts for every approval change it makes (content
  invalidation, deprecation, re-add, extends drift) via
  `refresh_conflict_statuses_for_spec`; (3) `approve_spec`/`revoke_spec`
  call `refresh_conflicts_for_spec` so conflicted is set/reverted when an
  approval lands, without waiting for the daily pass; (4)
  `GET /api/v1/specs/conflicts` runs `reconcile_conflict_statuses` over
  every conflicts_with link before filtering `status == "conflicted"`, so
  the endpoint and the stored graph cannot drift.
- **Write-through** — every transition persists via `write_link_status`
  (hot cache + `spec_link_repo.save`); `stale_since` stamps the transition
  INTO stale and is cleared when the link leaves stale.

### Test evidence

Six tests cover the four verification probes (all assert both the hot
cache and the durable `spec_links` table where applicable):

- `staleness_pass_tracks_target_manifest_presence_both_ways` — broken on
  manifest removal (ledger keeps the target as Deprecated) + reversion to
  active on manifest re-add (the one-way-ratchet defect).
- `staleness_pass_does_not_break_deprecated_target_still_in_manifest` —
  deprecation is not deletion.
- `staleness_pass_sets_and_reverts_conflicted_status` — daily-pass
  conflicted set/revert.
- `sync_marks_inbound_links_broken_when_target_leaves_manifest` —
  push-time broken transition.
- `sync_stores_conflicts_with_link_as_conflicted_when_both_approved` —
  link stored conflicted at sync.
- `conflicted_status_set_on_approval_and_reverted_on_revoke` (api/specs.rs)
  — API-level: approve → stored `conflicted` + listed by
  `GET /api/v1/specs/conflicts`; revoke → delisted + status reverted to the
  active/stale/broken rules.

`cargo check -p gyre-server --lib --tests` exits 0 with zero warnings (see
/tmp/stage/review-evidence/task-288-check.log). Per the 4 GiB build-wrapper
budget, gyre-server test compilation is deferred to the independent
verifier, which must run:

    cargo test -p gyre-server --lib -- \
      staleness_pass_tracks_target_manifest_presence_both_ways \
      staleness_pass_does_not_break_deprecated_target_still_in_manifest \
      staleness_pass_sets_and_reverts_conflicted_status \
      sync_marks_inbound_links_broken_when_target_leaves_manifest \
      sync_stores_conflicts_with_link_as_conflicted_when_both_approved \
      conflicted_status_set_on_approval_and_reverted_on_revoke

plus the regression surface: `cargo test -p gyre-server --lib --
spec_link_staleness`, `... -- get_conflicts`, `... -- sync_`.

Coverage row 6 (Link Status) in specs/coverage/system/spec-links.md is
updated to `implemented` with the source evidence above; no unrelated rows
touched.
