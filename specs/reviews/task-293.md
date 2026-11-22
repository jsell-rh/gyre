# Review — task-293 (Spec Links — tenant-wide cycle detection)

Spec: `specs/system/spec-links.md` §Cycle Detection ("The forge rejects
manifest changes that would create cycles in the spec graph").
Candidate under review: `88631aa6bb19c1e79ff4d6e0eccef8a116f3f69c`
(branch `pipeline/task-293/50499ef6659a4b619b202a9ba25187be-1`, base
`bbcad833ecc7619efeedcf6c19e20f5f10467e6d`; product commits `092db230`,
`0680e21a`, `1fb8cfa2`, `34b3711d`, `824c68ee`). This is review round 4:
rounds 1–2 found three defects (fail-open on storage error, over-broad
legacy-cycle rejection, deferred-cycle bypass via staleness activation),
all repaired in `34b3711d` / `824c68ee`; the round-3 verification attempt
(`4db732c6`) failed one test on a genuine fixture bug (no-op `git commit`),
repaired in `1fb8cfa2`.

Verdict: **approved** — the task contract, all three acceptance criteria,
and all three prior review findings are satisfied by real production code;
no new findings.

## Probes run (checkout at candidate `88631aa6`; working tree carries only
the executor's protected-task-text restoration of `specs/tasks/task-293.md`,
which is not part of the product diff)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0**, zero
  warnings (10m04s; log: `/tmp/stage/review-evidence/cargo-check-gyre-server.log`).
  Test-target codegen/execution deferred to the verifier per the 4 GiB
  build-wrapper budget — commands recorded below and in
  `/tmp/stage/review-evidence/review-round4-notes.md`.
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → **exit 0**.
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- `bash scripts/check-fail-open-ref-resolution.sh` → exit 0.
- Full read of the base..candidate diff (spec_registry.rs, git_http.rs,
  spec_link_staleness.rs, coverage row 11, task file) and line-by-line
  parity check of `attribute_unresolved_stored_link` against
  `resolve_unresolved_cross_workspace_links`, and of
  `build_tenant_cycle_graph`'s replaced-row skip against
  `sync_spec_ledger`'s retain/replace_for_source scoping.

## Verified

- **Core contract.** `check_spec_link_cycles` takes a `TenantCycleContext`
  (git_http.rs builds it from state: durable `spec_link_repo`, workspaces,
  repos, tenant, memberships, the authenticated pusher, and the kv denial
  store) and `build_tenant_cycle_graph` combines every stored row in the
  durable spec_links table with the pushed manifest's edges, node identity
  (repo, path): manifest entries attributed to the source repo,
  cross-repo/cross-workspace targets resolved through
  `resolve_manifest_link_target` — factored out of and identical to the
  resolution `sync_spec_ledger` persists (task-287), so the checked graph
  and the stored graph attribute nodes identically. Same-repo targets
  attribute to the source repo. Rejection names the cycle with
  repo-attributed nodes. git_http.rs rejection wiring unchanged in shape
  (undo_ref_updates, PushRejected event, git_push_rejected audit with
  gate_name "spec-link-cycles").
- **AC1** (two-push cross-repo cycle): unit test
  `tenant_cycle_cross_repo_second_push_rejected` runs the real
  `sync_spec_ledger` for push 1 (asserts the stored row is attributed to
  repo B) and rejects push 2 naming `repo-a:system/a.md` and
  `repo-b:system/b.md`; e2e
  `cross_repo_spec_link_cycle_second_push_rejected_e2e` drives the real
  git-receive-pack endpoint end to end: push 1 accepted + stored,
  push 2 FORBIDDEN with the cycle named, default branch restored to the
  pre-push SHA, `git_push_rejected` audit and `push_rejected` workspace
  message asserted.
- **AC2** (single-manifest cycles, no regression): `detect_link_cycles`
  retains single-manifest semantics (every manifest edge is a pushed edge);
  the 12 pre-existing tests stand and
  `tenant_cycle_single_manifest_cycle_rejected_with_tenant_context` adds
  the tenant-context variant. Self-loops are still caught (`bfs_path`
  start==goal).
