# Review — task-135 (Implement gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies) + §Gate Agent Lifecycle.
Candidate: `d6b6b3c5dad7a6263335e1d7832c2bd63a16c3f4` (base `f98b179253b9e3f12cbc9a165532cacf709244d9`).
Product commits: `583c4a07d`, `d71555220`, `6ccbdff17`, `2999bb5e7` (all recorded in
`commits:` frontmatter; `check-task-commit-attribution.sh` OK), plus rebase/merge/process
commits (`62c7de659`, `7181c1e47`, `d6b6b3c5d`).

Verdict: **approved** — both coverage sections are implemented with real production
behavior; no structural defects, no missing enforcement, no fake implementations, no
test inflation found. Two minor doc/data observations recorded below (non-blocking).
gyre-server test *execution* is deferred to the independent verifier (4 GiB wrapper
restriction in this sandbox — infrastructure, not a code defect); the full probe list
is recorded in `/tmp/stage/review-evidence/probes.md` and below.

## Round 1 (this review)

Probes run in this sandbox (all exit codes and outputs under
`/tmp/stage/review-evidence/`):

- `cargo test -p gyre-adapters --lib sqlite::quality_gate` → **3/3 passed** against real
  SQLite through migration `2026-11-12-000062_gate_chain_config`:
  `list_by_repo_orders_by_chain_position` (positions 2,0,1 with anti-correlated
  `created_at` — only `(position, created_at)` ordering produces the asserted sequence),
  `list_by_repo_position_tie_falls_back_to_created_at`, `save_upsert_updates_all_config_columns`
  (now also asserts `position` persists through upsert).
- `cargo test -p gyre-adapters --lib sqlite::review` → **11/11 passed**, including
  `review_attribution_persists_and_roundtrips` (`reviewer_persona`/`gate_id` round-trip
  through the real schema) and `review_without_attribution_stays_null`.
- `cargo test -p gyre-domain` → **474 + 8 passed**.
- `cargo check -p gyre-server --lib` and `--tests` → **exit 0** (all new cfg(test)
  code type-checks; test execution blocked by the build wrapper's 4 GiB budget —
  recorded for the verifier, not treated as a defect).
- Mechanical gates: `check-migration-versions.sh` (no duplicate Diesel versions; the
  renumbered `000062` vs the base's `000061_repo_access_overrides` is correct — both
  would have mapped to Diesel version `20261112000061` and silently dropped one),
  `check-abac-route-registry.sh` (new `/api/v1/repos/:id/gate-chain` route is in the
  resolver, resource `gate`; exemption file not grown), `check-mem-port-contracts.sh`,
  `check-arch.sh`, `check-task-commit-attribution.sh` — **all exit 0**.
- Rebase audit: `git diff 2999bb5e7..d6b6b3c5d -- crates/gyre-server/src/gate_executor.rs
  crates/gyre-server/src/api/gates.rs` is **empty** — the merge with base `f98b179`
  did not alter the gate implementation; the only `merge_requests.rs` drift is the
  base's task-203 search scoping (unrelated to gates). Working tree verified clean
  before and after review.

### §Gate Chains (Composable Policies) — verified real

- **Ordered chains of mixed types**: `quality_gates.position` (portable migration:
  standard-SQL `ADD COLUMN ... DEFAULT 0`, no dialect functions), `ORDER BY position,
  created_at` in SQLite and Postgres adapters, identical sort in the mem adapter and
  in `list_gates`. `create_gate` appends (max position + 1, per-repo duplicate-name 409
  guard). Adapter tests prove the ordering against real SQLite (probe above).
