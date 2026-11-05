# Review — task-134 (AgentReview / AgentValidation gate types with review protocol)

Spec: `agent-gates.md` §Part 1 — "Gate Types (Extended)", "AgentReview Gate".
Base `2f215b1e`, candidate `ca5cfa2b` (frontmatter records 16 commits,
`1b40aacb`…`28ada907`, recovered across interrupted runs; TTL repair
`72c5bb03`).

## Round 1 — approved

Verified working (no blocking findings):

- **Gate types / config model**: `AgentReview`/`AgentValidation` variants
  pre-existed in `gyre-common::GateType`; this task adds the `validation_type`
  column (`QualityGate`, migration `2026-10-09-000056`, schema + SQLite and
  Postgres adapters incl. upsert `.set()` coverage with a column-drift
  regression test). Gate CRUD accepts/returns both types and
  `validation_type` with an API round-trip test.
- **Spawn protocol** (`gate_executor.rs`): persona resolved nearest-wins
  (repo → workspace → tenant; unresolvable workspace skips tenant scope
  rather than fabricating "default"); unresolvable persona or spec_ref fails
  the gate before any process spawn or token mint. MR context is gathered
  for real: full diff via `git_ops.diff`, spec content at the pinned SHA via
  the new `read_file_at_commit` port (unknown SHA is an error, not a silent
  None — fail-closed per the ref-resolution invariant), MR title, and the
  task description (acceptance-criteria carrier) via MR → author agent →
  task. No-command gates fail closed (the old stub fabricated an approval).
- **Scoped token**: `mint_scoped` mints `review:submit` (reviewers) or
  `validation:report` (validators — no review-submission capability). TTL
  derived from `gate.timeout_secs + 60` on both mint sites (repair
  `72c5bb03`, with regression tests asserting `exp − iat` covers a
  configured `timeout_secs=700` window). Enforcement is layered and
  exact-match: `git_receive_pack` denies push for both scopes before repo
  resolution; the ABAC middleware allow-lists each capability's routes only
  (403 outside; validators read-only MR context); `submit_review` binds the
  reviewer identity to the token subject (forged `reviewer_agent_id`
  ignored); revocation via `kv_remove` rejects with 401.
- **Verdict mapping**: only the gate's own agent's reviews count; Approved →
  Passed, ChangesRequested → Failed with the review body surfaced in gate
  output; exit-0-without-review fails. Teardown: token revoked and temp
  spec/diff files removed on every exit path (verdict, timeout, spawn
  failure) — asserted by tests.
- **Tests**: real-process and real-protocol throughout — `printenv` probes
  capture the actual minted JWT and validate it against the server signing
  key; two e2e tests bind a real axum server + real bare git repo with the
  spec pinned at a pre-tip SHA and drive the fixture
  `review_agent_driver.sh` through the live Review API (forged reviewer id
  included); `task134_review_probe.rs` (7 tests) covers identity binding,
  allow-list denial, push denial, revocation, validator review-denial +
  MR-context read, and lookalike-scope exact-match discrimination — all via
  oneshot router calls. No self-confirming/assertionless/mirrored-logic
  patterns found in static review (the automated test-quality scripts cannot
  run under this sandbox's mawk — see evidence file).

Independent probes: 22 mechanical check scripts pass on the candidate;
`check-unnamed-tuple-carriers.sh` fails with 3 errors verified identical at
base (pre-existing, untouched by this diff); exemption files unchanged in
count. Rust test execution was blocked by the review sandbox's transport
restriction (crates.io refused while npm registry reachable, `PI_OFFLINE=1`,
no vendored sources or populated cache) — deferred commands for the
verification stage are recorded in
`/tmp/stage/review-evidence/task-134-review-evidence.md`. Per the review
assignment, a transport restriction is not a code defect.

Non-blocking observations (no reproducible failure):

- [minor] Migration `2026-10-09-000056_gate_validation_type` reuses 6-digit
  sequence `000056` already used by base's
  `2026-10-08-000056_spec_assertion_results`; the convention says use the
  next unused sequence (`000057`). Verified harmless: the derived Diesel
  versions (`20261008000056` vs `20261009000056`) are distinct, so both
  migrations run in order and `check-migration-versions.sh` passes — the
  invariant's failure mode (silently dropped duplicate) is absent. Future
  migrations should take the next globally unused sequence.
- [note] The scoped-token allow-list lives inside `abac_middleware` after
  resolver mapping, so resolver-exempt/unregistered routes (frozen baseline)
  remain reachable by scoped tokens exactly as by any agent token;
  per-handler authorization still applies. Pre-existing condition, not
  introduced or widened here.
- [note] `check_review_verdict` reads the review list after process exit; an
  agent that submits its verdict asynchronously and exits before the POST
  completes would be recorded as "exited without submitting a review"
  (fail-closed false failure). The specced protocol (submit, then exit) and
  the driver are synchronous.
