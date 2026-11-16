# Review: task-166 — Supply chain: SLSA provenance mapping, AIBOM release artifacts, laptop problem

**Candidate:** b1dfe23662a219b48666453f1c0527f4115cc597
**Base:** c66ee2ac9d0fff7e45cffc6dbeabdf90a3f3a70e
**Reviewer round:** 2 (repair re-review of prior findings from candidate 7b083fab)
**Verdict:** approved

## Prior findings — all three verified repaired

1. **git_rev_list `--` separator before the range (aibom.rs)** — FIXED. The
   `--` is dropped; the range is passed as a single argv element (callers
   pass a `rev-parse`-resolved hex SHA or an `is_valid_ref`-validated
   `from`, which rejects leading dash and `--`, so no option injection).
   Verified against a real bare repo with git 2.43.0: both invocation
   shapes (single SHA, `from..to`) exit 0 and emit the expected SHAs
   (/tmp/stage/review-evidence/probe-rev-list.md). `?format=release` can
   now actually produce the artifact.

2. **SLSA provenance not scoped to the subject ref (provenance.rs)** —
   FIXED. `get_slsa_provenance` now enumerates the subject's history via
   `git_rev_list(resolved_sha)` and filters both the per-commit attribution
   and the weakest-link `slsaBuildLevel` to that set.
   `slsa_provenance_statement_shape_and_level_mapping` asserts an
   agent-commit record targeting an unreachable SHA neither appears in the
   statement nor caps the level; `slsa_weakest_link_among_reachable_commits`
   pins the reachable-commit cap semantics.

3. **stack-drift default limit mismatch (stack_attest.rs)** — FIXED.
   `params.limit.unwrap_or(100).min(500)` matches the documented
   default-100/cap-500 bounds; docs/api-reference.md consistent.

## Independent verification performed

- Real-git probe of the repaired rev-list invocation (evidence above).
- `cargo check -p gyre-server --tests` — exit 0 (type check of all new
  code and tests; full server test-binary compilation deferred to the
  verifier per the 4 GiB sandbox budget, honestly disclosed in the
  coverage row).
- `cargo test -p gyre-domain` (572 pass) and `cargo test -p gyre-cli`
  (223 pass, incl. `cli_push_stack_override_reason_parses`).
- All 15 mechanical invariant scripts pass, including check-arch,
  check-abac-route-registry (new `/slsa-provenance` and `/stack-drift`
  routes carry `RouteResourceMapping` entries), and the scope/forgery/
  forwarded-header checks.
- Source read of the full diff base..candidate.

## Assessment against the acceptance criteria

- **AIBOM release artifact**: `build_release_aibom` builds the structured
  JSON from real state — git-range commit enumeration (so `human_commits`
  comes from actual git history), per-agent registered stacks
  (KV `agent_stacks`), lockfile fingerprint read from the release tip's
  tree, repo stack policy, Rekor entries via the commit-signature
  repository, and drift history via the audit repository. Release fixture
  carries `gyre-stack.lock` into the tip tree, so the `verified` /
  `lockfile_hash` assertions are genuinely exercised. Unresolvable ref →
  404, unknown format → 400, both tested.
- **SLSA provenance**: in-toto Statement v1 envelope
  (`_type`/`predicateType: https://slsa.dev/provenance/v1`), subject =
  resolved ref with `gitCommit` digest, level mapping L1→0, L2→1/2
  (lockfile-pinned), L3→3 evaluated against real state; weakest link over
  reachable commits. Coverage row honestly notes hermeticity remains a
  Level-3 runtime property — the endpoint maps the chain, as specced.
- **Laptop problem**: every drift finding (block rejection, warn rejection
  for missing reason, warn acceptance) records a `StackDriftEvent` as a
  `stack_drift_detected` audit event with full context; warn-mode
  acceptance genuinely requires `X-Gyre-Stack-Override-Reason` (403 +
  blocked drift event otherwise — enforced in `enforce_stack_policy`,
  tested end-to-end through real receive-pack requests);
  `GET /repos/:id/stack-drift` queries the audit repository scoped by
  workspace + repo with a cross-repo leak test. CLI flag
  `gyre push --stack-override-reason` wired and documented.

## Notes for the verifier (non-blocking)

- Run the focused gyre-server test list recorded in
  /tmp/stage/review-evidence/focused-tests.md, then `cargo test --all`.
- No generated-output observations: web/dist untouched by this candidate.
