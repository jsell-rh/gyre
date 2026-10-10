# Review — task-134 (AgentReview and AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Part 1 — §Gate Types (Extended) + §AgentReview Gate
(coverage rows named in `specs/tasks/task-134.md`).
Base `27bd585ca7eb429905ccbded1f48b4d0167c0c20` → candidate
`da6d5afc86fa9d99cee5b0555776b922900e3f0a` (product commits `42d79754`…`1b40aacb`,
recovered across interrupted runs; review-F1/F2/F3 repairs from the round-3 review included).
Verdict: **approved**.

## Round 1 (this review)

All probes run at HEAD `da6d5afc` in the review sandbox with
`CARGO_TARGET_DIR=/tmp/gyre-target`. Sandbox cannot `accept()` on TCP listeners
(errno 95, capabilities.json); every probe below is oneshot/in-process. Full evidence:
`/tmp/stage/review-evidence/task-134-independent-review.md`.

Test runs:

- `cargo test -p gyre-server --lib gate_executor` → **35 passed, 0 failed** (the two
  `agent_review_end_to_end_*` tests skip on loopback-deny with a clear message; the
  always-runnable guard `e2e_review_submission_is_abac_allowed_with_seeded_policies`
  passes, proving the exact e2e setup is ABAC-allowed — review F1 regression guard).
- `cargo test -p gyre-server --test task134_review_probe` → **7 passed, 0 failed**
  (identity binding, allow-list, push 403, revocation 401, F2 validator cannot submit
  review + can read MR context, F3 lookalike scopes).
- `cargo test -p gyre-server --lib api::gates` → **6 passed, 0 failed** (agent_review +
  agent_validation CRUD with validation_type round-trip).
- `cargo test -p gyre-adapters sqlite::quality_gate` → **1 passed** (upsert persists
  `validation_type` AND `required` — the dropped-`.set()`-column drift class);
  `git2_ops::tests::test_read_file_at_commit` → **3 passed** (pinned content, missing
  path → None, unknown SHA → error — no fail-open).
- All frozen-baseline mechanical checks green on candidate (arch, migration versions +
  portability, ABAC registry + exempt handlers + context parity, api-auth, fabricated
  scope defaults, fail-open ref resolution, forged scope fields, forwarded-header trust,
  in-memory state stores, inert enforcement, lossy secret conversion, MCP write tools,
  mem port contracts, relative path defaults, scope literal defaults, commit attribution,
  unbounded external HTTP, byte-slice truncation, dead message kinds, unnamed tuple
  carriers). The four non-green scripts (unwritten-store-fields, mcp-wrapper-parity,
  llm-endpoint-hygiene, dead-components) fail identically on base in an isolated
  worktree — pre-existing, not touched by this delta.

Verified working (no findings):

- **Gate types**: `GateType::AgentReview`/`AgentValidation` in `gyre-common`
  (gate.rs:21-23); `QualityGate.validation_type` (domain) + migration
  `2026-10-09-000056` (next unused sequence, portable `ALTER TABLE`), persisted by
  SQLite and Postgres with an upsert round-trip test guarding the full `.set()` clause.
- **Gate CRUD**: `POST/GET /api/v1/repos/:id/gates` accepts/returns both types plus
  `validation_type` and `persona` (gates.rs:215-217, 231); per-type validation retained.
- **Spawn protocol** (gate_executor.rs): persona resolved nearest-wins
  repo → workspace → tenant with no fabricated tenant scope; unresolvable persona,
  unresolvable `spec_ref@sha`, missing MR/repo, or diff failure fails the gate BEFORE
  any process spawn or token mint (asserted: no `gate-*` token left in the store).
  MR context is real: full diff via `git_ops.diff`, spec content at the pinned SHA via
  the new `read_file_at_commit` port (unknown SHA is an error, absent path is Ok(None)),
  MR title, and task description (MR → author agent → task). Delivered via env vars +
  temp files (GYRE_SPEC_FILE/GYRE_DIFF_FILE), cleaned up on every exit path.
