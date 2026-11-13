# Review — task-139 (Forge enforcement policies for spec-to-code binding at merge)

Spec: `specs/system/agent-gates.md` §Forge Enforcement Policies + §Integration with Agent Gates.
Candidate: `219a0812` (base `a8bd7d09`). Verdict: **approved** — no blocking findings.

## Candidate shape

`219a0812` = the previously published, CI-green task-139 head `2f6a1d0f` with current
main (`a8bd7d09`, task-179 web-only changes) merged in as `af9ea6c1`. Verified:
`git diff 2f6a1d0f 219a0812 -- crates/` is **empty** — every Rust source in the
candidate is byte-identical to the head that passed the full CI suite (Test, Clippy,
Format, Architecture lint, per the durable check summary). The merge delta beyond
task-139's own work is main's `web/src` + `web/dist` (generated, verifier-owned).
The DIRTY merge state from the prior round is therefore resolved with zero drift on
task-139's production surface.

Task-139's delta vs base closes the two assigned coverage sections end-to-end:
all five policies (the fifth, `require_signed_approval`, lands in this round:
domain field, portable migration `2026-11-09-000059`, SQLite+Postgres adapters,
`GET/PUT /api/v1/repos/:id/spec-policy` round-trip, UI toggle, merge-queue
enforcement), author notification on blocking violations (`GateFailure` event +
inbox notification), `warn_stale_spec` advisory MR comment, `GET
/api/v1/merge-requests/:id/spec` serving the spec blob at the **pinned** SHA,
`GYRE_SPEC_URL` delivery to review/validation gate agents with the MR-spec route
added to both scoped-token allow-lists and the ABAC resolver, and blob-pinned
spec_ref resolution in `GitOpsPort::read_file_at_commit`.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters --lib spec_policy` → **PASS** (fresh SQLite DB proves
  migration 000059 runs; all five flags round-trip; upsert flip persists).
- `cargo test -p gyre-adapters --lib test_read_file_at_commit` → **4/4 PASS**
  (blob-pinned SHA resolves to the pinned blob's content, not the branch tip;
  commit+path shape and fail-closed unknown-SHA error both covered).
- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --tests` →
  **PASS, clean** (server test *execution* is blocked by the sandbox build wrapper;
  see verifier list below).
- `cd web && npm test -- --run src/__tests__/RepoSettings.test.js` → **64/64 PASS**
  (node_modules installed by build.rs `npm ci`). Includes the new regression tests
  proving the toggles bind the real API field names (`require_approved_spec`,
  `warn_stale_spec`, `require_current_spec`, `require_signed_approval`) and that
  saving round-trips the full policy object.
- Mechanical checks all exit 0: check-arch, check-migration-versions,
  check-migration-sql-portability, check-abac-route-registry,
  check-abac-exempt-handlers, check-mcp-write-tools, check-mem-port-contracts,
  check-inert-enforcement, check-dead-message-kinds, check-forwarded-header-trust,
  check-in-memory-state-stores, check-lossy-secret-conversion,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-unbounded-external-http, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution.

## Behavior verification (code-reading where the wrapper blocked execution)

- **Merge-time enforcement order** (`merge_processor::process_next`): blocking
  policies (`require_spec_ref`, `require_approved_spec`,
  `require_signed_approval` — the last implies a spec_ref must exist) run before
  the merge; a violation fails the queue entry with the specific reason, emits a
  `GateFailure` workspace event, and notifies the author's spawning user
  (workspace lookup failure skips the inbox message rather than fabricating a
  scope — no notification, event still delivered). `warn_stale_spec` records a
  deduplicated advisory MR comment (`forge:spec-policy` author) plus the
  `StaleSpecWarning` event and does not block. Staleness is computed with real
  `git rev-parse HEAD:<path>` against the repo on disk (with the `specs/`-prefix
  fallback for ledger paths), compared to the pinned blob SHA.
