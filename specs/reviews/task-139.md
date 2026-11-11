# Review — task-139: Implement forge enforcement policies for spec-to-code binding at merge

Round: 1
Candidate: `091b1f683feffd0e02fd25e1b83752166cc1e951`
Base: `cba416b46a73d3568f745d7b4eae49a0700a90a6`
Reviewer scope: full diff base→candidate (21 files, +1245/−34). Evidence
persisted at `/tmp/stage/review-evidence/` (findings.md + per-check logs).

## Verdict: NOT APPROVED — 1 blocking defect, 1 in-scope integration gap, 1 minor

The merge-time enforcement core is real and well tested. The gate-integration
half is not: the new pinned-spec delivery channel is dead on arrival for its
only intended consumer.

### What checked out (independently probed)

- **Five policies configurable per repo.** `SpecPolicy.require_signed_approval`
  added to the domain type, both Diesel adapters, portable migration
  `2026-11-09-000059_require_signed_approval` (INTEGER NOT NULL DEFAULT 0 —
  runs on SQLite and PG), mem store stores the struct. Focused test run:
  `cargo test -p gyre-adapters spec_policy` →
  `set_and_get_round_trips_all_five_policies ... ok` on a fresh SQLite DB
  (proves the migration ran and the upsert update path persists).
  `GET/PUT /api/v1/repos/:id/spec-policy` round-trips all five fields
  (Admin-only PUT preserved); RepoSettings web form now binds the real API
  field names (the old form bound `require_approval`/`stale_spec_warning`,
  which the API never returns — silently dead toggles; fixed and covered by
  `binds toggles to the API policy field names`, vitest 58/58 passed).
- **Merge-time enforcement** (`merge_processor.rs:1936-2035`): every blocking
  policy runs before the merge executes against real state — ledger query via
  `verify_spec_ref`/`verify_signed_spec_ref` (exact (path, sha) rows, active
  approvals only, non-empty signature for the signed policy, whitespace-only
  treated as unsigned), stale comparison via `resolve_spec_blob_sha` against
  the real repo. Violations fail the queue entry with the specific reason,
  emit a `GateFailure` workspace event, and notify the author's spawning user
  (`notify_spec_policy_violation`, tenant resolved from the workspace record,
  skipped+logged when unresolvable — no fabricated scope). `warn_stale_spec`
  adds a deduplicated advisory MR comment (`forge:spec-policy` author) and
  does not block. Tests use real git repos and real ledger rows, with
  signed/unsigned controls and merge-must-proceed assertions.
- **Mechanical invariants:** all 24 runnable check-*.sh scripts pass at the
  candidate (arch, abac-route-registry, migration-versions, migration
  portability, mem-port-contracts, mcp-write-tools, byte-slice, dead-message-
  kinds, relative-path, fail-open-refs, fabricated-scope, lossy-secret,
  scope-literal, inert-enforcement, forwarded-header, in-memory-state,
  unbounded-http, forged-scope, task-commit-attribution, …). Four scripts
  (assertionless-tests, crypto-verify, event-emission-coverage,
  early-return-side-effects) exit 2 on mawk syntax errors — sandbox has mawk
  1.3.4, scripts need gawk; environment limitation, not a candidate failure.
  `cargo check -p gyre-server` exit 0.

### F1 (blocking) — GYRE_SPEC_URL is unfetchable by the gate agents it is minted for

`gate_executor.rs` sets `GYRE_SPEC_URL = {base}/api/v1/merge-requests/{id}/spec`
for both review and validation gates, and the gate process receives only its
scoped token (`review:submit` / `validation:report`).
`docs/agent-protocol.md` documents fetching this URL as the spec-alignment
protocol. But `REVIEWER_ALLOWED_ROUTES` / `VALIDATOR_ALLOWED_ROUTES`
(`abac_middleware.rs:53-64`) do not include `/api/v1/merge-requests/:id/spec`,
and the scoped-token gate (`abac_middleware.rs:1126-1165`) denies any
non-allow-listed pattern **before** policy evaluation (403). The route is
resolver-mapped (candidate added the mapping at `:250`), so it reaches the
deny rather than falling through. Any gate agent following the documented
protocol gets 403 — a dead delivery link for the exact consumer it was built
for, and the repo's own standard treats dead spec delivery links as defects.

No test catches it: `get_mr_spec_serves_pinned_sha_not_head` builds
`api_router()` bare, and the auth/ABAC layers are applied only in
`lib.rs:791-800` — the endpoint's authorization path is never exercised by
any test.

Fix: add the route to both allow-lists (e.g. a new `MR_SPEC_ROUTE` const
beside `MR_DIFF_ROUTE`) and add a scoped-token test (minted `review:submit`
token → 200 on `/spec`, 403 on an unrelated route; same for
`validation:report`).

### F2 — spec-alignment gate still cannot receive the spec for production-shaped spec_refs

Pre-existing at base, but squarely inside the coverage section this task
claims closed ("§Integration with Agent Gates", shipped notes: "the
spec-alignment gate receives the spec at the pinned SHA"). Production MR
spec_refs carry the ledger **blob** sha and ledger path
(`api/spawn.rs:2005` uses `spec_ledger.current_sha`; `spec_registry.rs`
computes it from the HEAD blob; `api/specs.rs:135`: "The git blob SHA being
approved"). The gate's primary spec channel
(`gate_executor.rs:549-567` → `git2_ops.rs:641-671` `read_file_at_commit`)
does `repo.find_commit(oid)` — a commit lookup that errors on blob OIDs — and
a full-tree path lookup, so a production-shaped `system/foo.md@<blob-sha>`
fails context gathering: the gate fails (fail-closed) without ever receiving
the spec. The candidate added a parallel channel with the correct semantics
(`/spec` → `git cat-file blob`) rather than fixing the primary one — and that
channel is 403-blocked for gate agents (F1). The gate tests only exercise
commit-SHA spec_refs (`specs/system/widget.md@<commit-sha>`), so the
production shape is untested. Net: the AC "spec-alignment gate receives spec
at pinned SHA, not HEAD" is not achieved end-to-end for real MRs.

Suggested fix direction: route the gate's spec delivery through the
blob-semantics resolver (the new `read_blob`) and/or the now-allow-listed
`/spec` endpoint, and add a gate test with a ledger-shaped spec_ref
(ledger path + blob sha).

### F3 (minor) — truncation pattern spread

`notifications.rs:263` (`notify_spec_policy_violation`) adds a 5th instance of
`mr_id[..8.min(mr_id.len())]`, mirroring 4 pre-existing sites in the same
file. Not flagged by `check-byte-slice-truncation.sh` (index is an
expression), reachable only when the MR lookup fails, ASCII ids in practice —
but the check's header says the pattern should not spread; new code should
truncate on a char boundary.

## Verifier must run (gyre-server test compilation was out of budget here)

    cargo test -p gyre-server require_signed_approval_blocks_unsigned_spec_approval
    cargo test -p gyre-server warn_stale_spec_adds_advisory_comment_without_blocking
    cargo test -p gyre-server signed_spec_ref_requires_signature_on_active_approval
    cargo test -p gyre-server get_mr_spec_serves_pinned_sha_not_head
    cargo test -p gyre-server set_require_signed_approval_round_trips
    cargo test -p gyre-server spec_
    cargo test --all

Plus an F1 probe: layered router (or live server), `review:submit` scoped
token, `GET /api/v1/merge-requests/{id}/spec` → expect 403 today.
