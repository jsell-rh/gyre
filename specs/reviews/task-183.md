# Review — task-183: Architectural Timeline (system-explorer.md §6)

Candidate: `dae0b9cad187ecb4aca725ebf289b2550a7886a6`
Base: `44a01390cc1afae37d39b2d2bfeb4882ea451c29`
Round: rebase repair (merge of main/task-171 into the retained task-183 branch), prior
verified implementation retained below `cb8a21984`.

## Verdict

**Approved.** No blocking findings. Independent probes support the task contract.

## What the candidate delivers (net diff vs base)

- **Point-in-time graph API:** `GraphPort::list_nodes_at`/`list_edges_at` with an
  explicit existed-at contract (`first_seen_at <= at` AND not soft-deleted at or
  before `at`), enforced in BOTH adapters — SQL filters in SQLite, the same guard
  in code in `MemGraphStore` (satisfies the mem-port-contract invariant; a
  server-side test pins the mem contract). `GET /repos/{id}/graph?at=<epoch-seconds
  |RFC3339>` reconstructs the graph at that instant, including nodes soft-deleted
  since; invalid `at` → 400. Documented in `docs/api-reference.md`.
- **Scrubber integration:** ExplorerView fetches the server `?at=` graph through
  `createTimelineHistoryFetcher` (200 ms debounce; superseded positions resolve
  null; late out-of-order responses refused by position pinning
  `timelineHistoricalGraph.at === timelineScrubTimestamp`), with the client-side
  first_seen_at filter as instant fallback. "Now" (last index) shows the current
  graph with no ghosts.
- **Reconciliation key moments:** the server materializes
  `meta-spec:{name}@v{version}` provenance and `recon-wave:{id}` identity labels on
  wave tasks at creation; `maybe_emit_reconciliation_completed` scopes its payload's
  `specs: [{name, version}]` to the current wave (older waves' terminal tasks
  cannot bleed in). The frontend renders reconciliation markers from
  `GET /workspaces/:id/messages?kind=reconciliation_completed` and derives the
  narrative ONLY from fields the production emitter sends
  (task_count/duration_secs/specs) — no fabricated persona narrative.
- **Key moment narrative panel** in ExplorerView (click marker → grounded narrative,
  dismissible) and a "Why" field in the scrubber popover.

## Probes run (logs in /tmp/stage/review-evidence/)

- `npm ci` (locked versions) then `npx vitest run src/__tests__/timeline-utils.test.js
  src/__tests__/TimelineScrubber.test.js` — 60/60 passed.
- `npx vitest run src/__tests__/ExplorerViewNavigableArchitecture.test.js` — 12/12
  passed.
- **Mutation probe:** reverted the fetcher race fix (settle waiters by `latestAt`
  instead of fetch target) — the race-regression test FAILS against the mutated
  source (1 failed), proving the test kills the real bug. Source restored;
  working tree verified byte-identical to the pre-review guard state
  (dev-review-guard formula incl. mode bits; 1654/1654 files match).
- `cargo check -p gyre-ports -p gyre-adapters -p gyre-server` — clean.
- `cargo test -p gyre-adapters sqlite::graph::tests::list_` — 3/3 passed, including
  both point-in-time reconstruction tests (soft-deleted-after-`at` resurrection,
  pre-`first_seen_at` exclusion).
- Rebase claim verified: all task-183 implementation files are byte-identical to the
  retained verified candidate `cb8a21984`; only the three both-sides-touched files
  (`api/graph.rs`, `web/src/lib/api.js`, `docs/api-reference.md`) differ, and their
  deltas are purely main-side (task-171) changes coexisting with the `?at=` code.

## Verifier must run (blocked here by the 4 GiB server-test wrapper — not a defect)

- `cargo test -p gyre-server --lib api::graph::tests::repo_graph_at_returns_point_in_time_graph`
- `cargo test -p gyre-server --lib api::graph::tests::test_mem_list_nodes_at_and_edges_at_contract`
- `cargo test -p gyre-server --lib reconciliation::` (label round-trips, wave
  scoping v4/v5 isolation, provenance label assertions)
- Full workspace suite, `scripts/check-arch.sh`, all-target Clippy, full web vitest,
  GitHub CI.

## Non-blocking observations

1. `ArchitecturalDelta` has no `narrative` field in production, so the popover/panel
   "Why" narrative renders only for reconciliation key moments; delta-backed moments
   (spec approvals, milestones, structural changes) show what-changed counts and an
   explicit "No narrative recorded for this moment." rather than a fabricated story.
   This is the correct trade under the no-fakery rules. The test
   "popover shows the grounded narrative for delta-backed key moments" injects a
   fixture-only `narrative` property (renders the optional field; production data
   never exercises that path for delta moments).
2. Generated-output observation (verifier-owned): the committed `web/dist/` matches
   main's build at the merge point; an `npm ci` in the review sandbox regenerated
   differing dist assets, which were restored to HEAD. The verifier rebuilds and
   binds dist after integration — not a source defect.
3. `MessageKind::ReconciliationCompleted`'s declared payload schema (persona_id
   required) does not match the emitter's shape — pre-existing at base (the base
   emitter already omitted persona_id); the new `specs` field is an unknown-extension
   field, which `validate_payload` passes through, and the emitter path never
   validates. Cosmetic schema drift, not introduced behavior change.
