# Review: task-139 — Implement forge enforcement policies for spec-to-code binding at merge

Comparison base: 3b734b1af744f594ec5d903be242b41ec0f287f2 → candidate
d222f883acec5e4f0ada0a7bec8f2f92374a0c6f (branch head after the main merge
`ce115b65a` and process commits). Diff: 24 files, +1628/−50 —
`SpecPolicy.require_signed_approval` across domain/ports/adapters/migration,
merge-processor enforcement + author reporting, pinned-SHA spec delivery
(endpoint, gate env, ABAC allow-lists), web policy toggles + tests, docs.
Generated `web/dist/` excluded (verifier-owned).

## Verdict: approved

Independent evidence supports the task contract. No blocking findings.
Probe logs: `/tmp/stage/review-evidence/` (summary.md indexes everything).

## Spec conformance (agent-gates.md §Forge Enforcement Policies)

All five policies are per-repo configurable and enforced in the merge
processor before executing a merge:

- **require_spec_ref / require_approved_spec / warn_stale_spec /
  require_current_spec** — pre-existing task-138 enforcement, retained
  through the main merge and now extended: blocking violations also emit a
  `GateFailure` workspace event and notify the author's spawning user;
  `warn_stale_spec` additionally records a deduplicated advisory MR comment
  (`forge:spec-policy`) instead of only an event.
- **require_signed_approval** (new) — `gates::verify_signed_spec_ref` does
  the same exact `path@sha` ledger lookup as `verify_spec_ref` and requires
  an active approval carrying a non-blank signature; whitespace-only
  signatures are treated as unsigned; one signed active approval among
  several rows for the same version satisfies the policy; an invalid
  spec_ref format or missing SHA fails closed. Blocked merges fail the
  queue entry with a reason naming the missing signature.
- Configuration surface is complete: domain field (`serde(default)`, opt-in
  false), SQLite + Postgres rows (portable `INTEGER NOT NULL DEFAULT 0`
  migration 000059), mem store, `GET/PUT /api/v1/repos/:id/spec-policy`
  round-trip, and the RepoSettings UI toggles (which also fix the
  pre-existing silent-no-op bindings `require_approval` /
  `stale_spec_warning` that never matched the API field names).

## Spec conformance (§Integration with Agent Gates)

The gate agent receives the spec at the **pinned** SHA, not HEAD, through
two real channels:

- `build_review_agent_context` resolves the MR's `spec_ref` via
  `GitOpsPort::read_file_at_commit`, now extended (with a port-contract
  test) to the production pin shape: MR spec_refs carry the spec ledger's
  **blob** SHA with a ledger path that is not a repo tree path — the blob is
  the content. A SHA that is neither commit nor blob is an error
  (fail-closed), preserving the pre-existing anti-fail-open behavior
  (`errors_on_unknown_sha` still passes).
- `GYRE_SPEC_URL` → `GET /api/v1/merge-requests/{id}/spec` serves the blob
  at the pinned SHA (`git cat-file blob`, SHA validated 40-hex before
  reaching the command line). The route is registered in the ABAC resolver
  and added to both the `review:submit` and `validation:report` scoped-token
  allow-lists, so the agents the URL is minted for can actually fetch it.

## Evidence (this sandbox)

- `cargo test -p gyre-adapters --lib spec_policy` — pass on a fresh SQLite
  DB: proves migration 000059 runs and all five flags persist (including
  the upsert update path).
- `cargo test -p gyre-adapters --lib read_file_at_commit` — 4 pass
  (blob-pin resolution, fail-closed unknown SHA).
- `cargo check -p gyre-server` — clean (5m46s).
- `npm ci` + `npx vitest run src/__tests__/RepoSettings.test.js` — 64/64.
- **Mutation probes** (both restored, `git diff` clean afterwards):
  - breaking the SQLite write path for `require_signed_approval` → the
    round-trip test fails ("require_signed_approval must persist");
  - reverting the Svelte binding to the old wrong field name → the
    "binds toggles to the API policy field names" test fails.
  The new tests kill real defects; they are not self-confirming.
- All mechanical guards pass (arch, ABAC route registry + exempt handlers,
  MCP write tools, migration versions + SQL portability, mem port contracts,
  dead message kinds, byte-slice truncation, relative-path defaults,
  fail-open ref resolution, fabricated/lookup scope defaults, lossy-secret
  conversion, inert enforcement, forwarded-header trust, in-memory state
  stores, unbounded external HTTP, forged scope fields).
- Commit attribution: candidate HEAD lists all 7 task-labeled commits
  (99edd0bc, 059586a3, ec05bf9a, 33cc4941, c92e8f53, 87e2aed4, d08359f1).
  Note: `check-task-commit-attribution.sh` fails **only** because the
  review stage reset the working-tree task file to the assignment version
  (`commits: []`); the committed candidate file is complete.

## Deferred to the verifier

The review-stage cargo wrapper blocks gyre-server codegen (4 GiB budget),
so server test compilation/execution was deferred. Required probes:

    cargo test -p gyre-server --lib merge_processor::tests::require_signed_approval_blocks_unsigned_spec_approval
    cargo test -p gyre-server --lib merge_processor::tests::warn_stale_spec_adds_advisory_comment_without_blocking
    cargo test -p gyre-server --lib api::gates::tests::signed_spec_ref_requires_signature_on_active_approval
    cargo test -p gyre-server --lib api::merge_requests::tests::get_mr_spec_serves_pinned_sha_not_head
    cargo test -p gyre-server --lib gate_executor::tests::agent_review_context_resolves_blob_pinned_spec_ref
    cargo test -p gyre-server --test task134_review_probe scoped_gate_tokens_can_fetch_pinned_mr_spec
    cargo test --all

Every API these tests use was signature-verified by hand against the
current tree (see review-evidence/summary.md).

## Non-blocking observations

1. Migration `2026-11-09-000059_require_signed_approval` reuses sequence
   000059 with a new date rather than the next unused number (000062). The
   derived Diesel version 20261109000059 is unique and sorts correctly
   between 20261108000059 and 20261112000060, so nothing is silently
   dropped; `check-migration-versions.sh` passes. Stylistic deviation.
2. `require_signed_approval` enforces presence of a non-blank signature on
   an active ledger approval, not cryptographic verification of the
   Sigstore signature — this matches the task plan verbatim ("reject if
   approval lacks signature field") and the task-138 ledger's
   `signature: Option<String>` recording.
3. `GET /merge-requests/:id/spec` decodes blob content with
   `from_utf8_lossy`; spec markdown is text, not security material.
4. Generated-output observation (verifier-owned): the committed
   `web/dist/` predates this review; `cargo check -p gyre-server` rebuilds
   it via build.rs (observed live), and the regenerated hashes differ from
   the committed bundle. Restored to the committed state after the probe;
   bundle regeneration is the verifier's stage, not a source defect.
