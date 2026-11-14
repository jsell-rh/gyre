# Review — task-135: Implement gate chain configuration and gate agent lifecycle

Candidate: `6413f6a90e59058383a4512ac6e0df07aa3355bb` (base `b6892403d097413f31aa13f0c8685e360e9c608b`)
Review round: verification repair (prior candidate b4093d74 failed the full
suite on exactly one test, per the retained verification log for attempt
c7b64c69).

## Scope of this round

Commit 54ac2796 is the only substantive change: a test-fixture repair in
`crates/gyre-server/src/gate_executor.rs`'s `mod tests` (no production code).
Follow-up commits c648b4eb / 6413f6a9 touch only `specs/tasks/task-135.md`.
The production behavior for both coverage sections (§Gate Chains, §Gate Agent
Lifecycle) was reviewed and approved in prior rounds and is unchanged here;
relative to the assigned base, the candidate diff contains only task-135 files.

## Root-cause validation

The failing test asserted that context capture is skipped when the MR's
workspace has no resolvable tenant, but the old `seed_mr_with_repo` fixture
left the MR on `MergeRequest::new`'s bootstrap `default` workspace (with a
`default` row ensured) while the repo lives in `ws-test` — so the premise could
never hold, and the capture was recorded under a tenant the repo does not
belong to. The repair sets the MR's workspace to the repo's workspace
(`ws-test`, row ensured) — exactly the production create-path shape
(`api/merge_requests.rs` L342: `mr.workspace_id = repo.workspace_id.clone()`),
matching the pre-existing `seed_full_review_fixture`.

## Independent verification performed

- `cargo check -p gyre-server --lib --tests` → exit 0 (repaired test module
  compiles; mem `WorkspaceRepository::delete` and public `mr.workspace_id`
  assignment confirmed).
- Line-by-line trace of the rewritten orphan test against production code:
  persona resolution skips the missing workspace's tenant scope without
  fabricating (gate_executor.rs L1186-1220; mem `find_by_slug_and_scope`
  matches scope values, no row join), then the token-scoping workspace lookup
  (L1352-1360) fails with `workspace ws-test not found` BEFORE `mint_scoped`
  (L1362) and BEFORE `capture_gate_agent_context` (L1449). All three test
  assertions (Failed status + error text, no `gate-*` token in `agent_tokens`,
  no `gate_agent_context` audit event) bind to real fail-closed behavior.
- Mutation analysis: if production regressed to fabricating a tenant on lookup
  failure, every assertion in the test would fail (output would read "exited
  without submitting", a token would be registered, an audit event would
  exist). The prior verification failure is direct evidence the assertions
  fire on real state.
- Blast radius: all 25 fixture call sites audited; no sibling test depends on
  the removed `default`-row bootstrap (validation/token-mint families resolve
  the now-ensured ws-test row under the same tenant as before).
- Mechanical checks: `check-fabricated-scope-defaults`, `check-scope-literal-defaults`,
  `check-arch` all exit 0.
- rustfmt observation (non-blocking): the sandbox rustfmt 1.10.0 reports 11
  formatting hunks in `gate_executor.rs`, but the identical 11 hunks
  (line-shifted) exist on the assigned base's copy of the file — a
  pre-existing tool-version artifact; the candidate introduces zero new
  deviations.

## Deferred to verifier

gyre-server test compilation is excluded from the review sandbox by the build
wrapper (4 GiB budget). The verifier must run `cargo test --all` — in
particular `cargo test -p gyre-server --lib gate_executor` (the repaired
orphan test plus the sibling fixture family).

## Verdict

Approved. The repair correctly fixes the fixture (not production code) to
match the production MR-create shape, and the rewritten test now asserts the
spec's fail-closed token-scoping contract with assertions that demonstrably
fire. Evidence: `/tmp/stage/review-evidence/task-135-candidate-6413f6a9.md`.
