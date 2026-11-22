# Review — task-293 (Spec Links: tenant-wide cycle detection)

Spec: `specs/system/spec-links.md` §Cycle Detection ("The forge rejects manifest
changes that would create cycles in the spec graph").
Candidate under review: `cf09e50a` (branch
`pipeline/task-293/9f245c50b584412488dd178bf305884a-1`, base `bbcad833`;
product commits `092db230`, `0680e21a`, `1fb8cfa2`, `34b3711d`, merge
`56addea8`, contract restore `cf09e50a`).
Verdict: **needs-revision** — one finding (F1: deferred cross-repo cycle
through an unresolved stored link re-activated by the ungated staleness job).
All three acceptance criteria, both round-2 repairs, and the test suite are
verified clean by inspection and type check; server test execution is deferred
to the verifier (see the end of this record).

## Round 4 (this review)

Probes run (checkout at candidate `cf09e50a`):

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0, 10m03s, zero
  warnings (`/tmp/stage/review-evidence/cargo-check-gyre-server-tests.txt`).
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → exit 0
  (`/tmp/stage/review-evidence/cargo-fmt-check.txt`).
- Full diff read of `bbcad833..cf09e50a` (spec_registry.rs, git_http.rs,
  coverage row, task doc); grep sweeps for every `check_spec_link_cycles` /
  `sync_spec_ledger` / `resolve_manifest_link_target` caller and for the
  re-resolution path (`target_repo_id = Some`).
- gyre-server test EXECUTION deferred to the independent verifier per the
  stage 4 GiB build-wrapper budget; required commands in
  `/tmp/stage/review-evidence/verifier-commands.txt`.

### Verified working (no findings)

- **AC1 (cross-repo two-push cycle).** `build_tenant_cycle_graph` combines the
  durable `spec_links` table (via `list_all`) with the pushed manifest's edges,
  node identity `(repo, path)`: manifest entries attributed to the source repo,
  cross-repo/cross-workspace targets resolved through `resolve_manifest_link_target`
  — a faithful extraction of the exact inline resolution `sync_spec_ledger`
  previously performed (the removed base code is behaviorally identical), so
  checked-graph attribution matches stored-graph attribution. Unit test
  `tenant_cycle_cross_repo_second_push_rejected` does the real two-push
  sequence with real git repos and the real post-push `sync_spec_ledger`
  storing push 1's edge, and asserts the rejection names
  `repo-a:system/a.md` / `repo-b:system/b.md`. E2e
  `cross_repo_spec_link_cycle_second_push_rejected_e2e` drives the real
  git-receive-pack endpoint across two repos and asserts FORBIDDEN + cycle
  named + ref rolled back to the pre-push SHA + `git_push_rejected` audit
  (gate "spec-link-cycles") + `push_rejected` workspace message.
- **AC2 (single-manifest cycles, no regression).** `detect_link_cycles` (the
  `None`-context path) now builds the same graph structure with every manifest
  edge marked pushed, preserving the pre-task-293 behavior;
  `tenant_cycle_single_manifest_cycle_rejected_with_tenant_context` covers the
  tenant-context path too; the 12 pre-existing cycle tests are unchanged.
- **AC3 (excluded types).** `participates_in_cycle_detection` excludes
  ConflictsWith/References/Supersedes on BOTH halves of the graph;
  `tenant_cycle_references_back_edge_accepted` and
  `tenant_cycle_conflicts_with_bidirectional_not_a_cycle` cover the cross-repo
  forms.
- **Round-2 repair 1 (fail closed).** `build_tenant_cycle_graph` propagates
  `list_all` errors as a rejection ("spec link cycle check failed: unable to
  read the stored spec link graph") instead of the round-1 `if let Ok` silent
  degradation; `tenant_cycle_list_all_error_fails_closed` uses a real failing
  repository impl.
- **Round-2 repair 2 (push-scoped rejection).** `detect_graph_cycles` BFSes
  from each pushed edge's target back to its source; only cycles containing a
  pushed edge are reported. `tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push`
  proves repo C's acyclic push is accepted while a stored A↔B cycle exists AND
  repo A re-declaring the participating edge is still rejected — the gate was
  scoped, not disabled.
- **Replace semantics.** The stored-half exclusion of the pushed repo's rows
  for manifest-declared source paths exactly mirrors the sync's
  `replace_for_source` scoping; `tenant_cycle_push_removing_edge_accepted`
  covers both directions (edge-removing push accepted, re-declaration
  rejected).
- **Wiring.** `git_receive_pack` builds the `TenantCycleContext` from real
  state (durable link repo, workspaces/repos/tenant/memberships, authenticated
  pusher); the rejection path is unchanged (undo ref updates, PushRejected,
  audit). Coverage row 11 correctly updated to implemented/task-293 without
  claiming verification.
- **Test quality.** No inflation found: real git repos, real sync, real
  resolution, real failing adapter; every test would fail if its gate were
  removed.

### Findings

- [-] **F1 (missing enforcement — deferred cross-repo cycle via unresolved
  stored link + ungated staleness re-resolution).** `build_tenant_cycle_graph`
  drops unresolved cross-repo rows from the combined graph (stored half
  `None => continue`, spec_registry.rs:1824; pushed half :1859), but the daily
  `cross_workspace_link_staleness_check` job (jobs.rs:481-495)
  deterministically converts exactly those rows into real attributed active
  edges with no cycle gate (`resolve_unresolved_cross_workspace_links`,
  spec_link_staleness.rs:158-315; activation save at :295-310 requires only
  that the target repo's current manifest declares the target path).
  Reproduction, entirely through the gated receive-pack path (two repos A/B in
  one workspace, B declaring `system/b.md`):
  1. Push to repo B: `system/b.md depends_on @A/system/a.md` while repo A does
     not declare `system/a.md` → push-time resolution returns
     `(None, "unresolved")` (spec_registry.rs:1380-1400) → the pushed edge is
     skipped from `SpecCycleGraph.pushed` → ACCEPTED; the sync stores the row
     `(repo-b, system/b.md)` with `target_repo_id NULL`,
     `target_display "@A/system/a.md"`, status "unresolved".
  2. Push to repo A: `system/a.md depends_on @B/system/b.md` (declaring
     `system/a.md`) → pushed edge A→B resolves, but repo B's pending stored
     row hits the :1824 skip, so the B→A edge is absent from the checked graph
     → no cycle → ACCEPTED; the sync stores A→B active.
  3. The staleness job activates B's pending row (`target_repo_id
     Some(repo-a)`, status "active") — repo A is found by name in the
     workspace and push 2's manifest declares the target path.

  Result: the A→B→A cycle lives permanently in the durable forge graph and
  shows in `GET /api/v1/specs/graph`, with no push rejected — the exact
  failure mode of the task's discovery evidence ("a cycle whose edges are
  declared in two different manifests or repos ... is accepted and lives
  permanently in the forge-maintained graph") and a narrowing of the Required
  behavior's "all stored links in the durable spec_links table". Linking to a
  not-yet-declared target is the documented purpose of unresolved links
  (existing test `cross_repo_link_unresolved_when_target_path_not_in_target_manifest`),
  so step 1 is a mainstream sequence, not an exotic state. The gate holds all
  the information needed at step 2 (`ctx.repos`, the row's `target_display`,
  and the pushed manifest is the target repo's new manifest, making the
  activation deterministic given the push's own content).

  **Repair (either closes it):**
  (a) In `build_tenant_cycle_graph`, attribute unresolved cross-repo stored
  rows via the same name-in-source-link's-workspace derivation the staleness
  pass uses, counting the pending edge only when the target repo's current
  (i.e. pushed) manifest declares the target path — the existing
  `tenant_cycle_unresolved_stored_edge_not_attributed` false-cycle test still
  holds under that conditioning (its fixture's target path is not declared, so
  the pending edge stays excluded); or
  (b) Gate the activation: `resolve_unresolved_cross_workspace_links` refuses
  to flip a link active when doing so would create a cycle in the durable
  graph (leaves it unresolved with a warning naming the cycle).

  Add a regression test per
  `/tmp/stage/review-evidence/deferred-cycle-trace.md`
  (`tenant_cycle_deferred_unresolved_back_edge_rejected` — the step-2 check
  must be `Err`; on this candidate it returns `Ok`).

Full source trace with line numbers: `/tmp/stage/review-evidence/deferred-cycle-trace.md`.

### Minor (non-blocking, recorded for completeness)

- The post-receive ledger sync runs inside `tokio::spawn` (git_http.rs:1407)
  and the receive-pack response does not await it, so a second push arriving
  before the first push's row lands in the durable table bypasses the gate
  (the gate reads only the stored table). The e2e test works around this with
  a 30 s `wait_for_stored_link` poll, showing awareness. Not counted as a
  finding: the async post-receive design and the durable-table data source are
  both pre-existing / mandated by the task contract ("all stored links in the
  durable spec_links table"), and the window is bounded by the sync's own
  completion; noted so the verifier/controller can weigh it.

## Verifier must run

(deferred from review: gyre-server test compilation exceeds this sandbox's
4 GiB build-wrapper budget — cargo check only was run here)

- `cargo test -p gyre-server --lib spec_registry::tests::tenant_cycle`
- `cargo test -p gyre-server --lib cycle_detection` (pre-existing AC2 suite)
- `cargo test -p gyre-server --lib cross_repo_spec_link_cycle_second_push_rejected_e2e`
- `cargo test -p gyre-server --lib tenant_cycle_list_all_error_fails_closed`
- `cargo test -p gyre-server --lib tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push`
- `cargo test -p gyre-server --lib cross_repo_link_unresolved_when_target_path_not_in_target_manifest`
- `cargo test -p gyre-server --lib spec_link_staleness`
- After the F1 repair: the new `tenant_cycle_deferred_unresolved_back_edge_rejected`
  (fails on this candidate) plus the full workspace suite and Clippy per
  standard verification.
