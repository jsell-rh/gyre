# Review — task-169 (Spec registry — supply chain AIBOM integration)

Spec: `specs/system/spec-registry.md` §Supply Chain (supply-chain.md).
Candidate: `6b0503c288ea7ed0a52234e7fe1b0e8bd25a201f` (product commit `a4c24fc1`, process commit `6b0503c2`), base `6082be2f`.
Verdict: **approved** — real implementation, no findings.

## Probes

Evidence with commands, sources, and outputs: `/tmp/stage/review-evidence/task-169-probes.md`.

- `cargo check -p gyre-server` → exit 0 (7m49s). `cargo check -p gyre-server --tests` → exit 0 — the new unit and router-level tests compile.
- `cargo test -p gyre-server aibom_spec_registry` → **blocked by the sandbox build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB") — infrastructure limit, not a code defect. Tests the verifier MUST run:
  - `cargo test -p gyre-server --lib aibom_spec_registry` (3 tests)
  - `cargo test -p gyre-server --lib api::aibom` (incl. `aibom_includes_spec_registry_section`)
  - `cargo test -p gyre-server --lib api::release` (incl. `release_prepare_includes_spec_registry_metadata`)
  - `cargo test --all` (acceptance criterion)
- `cargo test -p gyre-domain` → 564 passed — covers the `MergeRequest::transition_status` and `SpecApproval` approve/revoke state machines the new tests seed.
- Mechanical guards pass on the candidate: check-arch, check-in-memory-state-stores, check-fail-open-ref-resolution, check-inert-enforcement, check-scope-literal-defaults, check-relative-path-defaults, check-mem-port-contracts, check-unwired-port-methods.
- check-task-commit-attribution fails only against the working tree, where the stage harness staged the assignment's original task body (`commits: []`); against the committed candidate `6b0503c2:specs/tasks/task-169.md` the frontmatter parses to `a4c24fc1` and the gate holds. Not a candidate defect.
- Git semantics probe: `git rev-parse --verify HEAD:specs/manifest.yaml` returns the blob SHA (exit 0) and fails for absent files (exit 128 → `None`), matching `resolve_blob_sha` and test 3's expectation.

## Verified

- **Real storage, no stand-ins.** `build_spec_registry_section` (new `crates/gyre-server/src/aibom_spec_registry.rs`) reads `spec_ledger.list_all()` (repo-scoped by the `repo_id` every production `sync_spec_ledger` call site stamps — git_http.rs, repos.rs ×2, mirror_sync.rs), `spec_approval_history.list_by_path()` (filtered to the current SHA and active events), `spec_approvals.find_by_spec_sha()` (the exact per-SHA ledger `verify_spec_ref` enforces), `merge_requests.list_by_repo()`, and git for the manifest blob SHA. All ports are implemented by SQLite, Postgres, and mem adapters (wiring verified by file/line). Storage failures propagate as `ApiError::Internal` — the section cannot silently under-report; only genuine absence (no manifest, no events, no MRs) is empty/null.
- **Both acceptance surfaces embed the same section.** `GET /api/v1/repos/:id/aibom` (new `spec_registry` field, `aibom_version` 1.1 → 1.2) and `POST /api/v1/release/prepare` (new field on `ReleasePrepareResponse`) call the same builder, so release metadata and the AIBOM cannot disagree. No routes added (api/mod.rs untouched → ABAC registry unaffected); the additive response field breaks no consumer (only `api/aibom.rs` references `aibom_version`; the web client reads the agents array).
- **Merge-time evaluation is temporally correct.** An approval counts only when granted at or before the merge and not revoked/rejected at or before it; approval-after-merge and revoke-before-merge both yield `Some(false)`. `merged_at = mr.updated_at` is backed by the production merge processor (merge_processor.rs:1482, :2592 stamp `updated_at = now` at the Merged transition); merged-then-reverted MRs report indeterminable rather than evaluating against the revert timestamp (revert overwrites `updated_at`, merge_processor.rs:1712). Other `MrStatus::Merged` sites are test code only.
- **Tests are discriminating, not self-confirming.** Hand-traced every scenario in `bindings_report_merge_time_approval_truthfully` against the predicate: mr-1 → `Some(true)`, mr-2 (approval after merge) → `Some(false)` — kills a "currently approved" check, mr-3 (revoked before merge) → `Some(false)`, open MR → `None`, reverted → `None`, cross-repo MR excluded, no-spec_ref MR absent. `section_reports_every_ledger_status_and_approval_chain` covers all five statuses, drift flagging, stale-SHA/revoked event exclusion from chains, and cross-repo ledger isolation. `manifest_version_is_the_real_git_blob_sha` asserts against `git rev-parse` output in a real temp git repo. Router-level tests drive the real axum router.
- **Docs and matrices are accurate.** `docs/api-reference.md` rows for both endpoints describe the new section; coverage row 18 (spec-registry.md) → `implemented` with an accurate evidence note (honestly not `verified`); row 14's integration note updated; task frontmatter records `a4c24fc1` and `progress: ready-for-review`.

## Non-blocking observations

- `resolve_blob_sha` validates 40-hex output, so a SHA-256-format git repo would report `manifest_version: null`. This is the pre-existing codebase-wide convention (`read_blob`, `verify_spec_ref` both enforce 40-hex) and consistent with the merge gate; not introduced or worsened by this task.
- `bindings_summary.merged` counts reverted MRs as merged — intentional and documented ("merged (or merged-then-reverted)"); the per-binding `mr_status` carries the distinction.
- The working-tree `specs/tasks/task-169.md` modification (assignment body staged by the harness) reverts frontmatter/`## Shipped` relative to the candidate; the committed candidate tree is what review and verification scope.
