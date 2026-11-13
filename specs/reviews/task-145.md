# Review — task-145 (agent-gates.md §Part 2 "What This Prevents" — spec enforcement validation tests)

Spec: `specs/system/agent-gates.md` §Part 2 "What This Prevents" — the 7-row
attack/drift table. Task: integration test per row plus merge-level unit tests
for the forge enforcement policies that lacked them.

Candidate: `41c27f12` (branch `pipeline/task-145/cde4afb17db74d0bbc0e3fea7f51172a-1`),
base `4dc0eb9b`. Product-surface commit under review: `f4b7f151`
(`test(task-145): add spec enforcement validation tests for attack/drift prevention`).
Diff is test-only plus spec/task bookkeeping — no production code changes
(`merge_processor.rs` +232 lines entirely inside `mod tests`).

Verdict: **approved** (source-level). Test execution is deferred to the
independent verifier per the sandbox build budget — required commands below.

## What was reviewed

`crates/gyre-server/tests/spec_enforcement.rs` (new, 7 integration tests) and
two new unit tests in `merge_processor.rs`
(`require_spec_ref_blocks_mr_without_spec_ref`,
`require_approved_spec_blocks_unapproved_spec`).

## Independent probes (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --tests` via the build wrapper: **EXIT=0**
  ("Finished `dev` profile ... in 9m 55s", cold, single-job; warm re-run 0.49s).
  Both the new unit tests and the integration suite type-check against the
  real `AppState`/REST surfaces. Log: `cargo-check-gyre-server-tests.log`.
- `scripts/check-task-commit-attribution.sh` OK; `check-arch.sh` OK;
  `check-conditional-test-guards.sh` OK.
- `check-assertionless-tests.sh` exits 2 in this sandbox — environment issue,
  not the candidate: the script's awk uses gawk-only 3-arg `match()` and this
  sandbox has mawk 1.3.4 (reproduced on an unrelated base file; the candidate
  does not touch scripts/). Manual check: every new test contains multiple
  hard assertions.
- `cargo test -p gyre-server ...` is refused by the build wrapper (gyre-server
  codegen exceeds the 4 GiB model-sandbox budget). **The verifier must run:**
  - `cargo test -p gyre-server --test spec_enforcement`
  - `cargo test -p gyre-server --lib -- merge_processor::tests::require_spec_ref_blocks_mr_without_spec_ref merge_processor::tests::require_approved_spec_blocks_unapproved_spec`
  - Both need a loopback TCP listener (capabilities.json reports listeners
    supported; verifier should confirm).

## Verification detail

Every assertion was traced to the production code it exercises (all in the
base from task-134/138/139; the candidate adds only tests):

- **Row 1 (no spec)** — `require_spec_ref` violation reason
  "spec policy requires a spec_ref but MR has none" (merge_processor.rs:1943);
  entry Failed, MR open, `git show main:feature.txt` fails (feature commit
  provably absent from main); control MR with a spec_ref merges through the
  real git2 adapter. Discriminating: a no-op policy would merge the first MR
  and fail the test.
- **Row 2 (nonexistent SHA)** — well-formed 40-hex SHA resolving to nothing;
  `require_approved_spec` → `verify_spec_ref` error "spec '{path}' at SHA
  '{sha}' has no active approval in the ledger" (api/gates.rs:528); the MR
  spec endpoint 404s via `read_blob` failure (merge_requests.rs `get_spec`).
  Observation (non-blocking): the merge processor has no rejection message
  distinct from the approval check that says "SHA not found"; the test
  validates the actual enforcement surface and independently proves the SHA
  is unreachable via the 404.
- **Row 3 (unapproved spec)** — real committed spec file, real blob SHA
  (`git rev-parse HEAD:<path>`); a Pending ledger row authorizes nothing
  (`is_active()` requires Approved); approving that exact (path, sha)
  releases the block and a fresh MR merges. Kills the "ledger row exists ⇒
  authorized" bug class.
- **Row 4 (spec changed after code written)** — v1→v2 blob SHAs differ
  (asserted); `warn_stale_spec` merges AND leaves the `forge:spec-policy`
  advisory MR comment ("⚠️ Stale spec reference: ...", merge_processor.rs:2766);
  `require_current_spec` blocks with "spec policy requires current spec"
  (merge_processor.rs:1994). Stale resolution goes through
  `resolve_spec_blob_sha`'s `specs/`-prefixed fallback (git_refs.rs:130) —
  matches the ledger-path pin shape used in production.
- **Row 5 (wrong spec version)** — a real `agent_review` gate whose reviewer
  process (bash+curl, scoped `review:submit` token, same contract as the
  existing `review_agent_driver.sh` fixture and gate_executor e2e tests)
  receives `GYRE_SPEC_FILE` written from `read_file_at_commit` at the PINNED
  blob SHA (git2_ops.rs:641-686 resolves blob pins directly). MR1 pins v1
  (answer 42) and implements 43: mismatch → `changes_requested` → gate
  Failed → merge blocked via `check_gates_for_mr` Err (merge_processor.rs:1731);
  MR2 pins v2 and implements 43 → approved → merged. Discriminating: if the
  machinery resolved HEAD instead of the pinned SHA, MR1 would be approved
  and the test fails.
- **Row 6 (retroactive modification)** — approval on SHA-1 stays active
  (`is_active` asserted directly) after the spec moves to SHA-2; MR
  referencing SHA-2 under `require_approved_spec` is rejected with the SHA
  named. Kills path-level (instead of SHA-level) approval matching.
- **Row 7 (unauthorized approver)** — registered agent JWT (minted with the
  real `agent_signing_key`, registered in `agent_tokens`) → roles [Agent] →
  subject.type "agent" → immutable `builtin:require-human-spec-approval`
  Deny at priority 999 (gyre-domain/policy.rs:185) → 403, with
  `record_abac_denied` awaited before the response (no audit race) recording
  policy_id `builtin-require-human-spec-approval`; no active ledger row
  created. Control: human Admin via API key → subject.type "user" (999 Deny
  no-match), 900 Allow Admin → 201, ledger records approver identity
  "user:Human Approver" (auth.rs sets agent_id from display_name for API
  keys). The control proves the 403 is the human-only policy, not
  default-deny.
- **Unit tests** — same shape as the existing task-139 merge-level tests
  (`test_state()` NoopGitOps, real `run_once` cycle): Failed entry with a
  reason naming the violation, author notification asserted, positive
  control merges. The other three policies
  (`warn_stale_spec`/`require_current_spec`/`require_signed_approval`)
  already had merge-level unit tests in the base — adding more would be the
  test inflation the goal forbids; adding only the two missing ones is the
  correct completion of acceptance criterion 2.
- Harness correctness: `build_state` wires the real `Git2OpsAdapter`
  (lib.rs:1202) so control merges are real git merges; the processor loop is
  spawned only by main.rs, so `run_once` is deterministic; each test binds
  its own ephemeral loopback listener with its own state/repos/policies —
  self-contained as required; `seed_builtin_policies` is called explicitly
  (idempotent), matching the established pattern.

## Observations (non-blocking)

- The task's Shipped section claims evidence "recorded under
  /tmp/stage/review-evidence" and "evidence retained" — no such artifacts
  existed at review start (this review created the directory). Possibly
  wiped between stages; either way the claims are unverifiable. This review
  re-ran the compile probe independently and recorded its own evidence.
- Row 2's rejection message semantics, as above — outcome enforced, cause
  proven separately via the 404; consistent with the task-139 enforcement
  design the tests were told to assume complete.
