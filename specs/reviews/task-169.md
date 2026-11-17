# Review — task-169 (Spec registry — supply chain AIBOM integration)

Spec: `specs/system/spec-registry.md` §Supply Chain (supply-chain.md).
Candidate under review: `1026cb9d` (base `e9ed695`); product commits
`a4c24fc1`, `ad473d3c`, `4bec0af5`, plus merge `8b39a025` resolving the
prior DIRTY merge against task-166's release artifacts (durable finding
`4c1347d6`).
Verdict: **approved** — no findings.

## Round 3 (post-merge-conflict repair)

The prior rounds approved the implementation twice (commits `390dc6ad`,
`2e9b5872`); the only durable finding was the published PR conflicting
with main after task-166 landed its `?format=release` AIBOM artifact in
`aibom.rs`. This round re-reviewed the full diff `e9ed695..1026cb9d`,
including the merge resolution, as a fresh review (no prior approval
carried forward).

### Probes run

- `cargo check -p gyre-server --tests` (via `/tmp/stage/bin/cargo`) →
  **exit 0**, 12m05s cold, 0 errors / 0 rustc warnings. Validates the new
  module, the `aibom.rs`/`release.rs`/`version_compute.rs` changes, and
  every new test module. (Log: `/tmp/stage/review-evidence/`.)
- `cargo test -p gyre-server …` is wrapper-blocked in the review stage
  (gyre-server codegen + model exceeds 4 GiB). Deferred probes recorded
  for the verifier: `cargo test -p gyre-server --lib aibom_spec_registry`,
  `--lib api::aibom`, `--lib api::release`, and `cargo test --all`
  (acceptance criterion).
- Mechanical scripts, all exit 0: `check-arch.sh`, `check-inert-enforcement.sh`,
  `check-scope-literal-defaults.sh`, `check-relative-path-defaults.sh`,
  `check-byte-slice-truncation.sh`, `check-mem-port-contracts.sh`.
  `check-task-commit-attribution.sh` timed out (full-history walk) — verified
  manually instead: the three product commits are in the committed
  `specs/tasks/task-169.md` `commits:` frontmatter at HEAD; `8b39a025` is a
  merge (outside the script's non-merge scope); the other task-169-labeled
  commits are `process:`/`verify:` type. Verifier should re-run with full
  history.
- Git inspection: no conflict markers in touched sources; no changes
  outside `crates/`, `specs/`, `docs/`.

### Verified working

- **Real state, not stand-ins.** `aibom_spec_registry.rs`
  (`build_spec_registry_section`) queries the real ports —
  `SpecLedgerRepository::list_all`, `SpecApprovalEventRepository::list_by_path`,
  `SpecApprovalRepository::find_by_spec_sha`, `MergeRequestRepository::list_by_repo`
  — and resolves the manifest version via `git_refs::resolve_blob_sha`
  (real `git rev-parse --verify HEAD:specs/manifest.yaml`, with
  path-injection guards; absent manifest reported `null`, never fabricated).
  Storage failures propagate as 500 — the section cannot silently
  under-report spec provenance.
- **All specced content is present.** Every ledger status
  (approved/pending/deprecated/revoked/rejected) with per-status counts and
  `total_specs`; the approval chain for approved specs (who, when,
  human/agent, persona) from the approval event history filtered to the
  current version and active events; drift risk (`drifted` count +
  per-spec `drift_status`); `spec_code_bindings` per MR with a
  `spec_ref`; `bindings_summary`. Response shape matches the task's
  example (`total_specs`, `approved`, `pending`, `specs[{path, status,
  approved_by, approved_at}]`) and extends it honestly.
- **Spec-to-code binding mirrors the real merge gate.** `merge_time_approval`
  evaluates the exact per-SHA ledger `verify_spec_ref` (api/gates.rs:819)
  enforces (`find_by_spec_sha` + activity), three-valued. The
  indeterminate case is correct, not evasive: `SpecApproval::revoke()`/
  `reject()` clear `approved_at` (mutual exclusivity, domain lines
  146/179; transitions terminal), so a withdrawal AFTER the merge leaves
  no grant time — reporting `Some(false)` there would misreport the
  systematic case (every push modifying a spec auto-revokes its
  approvals). Withdrawal at/before the merge IS determinably false
  (granted ≤ withdrawn ≤ merged). Reverted MRs report indeterminable
  rather than evaluating against the revert timestamp —
  `merge_processor.rs` overwrites `updated_at` at revert, so the merge
  moment is genuinely gone from the record.
- **`merged_at` is the real merge moment**: the merge processor stamps
  `updated_at = now` at the Merged transition (merge_processor.rs:1482
  group path, :2593 single-MR path).
- **Repo scoping is real**: ledger entries are filtered by the `repo_id`
  the push-time sync stamps (`git_http.rs:1269` passes the pushing repo's
  id; `spec_registry.rs:565` stores it), and bindings come from
  `list_by_repo`. Cross-repo isolation is test-asserted on both stores.
- **All three surfaces share one section**: built once in `get_aibom` and
  embedded in `AibomResponse` (aibom_version 1.1 → 1.2), in
  `AibomReleaseResponse` (`?format=release` — the merge-resolution
  extension so the task-166 release artifact carries manifest state and
  cannot disagree with the summary), and in `ReleasePrepareResponse`
  (`POST /api/v1/release/prepare`).
- **Tests are discriminating, not self-confirming.** Mutation-style
  analysis: if the binding check used current `is_active()` instead of
  merge-time evaluation, `mr-2` (granted after merge, still active —
  expects `Some(false)`) and `mr-8`/`mr-9` (withdrawn after merge —
  expect `None`) fail; if reverted MRs were evaluated against
  `updated_at`, `mr-7` (still-active approval, expects `None`) fails; if
  the manifest version were fabricated, the real-git-repo test comparing
  against `git rev-parse HEAD:specs/manifest.yaml` fails; if the repo
  filter were dropped, the cross-repo leak assertions fail. The API-level
  tests drive the real router (`api_router().oneshot`) with real mem
  stores and assert the JSON section on all three surfaces.
- **No invariant regressions**: no new routes (both touched routes have
  pre-existing ABAC `RouteResourceMapping` entries — aibom: repo/read,
  release/prepare: release/write); hexagonal boundaries intact (module in
  gyre-server, domain types only); mem adapters delegate transitions to
  the domain state machine; frontend consumes `aibom.agents` loosely so
  the additive field breaks nothing; `docs/api-reference.md` updated
  accurately for all three surfaces; coverage matrices
  (`spec-registry.md` row 18, `supply-chain.md` row 14) reflect the
  shipped behavior including the merge-note for the release-artifact
  embedding.

### Deferred to the verifier

- `cargo test --all` (acceptance criterion) and the focused
  `gyre-server` lib test filters listed above. `cargo check --tests`
  passing with zero warnings plus two prior CI-green rounds on the same
  product commits give high confidence, but the suites must run in the
  verifier's 4 GiB sandbox.
- `check-task-commit-attribution.sh` with full history.
