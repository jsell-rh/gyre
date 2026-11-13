# Review — task-135 (Gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies) + §Gate Agent Lifecycle.
Candidate under review: `505edec1` (base `f764bc00`). Attributed product commits:
`583c4a07`, `d7155522`, `6ccbdff1`, `2999bb5e`, `089cdce3`, `9ff6b003`.
Verdict: **approved** — this round is a base reconciliation (task-187 merge) plus
CI-failure triage; the task-135 product code is unchanged since the last approved
candidate and the reconciliation is type-clean.

## Round N (this review) — reconciliation + CI triage verification

Delta since the previously approved candidate (`738060fa`): merges of main
(`bb7af039`, `f19438f2`), docs/CI triage (`ae1da447`), attribution record
(`505edec1`).

Probes run (evidence under `/tmp/stage/review-evidence/`):

- `git diff 738060fa..505edec1 -- crates/gyre-server/src/gate_executor.rs crates/gyre-server/src/api/gates.rs crates/gyre-adapters/src/{sqlite,postgres}/{quality_gate,review}.rs crates/gyre-domain/src/{quality_gate,review}.rs` → **empty** — the approved gate-chain/lifecycle implementation survived the merges untouched. The only task-135-file changes are in `merge_requests.rs`, where main's search-indexer refactor (unrelated task) replaced inline `search_scope::index_doc` calls with `search_indexer::index(...)` around the untouched task-135 attribution block.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` (candidate worktree) → **Finished, clean** (6m07s) — the merged tree type-checks.
- `cargo test -p gyre-domain` → **515 passed** (507 + 8), 0 failed.
- `cargo test -p gyre-adapters` → **516 passed, 6 ignored**, 0 failed (includes the task-135 chain-ordering and review-attribution round-trip tests).
- All 20 mechanical `scripts/check-*.sh` lints → pass at the candidate commit (`check-task-commit-attribution.sh` initially failed **only** because the controller's uncommitted working-tree reset of `specs/tasks/task-135.md` blanks `commits:`; a clean worktree at `505edec1` passes: every task-labeled commit is recorded).
- `python3 -m unittest discover -s scripts -p 'test_dev*.py'` (with `GYRE_PIPELINE_STAGE` unset) → **61/61 OK**.
- `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'` → 286/288; the 2 failures (`test_crash_cleanup_stops_durable_agent_before_capturing_source`, `test_real_timeout_kills_check_and_records_infrastructure_code`) **reproduce identically at the base commit `f764bc00`** — pre-existing sandbox restrictions (killpg denial), not candidate defects.
- `node --test scripts/dashboard/test/*.test.mjs` → 7/7; `node --test scripts/test/*.test.mjs` → 18/18.

### CI failure triage (run 43465259045, "Development controller") — verified

`git diff --name-only d9c419a0 bb7af039` outside `crates/ docs/ specs/ web/dist/`
yields exactly `Cargo.lock` (adds the `toml` crate to gyre-server). The failed
job runs only `go test` on `tools/fullsend-dispatcher`, the two python script
suites, and the node script suites (`.github/workflows/ci.yml`) — all
byte-identical to the passing baseline `d9c419a0`. No task-135 change is
reachable from that job; the triage conclusion (transient/infra failure) is
sound.

### Verifier-required probes (gyre-server test compilation is deferred to the
independent verifier per the 4 GiB budget rule)

- `cargo test -p gyre-server gate`
- `cargo test -p gyre-server merge`
- `cargo test --all`
- `cd web && npm ci && npm test` (no web source is part of this task's diff; sanity only)

## Prior-round verification carried in this record (implementation unchanged since)

- **Gate chains (§Gate Chains):** `PUT/GET /repos/:id/gate-chain` accepts/returns
  the spec's exact `[[gates]]` TOML (mixed types incl. `AgentReview`,
  `RequiredApprovals` with `min_approvals`, `required` flags); replacement is
  atomic (validation-before-mutation), preserves gate ids by name, deletes
  absent pre-merge gates, preserves post-merge gates (collision → 400).
  Adapters (SQLite + PG + mem) order by `(position, created_at)`; migration
  `2026-11-12-000062` adds `reviews.reviewer_persona`/`gate_id` (portable SQL,
  no duplicate Diesel version). Parallel execution: enqueue →
  `trigger_gates_for_mr` → one tokio task per gate; merge processor polls
  `check_gates_for_mr` each cycle. Decision is two-pass and order-independent:
  required failures block+notify even while other required gates are
  Pending/Running; advisory gates never block in any state (including
  RequiredApprovals short of minimum); `RequiredApprovals` counts DISTINCT
  approvers live and the merge processor promotes the gate when the Nth
  approval lands. Advisory failures attach an attributed MR comment.
- **Gate agent lifecycle (§Gate Agent Lifecycle):** ephemeral (scoped token
  revoked on every exit path incl. crash/timeout — asserted by tests),
  read-only (`review:submit`/`validation:report` exact-match scopes; push
  denied), attributed (`reviewer_persona`/`gate_id` derived server-side from
  the scoped JWT's `task_id` claim in `submit_review` — body fields cannot
  forge it; persisted in both SQL adapters, round-trip tested), context windows
  captured (zstd-compressed exact bundle — persona prompt, MR diff, spec at
  the pinned `spec_ref` SHA, task description — stored via the conversation
  repository before the subprocess runs, replayable at
  `GET /api/v1/conversations/:sha`, referenced by a `gate_agent_context` audit
  event; validators captured too; capture skipped+logged when the tenant scope
  is unresolvable, never fabricated).

Test-inflation check: none found. The guards are behavioral and kill real bugs
(the two-pass decision regression test encodes the exact F2 masking ordering;
the context-capture test asserts the pinned-SHA spec content, not the tip; the
distinct-approvals test kills the count-as-submissions bug; token-scope tests
validate minted JWTs against the real signing key from a live `printenv`
subprocess).

## Non-blocking observations (recorded, no action required for this task)

- The spec's §Gate Chains TOML example configures `AgentReview` gates with a
  persona only (no `command`); the implementation accepts such chain entries
  but the gate fails closed at runtime with a clear
  "no agent command configured" message. This is the approved task-134 design
  ("no-command fails closed"); if persona-only AgentReview gates should
  resolve a default agent runner (compute target / `GYRE_AGENT_COMMAND` /
  entrypoint, as `spawn.rs` does), that is a task-134/spec-lifecycle change,
  not a task-135 regression.
- The checkout's working tree carries an uncommitted controller-side reset of
  `specs/tasks/task-135.md` (`progress: not-started`, `commits: []`); it makes
  `check-task-commit-attribution.sh` fail **in the working tree only** — the
  committed candidate passes. The verifier must run that check against the
  committed tree, not the dirty working tree.
