# Review: task-135 — Implement gate chain configuration and gate agent lifecycle

Comparison base: 6f5a631c9fc09664a3e766cd10536f8b56d9ce2f (current main, task-195
manifest-driven gate selection) → candidate 4ca52f89c4e6d76353fa860360372ee015d0040a
(branch `pipeline/task-135/2428cd2a20194fe28b4e069be6ee441f-1`, merge resolution
dbcd62f1 + docs 5fdee386 + attribution 4ca52f89). Diff: 26 files, +2219/−96.
No `scripts/` changes, no exemption-file growth, no test deletions, no gate
weakening. Base is an ancestor of the candidate, so the prior DIRTY merge state
against main is resolved by construction. No `web/` source changes (generated
`web/dist` untouched — verifier-owned).

## Verdict: APPROVED (independent review round 2428cd2a)

## Evidence (commands, exit codes, outputs under /tmp/stage/review-evidence)

- `bash scripts/check-migration-versions.sh` — exit 0, no duplicate Diesel versions
  (candidate's `2026-11-12-000062_gate_chain_config` → version `20261112000062`,
  distinct from base's `2026-11-13-000062_team_slug_lead_membership`).
- `bash scripts/check-arch.sh` — exit 0.
- `bash scripts/check-abac-route-registry.sh` — exit 0 (new
  `/api/v1/repos/:id/gate-chain` route registered as `gate` resource in
  `abac_middleware.rs`).
- 15 further mechanical checks (abac-exempt-handlers, inert-enforcement,
  mem-port-contracts, forged-scope-fields, scope-literal-defaults,
  fabricated-scope-defaults, lossy-secret-conversion, unbounded-external-http,
  in-memory-state-stores, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, dead-message-kinds, forwarded-header-trust,
  migration-sql-portability) — all exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — exit 0 (6m24s; also type-checks
  the unconditionally-compiled PG adapter with the new review columns/schema).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — exit 0 (3m04s).
- `cargo test -p gyre-domain` — 515 passed, 0 failed.
- `cargo test -p gyre-adapters` — 518 passed, 0 failed, including the new
  `sqlite::quality_gate::tests::list_by_repo_orders_by_chain_position`,
  `list_by_repo_position_tie_falls_back_to_created_at`,
  `sqlite::review::tests::review_attribution_persists_and_roundtrips`,
  `review_without_attribution_stays_null`, and
  `sqlite::tests::no_duplicate_migration_version_prefixes` — the attribution
  round-trips run through real SQLite rows created by the new migration, so the
  migration applies cleanly on fresh databases.
- `bash scripts/check-task-commit-attribution.sh` at the candidate commit
  (verified in a detached worktree) — exit 0. (It fails only against the
  uncommitted working-tree reset of `specs/tasks/task-135.md` staged by the
  controller for this round, which is not part of the candidate.)

## Verifier-required probes (gyre-server test compilation exceeds the 4 GiB
sandbox budget; the executor's build wrapper rejects it during review)

- `cargo test -p gyre-server gate`
- `cargo test -p gyre-server merge`
- `cargo test --all`

## Acceptance criteria vs. code

- **Ordered chains of mixed gate types** — `quality_gates.position` (task-168
  column, now the chain order), adapters `ORDER BY position, created_at`
  (SQLite + PG), mem mirror; `PUT/GET /repos/:id/gate-chain` parses/emits the
  spec's exact `[[gates]]` TOML (PascalCase + snake_case type names, `command`,
  `persona`, `min_approvals`, `required`, `timeout_secs`, `validation_type`).
  Replacement is validation-before-mutation (invalid chain → 400, chain
  untouched — tested), same-named gates keep ids, absent pre-merge gates
  deleted, post-merge gates preserved (name collisions rejected — tested).
  `update_gate` renames that collide with a sibling are now 409 (test guards
  the by-name replacement invariant the chain PUT relies on).
- **Parallel execution** — `trigger_gates_for_mr` (merge_queue.rs:133 on
  enqueue, idempotency-guarded) creates one Pending GateResult per gate and
  `tokio::spawn`s `run_gate` per gate — genuinely concurrent, no sequencing.