- **`verify_signed_spec_ref`** (`api/gates.rs`): exact `(spec_path, spec_sha)`
  ledger query (same shape as `verify_spec_ref`); satisfies only on an active
  (`approved_at` set, not revoked/rejected) approval carrying a non-blank
  signature; distinct error messages for "active but unsigned" vs "no active
  approval". The mem and SQLite ledger adapters implement the same
  exact-version filter semantics.
- **Pinned-spec delivery** (`gate_executor`): `build_review_agent_context`
  resolves the spec at the pinned SHA via the git port (fail-closed on an
  unresolvable pin — context error, never an empty spec), and both the review and
  validation agent processes receive `GYRE_SPEC_URL` pointing at
  `GET /api/v1/merge-requests/:id/spec`, which serves `git cat-file blob <sha>`
  content (SHA validated 40-hex before reaching the command line). The route is
  registered in the ABAC resolver and added to `REVIEWER_ALLOWED_ROUTES` and
  `VALIDATOR_ALLOWED_ROUTES`, so the scoped tokens the executor mints can
  actually fetch it.
- **Test quality (mutation check):** each new test fails if its behavior is
  disabled — unsigned approval merges if the check is removed; blank signature
  passes if the trim check is dropped; HEAD content served instead of the pinned
  blob trips `assert_ne`; dropping the blob-pin port support fails the gate
  context test; omitting the allow-list entry 403s the integration test; a
  missing advisory comment fails the warn test. The signed-approval merge test
  includes a negative control (signed active approval → `Merged`), proving no
  false blocking. Merge-processor unit tests stub only the port-level
  `merge_branches` (`NoopGitOps`) — policy checks, ledger state, notifications,
  comments, and git SHA resolution are all real.
- **`require_signed_approval` scope:** enforces signature *presence* (non-empty),
  which is exactly the task's acceptance criterion ("rejection when approval
  lacks signature") and the spec table's "must include Sigstore signature".
  Cryptographic Sigstore verification is not part of this task's contract.

## Verifier must run (blocked here by the 4 GiB build wrapper)

- `cargo test --all` (task acceptance criterion).
- Focused: `cargo test -p gyre-server --lib merge_processor::`,
  `cargo test -p gyre-server --lib gates::`,
  `cargo test -p gyre-server --lib merge_requests::`,
  `cargo test -p gyre-server --lib gate_executor::`,
  `cargo test -p gyre-server --test task134_review_probe`.
- Full web suite: `cd web && npm test`.
- Standard CI gates (fmt, clippy, arch lint) — all Rust sources are
  byte-identical to CI-green `2f6a1d0f`, so these are expected green.

## Non-blocking observations

1. `specs/coverage/system/agent-gates.md` rows 22/24 still read `task-assigned`.
   The coverage file is only ever modified by `audit(coverage)` controller
   commits, never by task branches (including this task's previously published
   rounds); the matrix transition is owned by the audit stage. Not an
   implementation defect.
2. `check-task-commit-attribution.sh` fails against the *working tree* only
   because the controller reset `specs/tasks/task-139.md` to `commits: []` for
   this round (the known frontmatter-clobber artifact). The candidate's
   committed frontmatter lists all eight task-labeled commits
   (`533795f2`, `d08359f1`, `87e2aed4`, `c92e8f53`, `33cc4941`, `ec05bf9a`,
   `059586a3`, `99edd0bc`) — the four SHAs the reset-state run flags are all
   present in the committed list.
3. Pre-existing base behavior (unchanged by this diff, noted for the record):
   `require_current_spec`/`warn_stale_spec` treat a spec file unresolvable at
   HEAD (deleted) as non-stale.
4. The new integration test lives in `tests/task134_review_probe.rs` (file
   predates task-139; naming drift only, content is task-139-scoped).
5. `web/dist` in the merge is resolved to main's bundle — generated output owned
   by the deterministic verifier; the authored frontend source and tests are
   what was reviewed here.

Verdict: **approved** — independent evidence supports every acceptance criterion;
no structural defects, missing enforcement, scope handling, fake implementations,
or test inflation found in the candidate delta.