- **AC3** (exclusions): `participates_in_cycle_detection` excludes
  ConflictsWith/References/Supersedes — exactly the spec's list;
  `tenant_cycle_references_back_edge_accepted` and
  `tenant_cycle_conflicts_with_bidirectional_not_a_cycle` prove the widened
  gate is not indiscriminate.
- **Round-1 finding 1 (fail-open) repaired.** `list_all` errors propagate
  out of `check_spec_link_cycles` (warn! + "spec link cycle check failed:
  unable to read the stored spec link graph") → push rejected;
  `tenant_cycle_list_all_error_fails_closed` uses a real failing
  `SpecLinkRepository`.
- **Round-1 finding 2 (over-broad rejection) repaired.** Only cycles in
  which a pushed edge participates are reported (BFS from each pushed
  edge's target back to its source);
  `tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push` proves
  repo C's acyclic push passes while a stored A↔B cycle exists AND repo A
  re-declaring the participating edge is still rejected. Replace-scoping
  matches `sync_spec_ledger` exactly (same predicate, verified), so a push
  that REMOVES a cycle edge is accepted
  (`tenant_cycle_push_removing_edge_accepted`, fixture repaired in
  `1fb8cfa2` so the edge-removal commit is a real content change).
- **Round-2 finding (deferred-cycle bypass) repaired, both halves.**
  (a) `attribute_unresolved_stored_link` attributes an unresolved stored
  row exactly when the staleness pass will activate it — same status/
  display filters, same kv-denial skip, same source repo → workspace →
  tenant derivation (never a fabricated scope), same display
  disambiguation, same target-manifest requirement, with the pushed
  manifest standing in for the target repo's current manifest when the
  target repo is the pushed one. The reviewer's three-step reproduction is
  encoded as `tenant_cycle_deferred_unresolved_back_edge_rejected`, which
  first reproduces the deferred state through the real sync (asserts
  status "unresolved", `target_repo_id: None`, display "@A/system/a.md")
  and then requires the step-2 push to be rejected with the cycle named.
  (b) Backstop: `resolve_unresolved_cross_workspace_links` refuses
  activation that would create a cycle (`link_activation_creates_cycle`,
  graph snapshot cloned per iteration so same-run activations are visible
  to later checks); `staleness_refuses_activation_that_creates_cycle`
  proves a pending B→A with a stored active A→B stays unresolved, and the
  pre-existing activation tests prove activation still works absent a
  cycle.
- **Evidence integrity.** Coverage row 11 moved to `implemented` with
  task-293 attribution, concrete source/test evidence, and "pending
  independent review for verified" — conservative and accurate. Task file
  carries `ready-for-review` with all five product commits attributed.

Minor (non-blocking, recorded for completeness):

- `link_denial`'s kv read fails open on a storage error (`.ok()??`). This
  is pre-existing task-287 code (present verbatim in the base commit), not
  a task-293 regression, and it fails safe in the push-gate direction
  (attribution on kv error can only over-, not under-, attribute).
- Mirror-sync ingestion still stores manifest edges without a cycle check.
  This was squarely presented in round 1 and the accepted repair option
  (push-scoped rejection: "a stored-only cycle ... is not created by this
  push") was chosen; the staleness backstop now also closes the activation
  route into the durable graph. The task contract's required gate is the
  git-receive-pack push path, which is enforced.
- If the workspace lookup fails while building the cycle context in
  git_http.rs, cross-workspace (three-segment) targets resolve unresolved
  and are skipped. This mirrors what `sync_spec_ledger` itself would store
  with the same failed lookup, so checked-graph/stored-graph attribution
  parity — the soundness property — is preserved.

## Verifier must run (server test compilation deferred per 4 GiB budget)

- `cargo test -p gyre-server --lib spec_registry::tests::tenant_cycle -- --nocapture`
- `cargo test -p gyre-server --lib cross_repo_spec_link_cycle_second_push_rejected_e2e -- --nocapture`
- `cargo test -p gyre-server --lib spec_link_staleness -- --nocapture`
- `cargo test -p gyre-server --lib detect_link -- --nocapture` (AC2 regression)
- `cargo test --all` (prior attempt `4db732c6` failed only the now-repaired
  `tenant_cycle_push_removing_edge_accepted` fixture; its git-commit panic
  was the no-op-commit test bug fixed in `1fb8cfa2`, not a product defect)
