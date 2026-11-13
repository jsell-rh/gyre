# Review — task-135: gate chain configuration and gate agent lifecycle

**Candidate:** `6c82bb1b22c621f895a426fa7c11b6d83821b6d3`
**Base:** `4dc0eb9bbd0a2cddad39f478b7bc70167fa455ed`
**Reviewer round:** independent re-review after contract-restore round
**Verdict:** **approved** — see `/tmp/stage/review-evidence/` for probe
commands, sources, exit codes, and outputs.

## What was reviewed

The full base→candidate diff (26 files, +2184/−96): gate chain
configuration endpoints, chain-order persistence across all three
adapters, RequiredApprovals live evaluation, the order-independent
merge-readiness decision, advisory (`required = false`) gate handling,
gate-agent attribution, context-window capture, scoped-token lifecycle
integration, migration `2026-11-12-000062_gate_chain_config`, ABAC
registry entry, and the docs/coverage updates.

## Acceptance criteria — evidence

- **Ordered mixed-type gate chains** — `PUT/GET /repos/:id/gate-chain`
  parses/returns the spec's exact `[[gates]]` TOML (PascalCase or
  snake_case types), `position = array index`, per-repo name uniqueness,
  validation-before-mutation (invalid chain → 400, chain untouched),
  same-named gates keep ids, absent pre-merge gates deleted, post-merge
  gates preserved. Tests assert persisted state, not just responses.
- **Parallel gate execution on enqueue** — `enqueue` →
  `trigger_gates_for_mr` creates one Pending GateResult per gate and one
  `tokio::spawn` per gate; already-queued guard prevents duplicate
  result rows.
- **Per-MR gate status Pending→Running→Passed/Failed** — GateResult
  persistence with `update_status`; merge processor polls
  `check_gates_for_mr` each cycle.
- **All required gates pass before merge** — `check_gates_for_mr` +
  two-pass `decide_gate_readiness`: required failures scanned across ALL
  results before any wait decision (a Running required gate can never
  mask a Failed one, in any iteration order — regression-tested both
  orderings).
- **Failed required gate blocks + notifies author** — merge processor
  fails the queue entry with the failure reason; `notify_gate_failure`
  (pre-existing, verified present) delivers to the author; GateFailure
  workspace event emitted.
- **Optional gates never block** — advisory gates are skipped in the
  wait decision in any state (including RequiredApprovals short of
  minimum — no indefinite requeue); advisory failures record the result
  and attach an MR comment attributed to `gate-agent:{id}`; required
  failures post no advisory comment (tested).
- **RequiredApprovals really counts** — distinct approved reviewers
  (HashSet), live re-evaluation each processor cycle, promotion to
  Passed persisted; duplicate approvals by one reviewer don't satisfy
  min-2 (tested).
- **Gate agent lifecycle** — ephemeral: token minted per run and revoked
  after the process exits (including timeout/crash paths); read-only:
  `review:submit` tokens denied git push (git_http exact-match check)
  and confined to the MR review routes by the ABAC allow-list (deny
  before policy evaluation); attributed: `reviewer_persona`/`gate_id`
  derived server-side from the scoped token's gate — request-body fields
  cannot forge them (tested with a forged `reviewer_agent_id`, which is
  also overridden by the token subject); context windows captured: the
  exact bundle (persona prompt, diff, pinned-SHA spec, task description)
  is zstd-compressed into the conversation repository before the
  subprocess runs, referenced by a `gate_agent_context` audit event,
  retrievable for replay; capture is skipped (logged) when the tenant
  scope is unresolvable — never fabricated.
- **`cargo test --all`** — cannot run here (build wrapper rejects
  gyre-server test compilation: >4 GiB). Focused evidence:
  `cargo test -p gyre-domain --lib` 499 passed;
  `cargo test -p gyre-adapters --lib sqlite::quality_gate` 3 passed
  (chain ordering + tiebreak); `sqlite::review` 11 passed (attribution
  round-trip); `cargo check -p gyre-server` clean. **Verifier must
  run:** `cargo test --all`, plus
  `cargo test -p gyre-server --lib gate_executor::`,
  `cargo test -p gyre-server --lib gates::`,
  `cargo test -p gyre-server --lib merge_requests::`.

All 19 mechanical invariant scripts pass at the candidate commit
(arch, migration versions/portability, ABAC registry, commit
attribution, mem port contracts, inert enforcement, dead message kinds,
forged scope fields, fabricated scope defaults, byte-slice truncation,
relative path defaults, lossy secret conversion, unbounded external
HTTP, in-memory state stores, forwarded-header trust, fail-open ref
resolution, MCP write tools).

## Non-blocking observations

1. `specs/coverage/system/agent-gates.md` rows 6–7 cite migration
   `2026-11-12-000061`; in this tree the attribution columns come from
   `2026-11-12-000062_gate_chain_config` (renumbered after main took
   000061) and `quality_gates.position` from task-168's
   `2026-10-08-000062_gate_position`. Stale prose cross-reference only —
   the schema objects and behavior all exist and are tested. Recommend a
   process-round correction; does not affect any acceptance criterion.
2. The assigned checkout's working tree carries the executor's restore
   of the protected task contract (`commits: []`, `## Shipped` removed),
   per durable finding e359388. The candidate commit itself carries full
   commit attribution (all six product-surface task-135 commits listed;
   `check-task-commit-attribution.sh` passes at HEAD). The next
   implementation round must re-record attribution and shipped notes
   without touching the protected contract text.
