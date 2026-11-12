# Review — task-183 (Architectural Timeline — time scrubber for historical architecture views)

Spec: `specs/system/system-explorer.md` §6 "Architectural Timeline".
Candidate: `9a24fb4c31b70bb9d1df6f2fe7d5b2f6249c3da8` (base `3afece9d73e0d76baf53c9983dede637f15449e8`).
Product commits: `6b9fb4e2` (implementation), `cf8069ea` (round-1 repairs), `8405c8e47` (round-2 fmt repair).
Verdict: **approved** — all three prior durable findings verified repaired with independent evidence; no new defects found.

## Round 3 (2026-11-12) — repair verification + independent re-review

Probes (exit-coded evidence in `/tmp/stage/review-evidence/`, summary in `README.txt`):

- `python3 scripts/check-rustfmt-diff.py 3afece9d…` → changed lines clean, exit 0.
- `bash scripts/check-arch.sh`, `bash scripts/check-mem-port-contracts.sh` → exit 0.
- `cd web && npm ci && npx vitest run timeline-utils.test.js TimelineScrubber.test.js` → 60/60.
- `npx vitest run ExplorerViewNavigableArchitecture / ExplorerViewLivenessWiring / canvas-state` → 37/37.
- `cargo check -p gyre-server --tests` → exit 0 (server test compilation deferred to verifier per budget).
- `cargo test -p gyre-adapters sqlite::graph::tests::list_` → 3/3 (both point-in-time tests against real SQLite).

### Prior findings, verified closed

- **Fetcher race (round 1dd74412)** — fixed. `settleTarget` settles only the waiters for the fetch's own target (not `latestAt`), `releaseSuperseded` resolves superseded/cancelled waiters with null, and `ExplorerView` refuses to pin a response whose position is no longer current. Mutation probe: reintroducing the old latestAt-matching makes the race-regression test fail (1 failed); restored, 60/60 pass.
- **Reconciliation narrative (round 1dd74412)** — fixed at the source. Wave tasks now carry `meta-spec:{name}@v{version}` provenance labels plus `recon-wave:{millis}` wave-identity labels (threaded through `PendingRolloutTask` so deferred rolling batches group with their wave), and the completion event derives `specs: [{name, version}]` scoped to the current wave. The frontend narrative reads only production fields (`task_count`, `duration_secs`, `specs`) with a truthful no-specs fallback for pre-label waves; the frontend test pins the exact production payload shape (both enriched and legacy), ending the prior mirrored-logic inflation.
- **rustfmt drift (round ee39d9c9)** — fixed for real in the adapters crate this time: changed-lines fmt gate vs the task base exits 0; `8405c8e47` verified purely formatting.

### Independent verification of the feature (acceptance criteria)

- **Point-in-time graph API is real.** `GraphPort::list_nodes_at`/`list_edges_at` with a documented existed-at contract (`first_seen_at <= at` AND not soft-deleted at or before `at`), implemented in BOTH adapters (Diesel SQL filters; identical in-code mem guard, contract pinned by a mem test). `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs the graph including nodes soft-deleted since; invalid `at` yields 400 rather than being silently ignored; documented in `docs/api-reference.md`. Adapter tests pass against real SQLite (soft-deleted-after-`at` reconstructed, deleted-before excluded).
- **Scrubber/canvas wiring is coherent.** Scrub position resolves to epoch seconds, drives a 200 ms-debounced `?at=` fetch through the race-fixed fetcher; the server graph replaces the client-side `first_seen_at` fallback only when it is for the exact current position; the "now" position (last index) shows the current graph with no fetch and no ghosts. Ghost overlays and delta stats (added/removed/modified, per type) flow from the historical graph through `computeTimelineGhostOverlays`/`computeTimelineDeltaStats` into the canvas.
- **Key moments cover all four spec categories** (spec approvals from `spec_ref` and node `spec_approved_at`, milestones from `milestone_completed_at`, structural changes from delta magnitude, reconciliations from workspace `reconciliation_completed` messages via the membership-checked `GET /workspaces/{id}/messages?kind=` endpoint). Clicking a marker surfaces the delta narrative in a panel above the canvas; the reconciliation narrative is grounded in the enriched production payload (see above).
- **Timestamp units are consistent end-to-end** (delta timestamps and `first_seen_at` in seconds; message `created_at` in milliseconds, converted once).
- **Mechanical gates:** architecture lint, mem-port-contracts, changed-lines rustfmt all pass. Commit attribution holds on the committed candidate tree (all three product SHAs in the frontmatter); a working-tree `check-task-commit-attribution.sh` failure was traced to the executor's staging clobber of `specs/tasks/task-183.md` (file mtime equals the job-staging timestamp, predating this review; the committed candidate state is correct) — an environment artifact, not a code defect.
- **Server-internal emit path** does not run the caller-side payload validator, and unknown extension fields pass through by design (the pre-existing emitter already sent `task_count`/`duration_secs`); the added `specs` field introduces no new contract violation. The spec-lifecycle patrol validates only `Escalation` payloads — no conflict.

### Non-blocking observations

- Key-moment dedup (merge within 1 index) can fold a reconciliation marker into an adjacent commit-derived marker; degenerate and pre-existing design, not a spec violation.
- `web/dist/` was regenerated as a side effect of `cargo check -p gyre-server` (build.rs auto-build); generated output is the verifier's domain and is not reviewed or blocking.

### Verifier must run

`cargo test -p gyre-server --lib reconciliation` and `cargo test -p gyre-server --lib api::graph` (payload/wave-scoping and `?at=` endpoint tests are type-checked here but their execution was deferred to the verifier per the build-budget rule), plus the full workspace suite, all-target clippy, the mechanical script gates, and the full web vitest suite.