- **TOML chain configuration**: `PUT/GET /api/v1/repos/:id/gate-chain` accepts and
  returns the spec's exact `[[gates]]` document (PascalCase or snake_case type names;
  `min_approvals` per the spec's TOML). Validation runs entirely before the first
  mutation (`validate_chain`: duplicate names, empty names, unknown types, missing
  `command`/`min_approvals` → 400 with the chain untouched — pinned by
  `gate_chain_invalid_entries_rejected_and_chain_unchanged`); JSON body → 415.
  Same-named gates keep their ids (GateResults stay linked); absent pre-merge gates
  are deleted; `position` = array index. Post-merge recovery gates sharing the table
  are neither deleted nor re-phased, `GET` emits only the pre-merge chain, and a chain
  entry reusing a post-merge gate's name is a 400 (`gate_chain_put_preserves_post_merge_gates`).
- **Parallel execution**: enqueue (`api/merge_queue.rs:133`) calls
  `trigger_gates_for_mr`, which creates one Pending GateResult per gate and spawns one
  tokio task per gate — all gates run concurrently (pre-existing base mechanism,
  correctly leveraged; not newly faked).
- **Per-MR status tracking**: Pending → Running (on spawn) → Passed/Failed via
  `GateResultRepository::update_status`; the merge processor polls
  `check_gates_for_mr` every cycle at both candidate-selection and group-readiness
  sites (`merge_processor.rs:788`, `:1732`).
- **All required gates must pass**: `decide_gate_readiness` is a pure two-pass
  decision — required failures scanned across ALL results first (Err → queue entry
  Failed), then required Pending/Running (Ok(false) requeue), else Ok(true). Order-
  independent by construction; `decide_gate_readiness_failure_precedes_waiting_in_any_order`
  pins both orderings plus advisory-never-blocks in any state.
- **Failed required gate blocks and notifies author**: Err → `MergeQueueEntryStatus::Failed`
  with the gate-failure reason; `run_gate` additionally emits a `GateFailure` workspace
  event carrying the gate output and calls `notify_gate_failure` (inbox notification,
  HSI §2) — verified present in code (pre-existing path, real queries).
- **RequiredApprovals really counts**: the base's hollow
  `(Passed, "delegated to merge processor")` is replaced by
  `run_required_approvals_gate` counting **distinct** reviewers with Approved reviews
  (one reviewer approving twice is still one — pinned by
  `required_approvals_counts_distinct_reviewers`), staying Running until the minimum
  is met, with live re-evaluation each processor cycle in `check_gates_for_mr` that
  persists the promotion to Passed the moment the Nth approval lands
  (`required_approvals_gate_waits_until_approvals_arrive`).
- **Optional gates never block**: `required = false` gates are neither waited on nor
  blocking in any state — including RequiredApprovals gates short of their minimum
  (`advisory_approvals_gate_must_not_block_merge`, plus the pure unit test).
  Advisory failures are recorded AND surfaced as an MR comment attributed to
  `gate-agent:{gate_id}` (`advisory_gate_failure_attaches_mr_comment`; required
  failures attach no advisory comment — pinned by the sibling negative test).

### §Gate Agent Lifecycle — verified real

- **Ephemeral and single-purpose**: `gate-review-<uuid>` identity per run; token
  revoked (`revoke_gate_token`) on every exit path — success, non-zero exit, spawn
  failure, and timeout (revocation precedes the result match).
- **Read-only**: `git_http::git_receive_pack` denies push for exact-match
  `review:submit`/`validation:report` scopes before workspace/repo resolution
  (verified in code; task-134 base, relied on correctly).
- **Identity-scoped**: `mint_scoped` mints an EdDSA JWT with `review:submit` only,
  issuer = the state's `base_url` (verification-repair `2999bb5e7` — the token now
  validates against the same issuer the extractor checks); `submit_review` binds the
  reviewer identity to the token subject, so the body cannot forge `reviewer_agent_id`.
- **Attributed**: `Review.reviewer_persona` + `Review.gate_id` are derived server-side
  in `submit_review` from the scoped token's `task_id` claim (the gate), never from
  body fields; surfaced in `ReviewResponse` and persisted through both SQL adapters
  (round-trip proven against real SQLite) and mem. A token whose gate no longer exists
  logs and records no attribution rather than fabricating one.
