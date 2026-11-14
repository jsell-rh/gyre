# Review: TASK-145 — Spec Enforcement Validation Tests (attack/drift prevention)

**Reviewer:** Independent review agent
**Date:** 2026-04-15
**Candidate:** `9cad5a848c571915a4af79e0dd7e7c71522a79de` (merged head `f2f4c0ef` + docs-only `9cad5a84`)
**Base:** `32c4faefdcd01744f5d2e3015a1a615b2cf83b6e`
**Verdict:** approved

---

## Scope of the diff (base → candidate)

- `crates/gyre-server/tests/spec_enforcement.rs` — new, 1112 lines (commit `f4b7f151`)
- `crates/gyre-server/src/merge_processor.rs` — +233 lines, entirely inside `mod tests`: the two missing unit tests `require_spec_ref_blocks_mr_without_spec_ref` and `require_approved_spec_blocks_unapproved_spec`
- `crates/gyre-server/src/git_http.rs` — test-only fix inside `#[cfg(test)]` (poll for owner notification; race fix, commit `0de2f4e4`)
- `specs/tasks/task-145.md`, `specs/tasks/task-130.md` (commit attribution), `specs/coverage/system/agent-gates.md` (row 23 → implemented)
- No production code changed relative to the assigned base.

## What was verified

**Per-row audit against the task contract (all 7 rows present, each a separate self-contained test):**

- Row 1 `code_without_spec_is_rejected` — `require_spec_ref` set via the REST policy surface (`PUT /repos/:id/spec-policy`); MR without `spec_ref` fails its queue entry with a reason naming `spec_ref`, MR stays open, `git show main:feature.txt` proves the commit never landed; control MR with a `spec_ref` merges. Traced to the real enforcement in `merge_processor.rs` (policy block, `fail_queue_entry`, `report_spec_policy_violation`).
- Row 2 `nonexistent_spec_reference_is_rejected` — well-formed 40-hex SHA resolving to nothing is rejected under `require_approved_spec` ("no active approval" naming the spec path), and the gate-agent spec channel `GET /merge-requests/:id/spec` 404s ("not found in repository"). Note: production has no policy-independent "SHA not found" rejection at merge time; the unresolvable SHA is caught by the approval-ledger check (no approval can exist for a blob never in the repo) plus the 404 on the spec channel. This matches the actual task-138/139 enforcement design the task instructs us to validate; recorded as an observation, not a defect.
- Row 3 `unapproved_spec_is_rejected_until_approved` — real committed spec file with its real blob SHA; a **Pending** ledger row authorizes nothing (kills the "any ledger row authorizes" bug); approving the exact (path, sha) releases the block and a fresh MR merges. Traced to `verify_spec_ref`'s `is_active()` per-SHA query.
- Row 4 `stale_spec_warns_or_blocks_by_policy` — same repo moves the spec v1→v2; under `warn_stale_spec` the merge proceeds AND the advisory lands as an MR comment from `forge:spec-policy` (asserted content "Stale spec reference"); under `require_current_spec` the stale ref is blocked with a reason naming the policy. Both directions tested.
- Row 5 `gate_agent_reviews_pinned_sha_and_catches_wrong_version` — real `agent_review` gate, real reviewer subprocess (bash+curl) reading `GYRE_SPEC_FILE`/`GYRE_DIFF_FILE` (both written by `gate_executor` from the pinned spec content and the MR diff), submitting its verdict through `POST /merge-requests/:id/reviews` with the scoped `review:submit` token. Wrong version (implements HEAD's answer, MR pins v1) → `changes_requested` → gate Failed → entry Failed, MR open; correct pairing → merged. This test would catch a "gate reviews HEAD instead of pinned SHA" regression: if the pinned-spec channel served HEAD, MR1 would be approved and the assertion would fail.
- Row 6 `retroactive_spec_modification_requires_new_approval` — v1 approval stays active after the spec moves to v2; v2 has no active approval and an MR referencing it under `require_approved_spec` is rejected naming the unapproved SHA.
- Row 7 `unauthorized_approver_is_denied_by_abac` — agent JWT (registered in `agent_tokens`, minted via the real signing key) gets 403 from the immutable `builtin:require-human-spec-approval` Deny; denial recorded in the audit trail with `policy_id = builtin-require-human-spec-approval`; no active ledger row created; human Admin via API key succeeds (control proving it is not default-deny) and the ledger records the approver identity. Policy id and approver-identity format (`user:{display_name}` from the API-key auth path) verified against source.

**Unit tests for the 5 forge enforcement policies at HEAD:** `require_spec_ref_blocks_mr_without_spec_ref` and `require_approved_spec_blocks_unapproved_spec` added by this candidate; `warn_stale_spec_adds_advisory_comment_without_blocking`, `require_current_spec_blocks_stale_spec_ref`, `require_signed_approval_blocks_unsigned_spec_approval` already in the assigned base (task-139 round). All five present, each with a negative case and a positive control.

**Self-containment:** every test starts its own server on an ephemeral loopback port (`build_state` in-memory, unique token), seeds its own workspace/repo/git dir, and sets its own policies. No shared fixtures.

**Error-message criterion:** each rejection assertion requires the reason to name the missing thing (`spec_ref`, the spec path, the SHA, "no active approval", "requires current spec").

**Mechanical checks (all pass, evidence in `/tmp/stage/review-evidence/`):**
- `/tmp/stage/bin/cargo check -p gyre-server --test spec_enforcement` — clean, 7m27s (also exercised the full build of web assets)
- `scripts/check-abac-route-registry.sh`, `scripts/check-migration-versions.sh`, `scripts/check-arch.sh` — pass
- All 5 product-surface commits are recorded in the task's `commits:` frontmatter

## Test-quality assessment

Each attack test includes a positive control that merges (rows 1, 3, 5) or succeeds (row 7 human approver), so the blocks are provably the policies under test and not default-deny or fixture breakage. Row 5 in particular is a genuine bug-killer for the pinned-vs-HEAD channel. Row 3's Pending-ledger case kills a real equivalence-class bug. The assertions are on observable outcomes (queue entry status, MR status, git main content, REST responses, ledger rows), not on mirrored internal logic.

## Deferred to the independent verifier (build-budget constraint)

- `cargo test -p gyre-server --test spec_enforcement`
- `cargo test -p gyre-server --lib merge_processor` (unit tests for the 5 policies)
- then the full `cargo test --all`

These were not executed in this review per the 4 GiB build-budget split; `cargo check` on the exact test target passed, and every assertion was traced to the production code path it exercises.

## Observations (non-blocking)

- Row 2's merge-time rejection reason is "no active approval" (the ledger check) rather than a policy-independent "SHA not found" — this is the actual production enforcement design; the unreachability itself is additionally proven by the 404 on `GET /merge-requests/:id/spec`.
