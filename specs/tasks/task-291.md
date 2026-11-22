---
title: "Spec Links — spec-detail link direction: API emits it, UI renders inbound vs outbound with staleness indicators"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-ui-link-direction / 5a915155d9bc"
depends_on: []
progress: complete
commits: ["9fda212d4aa1473b06c1dfe59f6a95f55e53dc3e", "b3ab4778a2f12a37d1214fe197062629b864ca43", "4bce7f4d7398485fe160471e781fdbba8f5bf205"]
---

## Required behavior

spec-links.md §Querying the Graph (UI): "Spec detail page: link section showing all outbound/inbound links with staleness indicators". The DAG visualization and impact-analysis bullets are real; the spec-detail bullet is broken: DetailPanel.svelte computes the inbound/outbound columns (lines 3674-3675) and '<- from'/'-> to' row labels (3729, 3744) from a direction field on each link object, but SpecLinkResponse (the serialized shape of GET /api/v1/specs/{path}/links, api/specs.rs:1648) has no such field, so link.direction is always undefined — every link, inbound included, is classified outbound, renders in the outbound column, and is labeled '-> to'. Fix at the source: SpecLinkResponse gains a direction field — "outbound" when source_path equals the queried path, "inbound" when target_path equals it (a self-link is outbound) — computed by get_spec_links relative to the requested path; the dependents/dependencies endpoints populate it consistently (inbound/outbound respectively). DetailPanel's existing direction-based filtering and labels then display the distinction; staleness continues to surface via the existing status badge.

## Acceptance Criteria

- [ ] For a spec with one inbound and one outbound link, GET /api/v1/specs/{path}/links returns direction "inbound" and "outbound" respectively on the corresponding link objects.
- [ ] The spec-detail links tab renders the inbound link in the inbound column labeled '<- from' and the outbound link in the outbound column labeled '-> to'.
- [ ] Stale links still show their staleness indicator in the links tab (status badge), unchanged.

## Verification intent

Open the spec-detail links tab for a spec that is the target of another spec's depends_on link: on current code the inbound link renders in the outbound column labeled '-> to' because link.direction is undefined (the API never emits it); after the fix it renders in the inbound column labeled '<- from'. API-level probe: assert the response's direction field equals "inbound" for the link whose target_path is the queried spec — undefined on current code, which is exactly the defect.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "5e761a34ffed5b87f0b401c1",
    "decision": "gap",
    "requirement_quote": "- **Spec detail page:** link section showing all outbound/inbound links with staleness indicators",
    "detail": "Two of the three bullets are real: the spec graph visualization (SpecGraphDAG renders the interactive DAG with nodes colored by approval status, stale edges highlighted yellow, conflicts red, and click navigation) and the impact analysis view (SpecDashboard impact mode computes all transitive dependents via BFS over the real graph edges and aggregates them per repo). The spec-detail link section is partially broken: the diagram's inbound/outbound columns and the list's direction labels are computed from a `direction` field on each link object, but the links API response (SpecLinkResponse) has no such field, so `link.direction` is always undefined -- every link, inbound included, is classified outbound, renders in the outbound column, and is labeled '-> to'. The outbound/inbound distinction the bullet requires is therefore not displayed. Staleness is surfaced via the link status badge (status string incl. \"stale\").",
    "evidence": [
      {
        "file": "web/src/lib/DetailPanel.svelte",
        "quote": "            {@const inbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) === 'inbound')}\n            {@const outbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) !== 'inbound')}",
        "detail": "The spec-detail links tab splits inbound/outbound and labels each row by `link.direction` -- a field the API never emits, so inbound links are silently rendered as outbound."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "pub struct SpecLinkResponse {\n    pub id: String,\n    pub source_path: String,",
        "detail": "SpecLinkResponse (the serialized shape of GET /specs/{path}/links) has no direction field: id, source_path, source_repo_id, link_type, target_path, ... only."
      },
      {
        "file": "web/src/components/SpecGraphDAG.svelte",
        "quote": "    const isStale = edge.status === 'stale';",
        "detail": "DAG visualization bullet is satisfied: stale edges are highlighted (yellow stroke) in the interactive graph."
      },
      {
        "file": "web/src/components/SpecDashboard.svelte",
        "quote": "    const queue = [{ path: impactPath, depth: 0 }];",
        "detail": "Impact analysis bullet is satisfied: BFS over the spec graph computes all transitive dependents of the selected spec."
      }
    ]
  }
]

## Shipped

Implemented in b3ab4778a2f12a37d1214fe197062629b864ca43; verification of that
candidate failed only on rustfmt for changed lines (specs.rs:4984), repaired
formatting-only in 9fda212d4aa1473b06c1dfe59f6a95f55e53dc3e (no behavior
change).

### Behavior

- `SpecLinkResponse` (crates/gyre-server/src/api/specs.rs) gained a `direction`
  field (`Option<String>`, skipped when unset): `"outbound"` when the queried
  path equals the link's `source_path`, `"inbound"` when it equals
  `target_path`; a self-link is outbound. Computed via a
  `with_direction_relative_to` helper applied in `get_spec_links` (relative to
  the requested path), `get_spec_dependents` (inbound), and
  `get_spec_dependencies` (outbound). Tenant-wide stale-links/conflicts
  listings omit the field (no queried path applies).
- `web/src/lib/DetailPanel.svelte` links tab: the existing direction-based
  filtering and '<- from'/'-> to' labels now receive real data. Inbound links
  additionally render their counterpart spec (`source_path`) in the diagram's
  inbound column and list rows — previously `target_path` was shown, which for
  an inbound link is the queried spec itself. Staleness still surfaces via the
  existing status badge (`link.approval_status ?? link.status`), unchanged.

### Test evidence

- Rust (gyre-server, api/specs.rs tests): extended
  `get_spec_links_returns_outbound_and_inbound` (spec b with one inbound a→b
  and one outbound b→c link; asserts `direction == "inbound"`/`"outbound"` on
  the corresponding objects — fails on base where the key is absent) and added
  `get_spec_links_self_link_is_outbound` and
  `get_spec_dependents_and_dependencies_populate_direction`. Not executed in
  this sandbox (server test compilation exceeds the assignment's build
  budget); recorded for the independent verifier in
  /tmp/stage/review-evidence/README.md. `cargo check -p gyre-server` and
  `cargo check -p gyre-cli` passed on the candidate (d688a1eb); the
  formatting-only repair does not affect types.
- Web: `npm ci` then `npx vitest run src/__tests__/DetailPanel.test.js` —
  exit 0, 41/41 passed (this repair attempt), including the 3 "Link direction
  in spec Links tab" tests covering the inbound/outbound column split,
  '<- from'/'-> to' row labels, and the stale badge on a stale inbound link.
  Evidence: /tmp/stage/review-evidence/.
- rustfmt: `rustfmt --edition 2021 --check crates/gyre-server/src/api/specs.rs`
  exits 0 after the repair (the prior verification failure).

### Coverage

specs/coverage/system/spec-links.md rows 13 (API) and 15 (UI) updated with
the direction-field evidence in b3ab4778; the pre-existing graph ?repo=
filter gap noted in rows 12/13 is untouched (task-290 scope).