- **Context windows captured for audit**: `capture_gate_agent_context` stores the
  exact bundle the agent sees (persona system prompt, MR diff, spec content at the
  **pinned** `spec_ref` SHA — the test asserts v1 is captured, not the v2 tip — MR
  title, task description) zstd-compressed through the conversation repository
  **before** the subprocess runs (so even a crashed/timed-out reviewer leaves an
  auditable record), referenced by a `gate_agent_context` audit event carrying the
  conversation SHA, gate, persona, and MR; replayable at `GET /api/v1/conversations/:sha`.
  Validators are captured too. Capture is skipped and logged when the tenant scope is
  unresolvable — never stored under a fabricated identity (consistent with
  `check-fabricated-scope-defaults.sh`).

### Test-inflation check

None found. The new tests drive real paths: HTTP router over `test_state` with real
token minting via `agent_signing_key.mint_scoped` (`scoped_gate_review_records_persona_attribution`
submits a forged `reviewer_agent_id` in the body and asserts the server-derived
identity and persona win — it fails if the derivation is removed); the context-capture
test asserts the audit event AND retrieves the compressed blob with tenant scoping,
asserting the pinned-SHA spec content; the ordering tests use anti-correlated fixtures
so only the asserted ordering produces the expected sequence; `decide_gate_readiness`
unit tests kill the order-masking bug in both orderings. Each test targets a behavior
whose absence or regression it would detect.

## Findings

None blocking.

Minor (non-blocking, recorded for completeness):

- [-] `specs/coverage/system/agent-gates.md` rows 6 and 7 cite migration
  `2026-11-12-000061`, but the shipped migration is
  `2026-11-12-000062_gate_chain_config` — the rebase renumbering (correctly explained
  in `specs/tasks/task-135.md` §Shipped) was not propagated into the coverage notes,
  and `000061` now names the base's `repo_access_overrides` migration. Doc drift
  only; the implementation, task record, and mechanical version checks are correct.
- [-] `check_gates_for_mr`'s live approvals promotion calls
  `update_status(..., started_at: None, ...)`, and the SQLite adapter's
  `update_status` unconditionally sets `started_at` (NULL on None), clearing the
  started timestamp on promotion. This is the same pattern the pre-existing
  `run_gate` finish-transition already uses (also passes `None`), so it is not a
  regression introduced by this task; cosmetic data wart, no behavior impact.
- [-] `count_distinct_approvals` counts every distinct reviewer with an Approved
  review — including the MR author and other gates' `gate-review-*` agents — toward
  a RequiredApprovals minimum. The spec's "peer-approval" does not define an
  exclusion set, so this is spec-conformant as written; recorded in case a future
  spec amendment defines peer eligibility.
- [-] A chain PUT that removes a gate while an MR with Pending/Running results for
  it sits in the queue leaves results whose gate definition is gone;
  `check_gates_for_mr` defaults unknown gates to required, so such an MR requeues
  until its results are superseded. Pre-existing default (the `DELETE /gates` route
  had the same property before this task); edge case, not introduced here.

## Required verifier probes (gyre-server test execution exceeds this sandbox's budget)

- `cargo test -p gyre-server --lib scoped_gate_review_records_persona_attribution`
- `cargo test -p gyre-server --lib unscoped_review_has_no_gate_attribution`
- `cargo test -p gyre-server --lib gate_chain`
- `cargo test -p gyre-server --lib required_approvals`
- `cargo test -p gyre-server --lib check_gates`
- `cargo test -p gyre-server --lib advisory`
- `cargo test -p gyre-server --lib agent_review`
- `cargo test -p gyre-server --lib decide_gate_readiness`
- `cargo test -p gyre-server --lib ralph_loop::tests::gate_failure_respawns_agent_with_fresh_context`
- Task acceptance criterion: `cargo test --all`
