# Review — task-166 (supply-chain §AIBOM Contents, §SLSA Provenance Integration, §The Laptop Problem)

Candidate `a00bc270fc5fcf7916d59719933c9b2f51abeacb` (base `9f446d0eb33ecb24b04c821e47007d8884882b63`).
Product-surface commits: `39a0aaec` (feature), `da143273` (review repairs), `edfb0ac8` (rustfmt),
`e7d8c215` (verification repair: nested-tree test helpers). Bookkeeping commits (`chore(task-166)`,
`process: record …`) touch only `specs/tasks/task-166.md` — correctly outside `commits:`.

Verdict: **approved**. All three coverage sections are real production implementations with
strong, discriminating tests. The prior verification round's 6 failing tests have a verified
root cause and repair (see probe below). Server test execution is wrapper-blocked in the review
sandbox and is recorded below for the independent verifier.

## What was verified

### §AIBOM Contents — `?format=release` (aibom.rs `build_release_aibom`)

- The release commit set is a real git range (`rev-parse --verify --end-of-options` + `rev-list`),
  so `human_commits` is counted from actual git history, not the agent-commit table. `from`/`to`
  are validated by `is_valid_ref` (no leading dash, no `--`, no metacharacters) before use; the
  range is a single argv element that cannot occupy an option position. Unresolvable ref → 404;
  unknown `format` → 400.
- `agents[]` attribution is assembled from real stores: registered stack (KV `agent_stacks`),
  per-agent stored level (`agents.find_by_id`, falling back to strongest commit label),
  `verified` = registered fingerprint == committed `gyre-stack.lock` fingerprint (parsed from the
  release tip via `git show`), model, MCP servers, `spawned_by`, `task_ids`, Rekor UUIDs via
  `commit_signatures.find`. `stack_policy.policy_violations` = the repo's `stack_drift_detected`
  audit events (real audit-repository query, both SQLite and mem adapters order newest-first —
  verified in adapters/sqlite/audit.rs:147 and mem.rs sort).
- Tests drive the real router against a real bare git repo (3-commit topology incl. nested
  `src/lib.rs` tree, lockfile, Rekor entry, policy, drift event) and assert every artifact block,
  plus range semantics (`from` bounds the set), 400 on bad format, 404 on unresolvable ref.

### §SLSA Provenance Integration — `GET /repos/:id/slsa-provenance` (provenance.rs)

- Emits an in-toto Statement v1 envelope (`_type`, `subject[{name, digest}]`, `predicateType
  https://slsa.dev/provenance/v1`) with the SLSA v1 predicate structure (`buildDefinition`:
  buildType/externalParameters/internalParameters/resolvedDependencies; `runDetails`:
  builder/metadata/invocationId).
- The level mapping is evaluated against real state, not hardcoded: Gyre 1 → SLSA 0; Gyre 2 →
  SLSA 1, or 2 when the subject ref pins an internally-consistent `gyre-stack.lock` (integrity
  verified via `verify_integrity()`); Gyre 3 → SLSA 3. `slsaBuildLevel` is the weakest link
  across agent-attributed commits reachable from the subject ref (rev-list scoping — records
  targeting SHAs on unrelated/deleted branches are excluded from both the attribution and the
  level; pinned by a test that would fail without the scoping). `?ref=` validated against flag
  injection (400) and unresolvable refs 404.
- Tests: statement shape + level mapping (incl. unreachable-record exclusion), weakest-link cap
  (unattested reachable commit → 0), level-2 → 1/2 lockfile discrimination, ref
  validation/resolution.

### §The Laptop Problem — drift audit trail + required override reason

- `enforce_stack_policy` (git_http.rs) records a `StackDriftEvent` (gyre-domain/stack.rs) as a
  `stack_drift_detected` audit event on **every** drift outcome: block-mode rejection, warn-mode
  rejection for a missing reason, and warn-mode acceptance. Detail carries both fingerprints,
  `integrity_verified`, reason naming differing components, mode, outcome, branch, agent, repo,
  workspace. The drift check is not gated on a repo policy (no policy → warn), matching
  task-165's semantics.
- Warn-mode acceptance now **requires** `X-Gyre-Stack-Override-Reason` — missing/blank → 403 +
  refs undone + blocked drift event with a self-describing error. CLI plumbing: `gyre push
  --stack-override-reason` flattens whitespace (no header CRLF injection) and skips blank values;
  server re-flattens and caps at 2000 chars.
- `GET /repos/:id/stack-drift` is repo-scoped through the audit repository (workspace +
  event-type + resource-type filter, client-side repo match), newest-first, `?limit` default 100
  cap 500 with an honest pre-truncation `total`. SIEM CEF severity 4 added for the event type.
  `AuditEventType` round-trip extended on both `as_str`/`from_str` with a test.