- **Per-MR status Pending → Running → Passed/Failed** — run_gate marks Running
  then persists the terminal status through the GateResultRepository.
- **All required gates must pass** — `check_gates_for_mr` two-pass
  `decide_gate_readiness`: required-gate failures scanned across ALL results
  before any wait decision (order-independent; a Running required gate can no
  longer mask a Failed one — regression tests pin both orderings), required
  Pending/Running → `Ok(false)` requeue, all passed → `Ok(true)`. Merge
  processor calls it per cycle in candidate selection (merge_processor.rs:1814)
  and fails the queue entry on `Err`.
- **Failed required gate blocks + notifies author** — merge processor marks the
  entry Failed with the reason; gate_executor emits a `GateFailure` workspace
  event and calls `notify_gate_failure` (author's spawning user, MR deep link).
- **Optional gates don't block** — advisory gates never block in any state
  (including RequiredApprovals gates short of their minimum — F1 repair,
  tested end-to-end); advisory failures attach a `gate-agent:{id}`-attributed
  MR comment (tested; required failures attach none — tested).
- **RequiredApprovals is real** — `run_required_approvals_gate` counts distinct
  approving reviewers live (Running until N distinct, promotion persisted by
  the processor's re-check — tested including the duplicate-reviewer case);
  replaces the previous hollow "delegated to merge processor" stub that had no
  delegate.
- **Gate agent lifecycle** — ephemeral (scoped EdDSA token registered in
  `agent_tokens`, revoked via `revoke_gate_token` on verdict, exit, crash, and
  timeout paths); read-only (git_http.rs rejects receive-pack for
  `review:submit`-scoped tokens; ABAC middleware route allow-list, exact-match
  scope); identity-scoped (`mint_scoped` carries only `review:submit`);
  attributed (`Review.reviewer_persona` + `Review.gate_id` columns in both
  adapters + mem, derived server-side in `submit_review` from the token's
  `task_id` claim — the HTTP test proves a forged body `reviewer_agent_id` is
  overridden by the token subject and the persona/gate attribution cannot be
  supplied by the body).
- **Context windows captured for audit** — `capture_gate_agent_context` stores
  the exact pre-run bundle (persona system prompt, MR diff, pinned-SHA spec
  content, MR title, task description) zstd-compressed in the conversation
  repository, referenced by a `gate_agent_context` audit event; the test
  retrieves the blob through the real store and asserts the pinned (not tip)
  spec content, and that capture is skipped — never fabricated — when the
  workspace tenant is unresolvable. Validators are captured too.

## Merge-resolution scrutiny (dbcd62f1 vs. task-195 main)

The resolution keeps both lines of evolution: `RequiredApprovals` keeps the
live counting (main's delegation stub would have re-hollowed it);
`AgentReview` takes main's constraints-aware signature so manifest-declared
gates still enforce persona/attestation constraints; `check_gates_for_mr`
keeps the order-independent decision (materialized manifest gates are unknown
to `quality_gates` and correctly default to required); `submit_review` keeps
task-195's reviewer identity binding and adds task-135's token-derived
attribution. No dead duplicate dispatch arms; `cargo check --tests` confirms
all call sites use the merged signatures.

## Non-blocking observations (for the verifier/auditor, no behavior impact)

1. `specs/coverage/system/agent-gates.md` rows 6–7 cite migration
   `2026-11-12-000061` for the `position` and attribution columns. The actual
   migrations are `2026-10-08-000062_gate_position` (task-168, pre-existing on
   main) and `2026-11-12-000062_gate_chain_config` (this task);
   `2026-11-12-000061` is `repo_access_overrides` (unrelated). The citation
   went stale when the migration was renumbered in a rebase round. The
   implementation itself is real and correctly attributed in the migration's
   own header comment; this is a one-number doc drift in the coverage note,
   not a hollow section.
2. An advisory (`required = false`) gate failure still triggers the
   pre-existing `GateFailure` notification/event path in addition to the new
   advisory MR comment. This is unchanged from base (not introduced by this
   candidate), does not block merging, and the spec only requires optional
   results to be recorded without blocking — recorded here for completeness.
