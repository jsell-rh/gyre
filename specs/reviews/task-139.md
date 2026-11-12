# Review — task-139 (agent-gates §Forge Enforcement Policies + §Integration with Agent Gates)

Candidate: `00b6c2d82f22acaadc40531fa1fa1506bb5d4228` (base `d31906c5`). This round
reviews the DIRTY-merge resolution of the previously published PR head `7d74ce9`
against current main, per the durable rebase finding. Verdict: **complete**.

## Merge-resolution integrity (the assigned risk)

- `git diff 7d74ce9..00b6c2d8 -- crates/` is **empty** — the retained Rust
  implementation is byte-identical to the previously verified PR head.
- `git diff 7d74ce9..d31906c5 -- crates/` is exactly the reverse of the
  task-139 delta, i.e. main introduced no `crates/` changes in the overlap;
  the merge (`ae76f2763`) could not silently drop a main-side change.
- Residual diffs vs the PR head are main's task-181 Risk Map `web/src` sources
  (taken wholesale from main), generated `web/dist` (verifier-owned), and
  task/docs bookkeeping. No source conflicts remain.

## Acceptance criteria vs implementation

| Criterion | Evidence |
|---|---|
| 5 policies configurable per repo | `SpecPolicy` carries all five flags (`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`, `require_current_spec`, `require_signed_approval`); SQLite + Postgres adapters persist the new column via portable migration `2026-11-09-000059_require_signed_approval` (`INTEGER NOT NULL DEFAULT 0`, both dialects); mem adapter stores the struct; `GET/PUT /api/v1/repos/:id/spec-policy` round-trips all five (PUT Admin-only); web `RepoSettings` binds the real API field names (the old `require_approval`/`stale_spec_warning` dead bindings were fixed). |
| `spec_ref` on MR (path@sha) | `MergeRequest.spec_ref` + `2026-10-10-000057_task_spec_ref` migration; parsed as `path@sha` with 40-hex SHA validation at every consumer. |
| `require_spec_ref` | `merge_processor.rs` blocks when `needs_spec_ref && mr.spec_ref.is_none()`; the check now also covers `require_signed_approval` (an MR with no spec_ref has no approval to sign). |
| `require_approved_spec` | `gates::verify_spec_ref` — real ledger query (`spec_approvals` filtered on exact `(spec_path, spec_sha)`, `is_active()` = approved ∧ not revoked ∧ not rejected). Fail-closed: ledger read error yields no active approval → blocked. |
| `warn_stale_spec` | `resolve_spec_blob_sha` (ledger path → `specs/`-prefixed tree path, real `git rev-parse`) compared against the pinned SHA; mismatch → deduplicated advisory MR comment (`forge:spec-policy` author) + `StaleSpecWarning` workspace event; merge proceeds. |
| `require_current_spec` | Same comparison, blocking: queue entry Failed with a reason naming both SHAs. |
| `require_signed_approval` | `gates::verify_signed_spec_ref` — same per-SHA ledger lookup; the active approval must carry a non-empty (whitespace-trimmed) signature; one signed active approval among several satisfies; distinct error messages for unsigned-approval vs no-approval. |
| Errors reported to author | `report_spec_policy_violation`: `GateFailure` workspace event + `notifications::notify_spec_policy_violation` to the author agent's spawning user (tenant resolved from the workspace record; skipped-and-logged when unresolvable — no fabricated scope). Failure reason lands on the queue entry. |
| Spec-alignment gate at pinned SHA | Three channels: (1) `build_review_agent_context` resolves the pinned SHA through `GitOpsPort::read_file_at_commit`, fail-closed on unresolvable pins, and the port now resolves the **production blob-pin shape** (ledger path + blob OID — `find_commit` fails → `find_blob` is the content), which the commit-only lookup would have rejected; (2) `GET /api/v1/merge-requests/:id/spec` serves the pinned blob via `git cat-file` (SHA validated 40-hex before reaching the command line); (3) `GYRE_SPEC_URL` env var for AgentReview/AgentValidation gate agents, with `MR_SPEC_ROUTE` added to the reviewer/validator scoped-token allowlists and the ABAC resolver. |
| `cargo test --all` | Not runnable in-model (executor wrapper rejects gyre-server test compilation under the 4 GiB budget). `cargo check -p gyre-server` clean on the merged tree; focused suites below all pass; crates byte-identical to the previously verified head. Verifier must run the recorded server test list (see `/tmp/stage/review-evidence/probes.txt`). |

## Probes run (full log: /tmp/stage/review-evidence/probes.txt)

- `cargo check -p gyre-server` → clean, exit 0 (7m52s).
- `cargo test -p gyre-adapters --lib sqlite::spec_policy` → 1 passed (all-five-policy round trip on a fresh DB, proving migration 000059 runs).
- `cargo test -p gyre-adapters --lib test_read_file_at_commit` → 4 passed (incl. blob-pinned SHA resolution and unknown-SHA error).
- `cargo test -p gyre-domain` → 466 + 8 passed.
- `web: npm ci && npx vitest run src/__tests__/RepoSettings.test.js` → 58 passed, including the new API-field-name binding regression test.
- Mechanical gates: check-migration-versions, check-abac-route-registry, check-arch, check-inert-enforcement, check-lossy-secret-conversion, check-fail-open-ref-resolution, check-scope-literal-defaults, check-unbounded-external-http → all OK.

## Test quality

The new server tests are discriminating, not self-confirming: the unsigned-approval
test asserts the exact terminal status `Failed` (not mere queue absence), the
specific failure reason (`no Sigstore signature`), and the author notification
content, then a signed control that must reach `Merged`; the warn-stale test uses a
real on-disk git repo with two spec versions and asserts the merge proceeded AND
the advisory comment names the drift; the pinned-spec endpoint test asserts the v1
blob content is served while HEAD holds v2, plus 404s for no-spec_ref and
unreachable pins; the gate-context test drives the real `Git2OpsAdapter` against a
real bare repo with the production ledger-path + blob-OID pin shape.

## Observations (non-blocking)

- Pre-existing base behavior (unchanged by this candidate): when the spec path
  cannot be resolved at HEAD at all, `require_current_spec`/`warn_stale_spec`
  treat the MR as non-stale (comment: "new/empty repo"). A spec *deleted* from
  HEAD therefore does not trip `require_current_spec`. This predates the diff
  under review; if considered a gap it belongs to a follow-up, not this task.
- `read_blob`/`resolve_blob_sha` shell out to `git` per call (consistent with the
  existing `git_refs.rs` pattern); SHA/path validation prevents injection.
- `String::from_utf8_lossy` on spec blob content in `read_blob` is fine — spec
  text, not secret material (check-lossy-secret-conversion passes).

Verdict: approved. The five forge enforcement policies are real, per-repo
configurable, enforced at merge against the real approval ledger and real git
state, violations reach the author, and the spec-alignment gate receives the spec
at the pinned SHA through three independent channels.
