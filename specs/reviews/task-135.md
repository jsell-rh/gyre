# Review — task-135 (Implement gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies), §Gate Agent Lifecycle.
Base `cfeefbda` → candidate `1759222d5` (product commits `583c4a0` implementation,
`d715552` repair round; `4d175e0`/`1759222` process-only).
Verdict: **approved** (round 2 — all five round-1 findings repaired and verified).

## Probes (review stage; evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --tests` → exit 0 (all new production + test code
  type-checks, including the gate_executor / api::gates / api::merge_requests suites).
- `cargo test -p gyre-adapters --lib sqlite::quality_gate` → 3 passed
  (chain ordering by `position, created_at`; upsert persists position).
- `cargo test -p gyre-adapters --lib sqlite::review` → 11 passed
  (reviewer_persona/gate_id attribution round-trips; null stays null).
- `cargo test -p gyre-domain` → ok.
- Invariant scripts all exit 0: migration-versions (000061 next after 000060),
  migration-sql-portability, abac-route-registry (gate-chain route mapped),
  arch, mem-port-contracts, fabricated-scope-defaults, inert-enforcement,
  byte-slice-truncation.
- Deferred to the verifier (build wrapper blocks server test execution in the
  review stage): `cargo test --all`, plus focused
  `cargo test -p gyre-server --lib {gate_executor,api::gates,api::merge_requests,merge_processor}`.

## Round-1 findings (candidate 4c24e80, review f0f41ed) — all repaired

- **F1 (advisory RequiredApprovals gate blocked merging forever): fixed.**
  `check_gates_for_mr` now evaluates every result into (status, is_required, id)
  triples and delegates to the pure `decide_gate_readiness`, which never waits on
  or blocks on `required = false` gates in any state. Regression tests:
  `advisory_approvals_gate_must_not_block_merge`,
  `decide_gate_readiness_failure_precedes_waiting_in_any_order`.
- **F2 (Pending/Running gate masked a later Failed required gate): fixed.**
  `decide_gate_readiness` scans ALL results for required Failed (→ Err, merge
  processor fails the queue entry and the author is notified) before any waiting
  decision (→ Ok(false) requeue); order-independent by construction. Regression
  tests: `waiting_approvals_gate_must_not_mask_failed_required_gate`,
  the decide unit test (both orderings).
- **F3 (hollow coverage marking): resolved.** Coverage rows 6/7 now describe the
  repaired behavior; the claims match the code as verified.
- **F4 (chain PUT deleted post-merge gates): fixed.** `set_gate_chain` filters
  to PreMerge gates only, deletes only removed pre-merge gates, rejects chain
  entries reusing a post-merge gate's name (400), and GET gate-chain emits only
  the pre-merge chain (roundtrip-safe). Test:
  `gate_chain_put_preserves_post_merge_gates`.
- **F5 (docs claimed Admin-only gate writes): fixed.** api-reference rows now
  document ABAC on the `gate` resource without the Admin claim.

## Spec contract verification (source review + probes)

§Gate Chains:
- Ordered chains of mixed gate types: `quality_gates.position`
  (migration `2026-11-12-000061`, portable SQL, next unused sequence number);
  SQLite/PG adapters `ORDER BY position, created_at`; mem adapter sorts
  identically; `create_gate` appends; `GET /repos/:id/gates` returns chain order.
- TOML chain configuration: `PUT/GET /api/v1/repos/:id/gate-chain` accepting and
  returning the spec's exact `[[gates]]` format (mixed types, `required`,
  `min_approvals`, `persona`); validation-before-mutation (duplicate names,
  missing command/min_approvals, unknown types, malformed TOML, wrong
  content-type all rejected with the chain untouched); same-named gates keep
  ids; absent pre-merge gates deleted. ABAC-mapped as resource `gate`.
- Parallel execution: enqueue → `trigger_gates_for_mr` creates one Pending
  result per gate and `tokio::spawn`s one task per gate (verified at
  gate_executor.rs:34-66).
- Wait/block/notify: merge processor maps Ok(false)→requeue, Err→fail queue
  entry with reason (merge_processor.rs ~1732); run_gate emits a GateFailure
  event and `notify_gate_failure` to the author on required failure.
- Optional gates: never waited on in any state, never block; advisory failures
  are recorded and surfaced as an MR comment attributed to `gate-agent:{id}`.
- RequiredApprovals really counts: distinct Approved reviewers (duplicates by
  the same reviewer count once), live re-evaluation each processor cycle
  promotes a waiting gate to Passed the moment the Nth distinct approval lands
  (the base's "delegated to merge processor" Passed-stub is gone).

§Gate Agent Lifecycle:
- Ephemeral: unique `gate-review-<uuid>` identity per run; token revoked on
  empty-command, timeout, and completion paths.
- Read-only: push denied for `review:submit`/`validation:report` in git_http;
  ABAC middleware allow-lists scoped tokens to MR routes only; gate-chain and
  all repo/admin routes unreachable.
- Identity-scoped: exact-match scope comparison; `mint_scoped` only called
  server-side with the gate id as the JWT `task_id` claim.
- Attributed (new): `Review.reviewer_persona` + `Review.gate_id` (migration
  000061, both SQL adapters + mem); derived in `submit_review` from the scoped
  token's `task_id` — `SubmitReviewRequest` has no attribution fields to forge;
  surfaced in `ReviewResponse`.
- Context windows captured (new): `capture_gate_agent_context` stores the
  exact bundle the agent sees (persona system prompt, MR diff, spec at the
  pinned SHA, MR title, task description) zstd-compressed in the conversation
  repository before the subprocess runs, retrievable for replay, referenced by
  a `gate_agent_context` audit event (conversation SHA, gate, persona, MR);
  validators captured too; capture skipped (logged) when the tenant scope is
  unresolvable — never a fabricated default tenant.

Test quality: the new tests kill the round-1 bugs by name (both orderings of
the masking scenario, advisory-approvals non-blocking), drive the real router
for chain PUT/GET round-trips and atomicity, mint a real scoped JWT for the
attribution test, and retrieve the captured context window from the store.
No mirrored-logic, stub-backed, or assertionless tests found in the diff.

Non-blocking observations (no action required):
- Gate-agent Approved reviews count toward RequiredApprovals distinct-reviewer
  counts — defensible under "count review approvals", but a future spec owner
  may want gate-agent reviews excluded from *peer* approval gates.
- `update_status(..., None, ...)` clears `started_at` on final gate-result
  writes (pre-existing pattern, inherited by the live-promotion call).
- Deleting a gate from a chain while an MR with its Pending result sits queued
  leaves that result defaulting to required (conservative block) until requeue.

Task hygiene: both product commits recorded in `commits:` frontmatter; process
commits touch only the task file; no exemption files touched; migration
numbering follows the next-unused rule.
