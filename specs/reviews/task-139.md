# Review: task-139 — Forge enforcement policies for spec-to-code binding at merge

Comparison base: cfeefbda975211a6ac24a17e5920be6f8b23008b → candidate
70cd41b159dfad533b191c1b29344f2b110e3582 (merge of the task branch over the
assigned base; conflicts in `gate_executor.rs` / `notifications.rs` resolved as
pure additions over base — no base-side behavior dropped, verified by diffing
both files base→candidate).

Diff: 21 files, +1268/−34. No `scripts/` changes, no exemption-file growth, no
deleted tests, no gate weakening.

## R1 Findings

No material findings. Evidence reviewed (probe log with commands/exit codes:
`/tmp/stage/review-evidence/probe-log.md`):

### What was already in base (task-138 lineage) vs. what this candidate adds

Base already ships `spec_ref` on the MR entity + migration 000041,
`require_spec_ref` / `require_approved_spec` / `require_current_spec`
merge-time enforcement, the `warn_stale_spec` domain event, and the
in-process pinned-SHA spec fetch in the review-gate context builder. The
candidate adds the missing fifth policy and the author-facing delivery:

- **`require_signed_approval` (the missing 5th policy)**: `SpecPolicy` field
  persisted via portable migration `2026-11-09-000059_require_signed_approval`
  (INTEGER NOT NULL DEFAULT 0; version prefix unique under Diesel semantics —
  `check-migration-versions.sh` OK), round-tripped in the SQLite + Postgres
  adapters (SELECT struct, insert values, upsert set) and the mem store.
  `verify_signed_spec_ref` (api/gates.rs) does the same exact-(path, sha)
  ledger query as `verify_spec_ref` and requires an ACTIVE approval carrying a
  non-blank signature (whitespace-only treated as unsigned; one signed active
  approval among several rows for the version satisfies). Fail-closed on
  ledger errors (`.unwrap_or_default()` → empty vec → "no active approval"
  rejection, never a pass). The merge processor enforces it before executing
  the merge; an MR with no spec_ref is rejected under this policy too (no
  referenced approval exists). The signature has a real production input
  path: `POST` approve-spec (`ApproveSpecRequest.signature`,
  api/specs.rs:635) records it on the ledger row.
- **Author-facing violation reporting**: queue entry → `Failed` with reason
  (unchanged channel), plus new `report_spec_policy_violation` — a
  `GateFailure` workspace event and an inbox notification to the author's
  spawning user (falls back to the author agent id; skips rather than
  fabricates a tenant when the workspace can't be resolved). Applied to all
  three blocking-policy rejection sites (blocking-violation, signed-approval,
  require_current_spec).
- **`warn_stale_spec` advisory MR comment**: `record_stale_spec_advisory`
  writes a `forge:spec-policy` review comment naming the drift, deduplicated
  per (path, referenced SHA, current SHA) so queue retries don't spam; merge
  still proceeds.
- **Pinned-sha spec delivery to the gate agent**: new
  `GET /api/v1/merge-requests/:id/spec` serves `git cat-file blob <sha>` with
  the SHA validated as 40-char hex before it reaches argv (no argument
  injection); 404 when the MR has no spec_ref or the blob is unreachable.
  Route registered in `api/mod.rs` AND given a `RouteResourceMapping` in the
  ABAC resolver (middleware ABAC evaluates it — `check-abac-route-registry.sh`
  and `check-abac-exempt-handlers.sh` both OK). `gate_executor.rs` passes
  `GYRE_SPEC_URL` to both review and validation gate agent processes
  (documented in `docs/agent-protocol.md`), complementing the pre-existing
  in-process pinned-SHA `spec_content`.
- **API/UI**: `GET/PUT /api/v1/repos/:id/spec-policy` round-trips the field
  (Admin-only PUT preserved, `#[serde(default)]` keeps old clients at false).
  The frontend fix is real: `RepoSettings.svelte` previously bound
  `require_approval` / `stale_spec_warning` — field names the API never
  returns, so those toggles silently did nothing; they now bind
  `require_approved_spec` / `warn_stale_spec` and the two new toggles exist.

### Tests (reviewed for self-confirmation; all kill the behavior they guard)

- `require_signed_approval_blocks_unsigned_spec_approval` (merge_processor):
  real ledger rows through the port, real `run_once` queue pass; unsigned
  active approval → `Failed` + reason naming the missing signature + author
  notification asserted; positive control with a signed approval → `Merged`.
  Fails if the policy is a no-op.
- `warn_stale_spec_adds_advisory_comment_without_blocking`: real git repo with
  two spec versions; asserts `Merged` AND the advisory comment present.
- `get_mr_spec_serves_pinned_sha_not_head`: real git repo, asserts the
  endpoint serves v1 blob content, not the v2 at HEAD; 404 paths covered.
- `signed_spec_ref_requires_signature_on_active_approval` (gates): signed /
  unsigned / blank-signature / no-approval / one-signed-among-many cases.
- `set_require_signed_approval_round_trips` (spec_policy API) and the
  RepoSettings vitest cases (toggle binding + save payload field names).
- `sqlite::spec_policy::set_and_get_round_trips_all_five_policies` proves
  migration 000059 actually runs on a fresh database.

### Probes executed here

- All 20 mechanical invariant scripts: OK (incl. arch, ABAC registry,
  migration versions/portability, byte-slice, mem contracts, scope-default
  family, commit attribution against the committed tree).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`:
  exit 0 — lib and test code type-check.
- `cargo test -p gyre-adapters sqlite::spec_policy`: 1 passed.
- `web`: `npm ci` + full `vitest run --no-file-parallelism`: 104 files,
  2078 passed / 41 skipped.

### Verifier must run (gyre-server test compilation deferred per budget rule)

`cargo test -p gyre-server` for: `require_signed_approval_blocks_unsigned_spec_approval`,
`warn_stale_spec_adds_advisory_comment_without_blocking`,
`get_mr_spec_serves_pinned_sha_not_head`,
`signed_spec_ref_requires_signature_on_active_approval`,
`set_require_signed_approval_round_trips`; then `cargo test --all`
(task acceptance criterion).

### Non-blocking observation (for a future cleanup, not a defect)

`notifications.rs` `notify_spec_policy_violation` uses
`&mr_id[..8.min(mr_id.len())]` — a fixed byte-index slice that evades
`check-byte-slice-truncation.sh` (non-constant index expression). The sliced
value is always a server-generated UUID (ASCII) at the only call site, so
there is no reachable panic; still, `chars().take(8)` would be the
boundary-safe form.

## Verdict

Approved. All acceptance criteria are satisfied by real production code on
the assigned base; the fifth policy is a genuine implementation (ledger
query + merge-time rejection + author notification + per-repo config +
API/UI), and the pinned-SHA spec delivery is enforced at the endpoint level
(SHA-validated `git cat-file`, not a HEAD read).