- **Scoped token**: `mint_scoped` mints a real EdDSA JWT (cryptographically validated by
  the auth extractor with issuer + expiry; kv_remove revocation → 401). Reviewer carries
  `review:submit`; validator carries `validation:report` (F2 — validators cannot submit
  reviews). Enforcement exact-match (F3) at three layers: `git_receive_pack` inline
  403 before repo resolution, ABAC middleware route allow-list (denial precedes policy
  evaluation and records a Deny decision; all allow-listed routes are in the resolver
  registry), and `submit_review` binding reviewer identity to the token subject.
- **Verdict mapping**: `check_review_verdict` counts only the gate's own agent's review;
  Approved → Passed, ChangesRequested → Failed with the feedback body surfaced;
  exit-0-without-review fails. `check_gates_for_mr` blocks merge on required-gate
  failure; merge processor consumes it (merge_processor.rs:174, 1002).
- **Teardown**: `revoke_gate_token` (kv_remove) on every exit path — verdict, timeout,
  spawn failure, empty command (gate_executor.rs:626, 685, 893, 924) — asserted by
  `gate_token_revoked_after_review_completes`.
- **Fail-closed semantics**: no-command AgentReview/AgentValidation gates fail rather
  than fabricate approval ("cannot determine state" is not "state is fine").

Prior-round findings from the durable review record — all repaired and regression-guarded:

- **F1**: both e2e tests now call `seed_builtin_policies` (gate_executor.rs:2131, 2216),
  and `e2e_review_submission_is_abac_allowed_with_seeded_policies` (gate_executor.rs:2251)
  pins the regression without needing loopback (passes here, 201 + identity-bound review).
- **F2**: validators mint `validation:report`, which the ABAC allow-list excludes from
  the review route and from comment writes; `validation_scoped_token_cannot_submit_review`
  proves no review is persisted.
- **F3**: all three enforcement sites use exact `==` comparison against the scope
  constants; `lookalike_scope_does_not_inherit_review_capability` discriminates by
  observing the review stored under the body's reviewer id for `review:submit:admin`
  and `xreview:submit`.

Non-blocking notes (recorded, not findings):

- Implementation-plan item 3's "spawn via existing spawn infrastructure with
  `agent_type = 'review'`" is realized as a gate.command subprocess with a scoped JWT
  and MR-context env vars rather than an agents-table row; the canonical spec text
  specifies behavior (scoped read-only reviewer, verdict via Review API, teardown),
  which is genuinely enforced, and a no-command gate fails closed instead of faking
  approval. Spec-faithful realization; no spec amendment needed.
- Teardown is JWT revocation + temp-file cleanup rather than an `/agents/:id/complete`
  call (gate agents are not agents-table rows); "torn down after verdict" (§AgentReview
  Gate step 5) is satisfied by revocation on every exit path, asserted by tests.
- The two e2e tests and `git_clone_empty_repo_via_smart_http` require loopback accept();
  they skip/fail in this sandbox for the documented errno-95 restriction and must run on
  host/GitHub CI (`cargo test --all` is the acceptance criterion and is owned by
  verification). `e2e_review_submission_is_abac_allowed_with_seeded_policies` covers the
  F1 regression locally. The git_http clone failure is pre-existing on base.
- `check-unwritten-store-fields` fails on the candidate at mem.rs:3578 — identical
  pre-existing failure on base (mem.rs:3560; the `mem.rs:3504` exemption entry is stale
  line drift from before the base commit). The candidate's +18-line mem.rs insertion
  (two `read_file_at_commit` impls for the test adapters) moved the line number only.
  Base-anchored, not introduced here; flagging so the drift gets re-anchored by the
  owning task rather than silently blocking.

Conclusion: every acceptance criterion in `specs/tasks/task-134.md` is implemented with
real, enforced production behavior; prior findings F1/F2/F3 are repaired with
regression guards; focused suites green. Approved.
