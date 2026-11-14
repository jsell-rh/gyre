# Review — task-145 (Spec enforcement validation tests for attack/drift prevention)

Spec: `specs/system/agent-gates.md` §Part 2 "What This Prevents".
Candidate: `f81930428af6d9be721764f19357afe7bd8a4fd0` (base `1ee52c43`).
Product commits under review: `f4b7f151` (test suite + merge-processor unit tests), `0de2f4e4`/`0c052944` (suite upkeep across merged upstream changes), `6128a39c` (mint-arity fix for the Row 7 fixture after the task-130 merge), `0d3c6a58` (checkpoint).
Verdict: **approved** (see `/tmp/stage/verdict.json`).

## Probes run (all at candidate `f8193042`)

- `cargo check -p gyre-server --test spec_enforcement` → exit 0 (8m37s). This is the exact probe that failed with E0061 in the earlier round; the `6128a39c` fix compiles clean.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 **on the candidate tree** (detached worktree at `f8193042`; all four task-labeled product commits recorded in the frontmatter, task-130 cross-attribution for `6128a39c` present). It fails only against the review working tree because the round-reset reverted `specs/tasks/task-145.md` to `commits: []` — not candidate content.
- All mechanical invariant checks on the candidate tree → exit 0: `check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers` (79 handlers), `check-mcp-write-tools`, `check-migration-versions`, `check-migration-sql-portability`, `check-dead-message-kinds`, `check-mem-port-contracts`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-inert-enforcement`, `check-forwarded-header-trust`, `check-in-memory-state-stores`, `check-unbounded-external-http`, `check-forged-scope-fields`.
- Static binding verification: every string/status the suite asserts on was traced to the production code that produces it (details below).
- Evidence: `/tmp/stage/review-evidence/`.

Deferred to the independent verifier (4 GiB build budget — server test binaries exceed the review sandbox): `cargo test -p gyre-server --test spec_enforcement`, `cargo test -p gyre-server --lib merge_processor`, then full `cargo test --all`.

## What was verified

**All 7 attack/drift rows have integration tests against real surfaces** (`crates/gyre-server/tests/spec_enforcement.rs`): each test starts its own server on an ephemeral loopback port (`build_state` + `seed_builtin_policies` + real axum router), configures policy via the real REST surface (`PUT /repos/:id/spec-policy` → `spec_policies.set_for_repo`, consumed by the merge processor via `get_for_repo`), creates MRs through `POST /merge-requests` (which validates `spec_ref` format) + real merge-queue enqueue, seeds real on-disk git repos with real blob SHAs (`git rev-parse HEAD:<path>`), and drives `merge_processor::run_once` — the same `process_next` cycle the spawned processor loop runs. No shared fixtures; each test owns its state.

**Assertions bind to real production behavior** (grep-verified, so the tests fail if enforcement breaks):

- Row 1/3/6 failure reasons: `merge_processor.rs` ~2059 (`spec policy requires a spec_ref but MR has none`) and `api/gates.rs::verify_spec_ref` (`spec '{path}' at SHA '{sha}' has no active approval in the ledger` — exact-SHA ledger query, `is_active()` = Approved-and-not-revoked; the Pending-row control in Row 3 and the unit test proves a non-active row authorizes nothing).
- Row 4: staleness via `git_refs::resolve_spec_blob_sha` (real `git` invocation; ledger-path → `specs/`-prefix fallback confirmed, so the tests' `system/identity.md` refs resolve the committed `specs/system/identity.md`); `require_current_spec` → Failed with "requires current spec"; `warn_stale_spec` → `record_stale_spec_advisory` comment from `forge:spec-policy` containing "Stale spec reference", merge proceeds.
- Row 5: enqueue triggers `gate_executor::trigger_gates_for_mr` (merge_queue.rs:133); the agent_review gate spawns a real reviewer process with `GYRE_SPEC_FILE` (blob at the **pinned** SHA via `read_blob`, never HEAD), `GYRE_DIFF_FILE`, and a scoped `GYRE_REVIEW_TOKEN`; the merge processor honors failed gates (`check_gates_for_mr`). The test is discriminating: if the gate reviewed HEAD instead of the pinned SHA, the wrong-version MR would be approved and the test's Failed assertion would fire.
- Row 7: `/api/v1/specs/:path/approve` is ABAC-mapped to resource `spec`, action `approve` (abac_middleware.rs:365); the immutable `builtin:require-human-spec-approval` Deny (priority 999, condition `subject.type != "user"`) is seeded by `seed_builtin_policies`; denial → 403 + `abac_denied` audit event with `policy_id` = policy **id** `builtin-require-human-spec-approval` (policy_engine.rs sets `matched_policy` from `policy.id`), matching the test's audit assertion. The human control (API key, Admin role → `builtin-admin-all-operations` Allow; `AuthenticatedAgent.agent_id` = display name → approver `user:Human Approver`) proves the 403 is the human-only policy, not default-deny.
- Unit tests for all five forge enforcement policies exist at merge level: `require_spec_ref_blocks_mr_without_spec_ref` and `require_approved_spec_blocks_unapproved_spec` (new, `f4b7f151`), plus `warn_stale_spec_adds_advisory_comment_without_blocking`, `require_current_spec_blocks_stale_spec_ref`, `require_signed_approval_blocks_unsigned_spec_approval` (task-139, present in the candidate tree via merge).

**Error messages are clear and actionable**: each rejection reason names the missing field / spec path / SHA / policy (asserted verbatim against production strings).

**Positive controls** throughout (compliant MRs merge; approving the exact (path, sha) releases the block; the correct-pinning MR merges; the human approver succeeds) — the tests kill real bugs, not just happy paths.

**Bookkeeping**: coverage row 23 marked implemented with an honest note (including "independent verification pending"); task frontmatter lists all product commits; task-130 cross-attribution for `6128a39c` recorded; the attribution gate passes on the candidate tree.

## Observations (non-blocking)

- **Row 2 ("SHA not found") is enforced through the approval ledger**: there is no dedicated merge-time "SHA exists in repo" check in production (task-139's design); a nonexistent SHA can never carry an active approval, so under `require_approved_spec` the MR is rejected with a reason naming the spec path and SHA. The suite additionally proves the pinned SHA is genuinely unresolvable (MR spec endpoint 404s). The observable contract of the row — "MR rejected" — is validated; a repo-existence check, if ever wanted, belongs to the enforcement tasks (138/139), not this validation task.
- The branch carries merges of upstream main (task-112/125/130/134/139 content); branch-authored changes are confined to the test suite, test-support polling in `git_http.rs` (race fix, assertions unchanged), and docs/process commits.
- Runtime execution of the suite is deferred to the verifier per the build-budget constraint; compilation is clean and the implementer's recorded full-suite run on the pre-rebase tree (`9b9954f1`) had the spec-enforcement suite passing.