- Tests run the full git-receive-pack HTTP path with real receive-pack bodies:
  no-reason → 403 + blocked event; with-reason → 200 + full recorded context (both fingerprints,
  branch, mode, outcome); block-mode rejection recorded; the task-165 no-policy test updated for
  the reason requirement; endpoint tests cover cross-repo scoping, limit semantics, empty list,
  404.

## Probes (evidence in /tmp/stage/review-evidence)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` — clean, no warnings in changed
  files (build wrapper; server test codegen is verifier-owned).
- `cargo test -p gyre-domain stack` — 9 passed. `cargo test -p gyre-cli --bin gyre push` — 5
  passed (incl. the new `cli_push_stack_override_reason_parses`).
- Mechanical gates on the candidate tree: check-arch, check-abac-route-registry,
  check-mcp-write-tools, check-dead-message-kinds, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution, check-inert-enforcement,
  check-fabricated-scope-defaults, check-scope-literal-defaults, check-lossy-secret-conversion,
  check-in-memory-state-stores (drift state is port-backed via the audit repository, not an
  in-memory store), check-forwarded-header-trust, check-unbounded-external-http,
  check-migration-versions, check-migration-sql-portability, check-forged-scope-fields,
  check-abac-exempt-handlers, check-mem-port-contracts — **all pass**.
  check-task-commit-attribution passes at the committed candidate (all 4 product-surface commits
  listed; the working-tree failure is the controller's assignment-restore of the task file, not a
  candidate defect). check-api-auth, check-event-payload, check-unnamed-tuple-carriers,
  check-conditional-test-guards, check-hierarchy pass. Five heuristic scripts
  (cli-spec-parity, cross-surface-parity, event-emission-coverage, assertionless-tests,
  dead-test-code, commit-msg-misuse) fail **identically on the base commit** (verified via a
  clean base worktree + log diff) — pre-existing, not candidate defects.
- `check-rustfmt-diff.py` — changed lines clean. Clippy: package-scoped `cargo clippy -p
  gyre-server --tests` run and every warning in changed files mapped to changed-line sets —
  all 13 fall on pre-existing lines; the changed-lines gate is clean (the script's own
  workspace-wide invocation is wrapper-blocked in review stage).
- **Verification-repair probe** (git-helper-probe.sh/.log): replicated the exact git command
  sequences of the repaired test helpers against real git — the aibom 3-commit topology (empty
  root, lockfile commit, lockfile + nested `src/lib.rs`), the provenance topology (empty-tree
  init, `-p`-chained commits), `rev-parse --verify --end-of-options`, `rev-list` full/range/
  `--max-parents=0`, `git show <sha>:<path>` reads, and a regression check that single-level
  `mktree` rejects slashed paths (the root cause of the prior round's 6 `commit-tree failed`
  failures, durable finding 5dce4ca4). All checks pass; the recursive-subtree repair
  (`e7d8c215`) is correct.

## Deferred to the independent verifier (wrapper-blocked here)

- `cargo test --all --quiet` (the assigned repair command).
- `cargo test -p gyre-server --lib api::aibom::tests api::provenance::tests api::stack_attest::tests`
  (the 6 repaired tests plus module suites) and the git_http task-166 push tests:
  `warn_mode_drift_without_override_reason_rejected`,
  `warn_mode_drift_with_override_reason_accepted_and_audited`,
  `block_mode_drift_rejection_recorded_in_audit_trail`,
  `lockfile_drift_without_policy_warned_and_accepted`.

## Observations (non-blocking)

- The in-toto subject/`resolvedDependencies` digest uses the key `gitCommit` rather than a
  hash-algorithm name (`sha1`). The statement is unambiguous and documented
  (docs/api-reference.md), and the level mapping — the substance of the section — is evaluated
  against real state; a future conformance pass could add `sha1` alongside `gitCommit`.
- `slsaBuildLevel`'s weakest link spans agent-attributed commits; human (unattributed) commits in
  the subject's history do not cap the level. This is documented, visible in the per-commit
  attribution, and the AIBOM release artifact reports `all_commits_attested=false` for exactly
  that case. Design choice, not fakery.
- `stack-drift` `total` is bounded by the internal 1000-event audit query cap (per workspace);
  beyond 1000 events the count understates. Edge case far beyond the documented 100/500 caps.
- The shipped notes cite evidence at `/tmp/stage/review-evidence/helper-logic-probe.rs` from the
  implementer's round; that file was not retained. This review's own probe
  (`git-helper-probe.sh`) covers the same ground with full output saved.

## Coverage matrix

Rows 14/16/17 moved to `implemented` with detailed, accurate notes; row 16 honestly records that
hermeticity remains a Level-3 runtime property (row 9 partials) and that the endpoint maps the
chain rather than verifying hermeticity. Matrix statuses match the code.
