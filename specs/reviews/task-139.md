# Review — task-139 (Implement forge enforcement policies for spec-to-code binding at merge)

Spec: `specs/system/agent-gates.md` §Forge Enforcement Policies + §Integration with Agent Gates.
Candidate: `81dd6cb8` (base `1e61f663`; merge resolution of the previously published
task-139 head `a637e4a` onto current main — prior durable finding: DIRTY merge).
Attributed commits: `c92e8f53`, `33cc4941`, `ec05bf9a`, `059586a3`, `99edd0bc`
(all verified ancestors of the candidate and recorded in its frontmatter).
Verdict: **approved** — all acceptance criteria map to real production code;
focused probes executed where the sandbox budget allows; the remaining server
lib tests are recorded for the independent verifier (see below).

## What was reviewed

Full diff base→candidate (~1,633 insertions across 24 files), read hunk-by-hunk:
merge_processor enforcement block + violation reporting, gates.rs
`verify_signed_spec_ref`, merge_requests.rs `GET /:id/spec`, notifications.rs
`notify_spec_policy_violation`, gate_executor GYRE_SPEC_URL delivery +
blob-pinned context resolution, git_refs.rs `read_blob`, git2_ops.rs
`read_file_at_commit` blob-pin shape, ports/domain/adapters schema +
migration 2026-11-09-000059, abac_middleware route/allow-list entries,
web RepoSettings source + tests + locales, docs (api-reference,
agent-protocol), task134_review_probe extension, task-139.md shipped notes.

## Verification of each acceptance criterion

- **5 policies configurable per repo** — `SpecPolicy.require_signed_approval`
  added to domain struct (serde default false), SQLite + PG rows, migration
  000059 (`INTEGER NOT NULL DEFAULT 0`, portable per check script), API
  `GET/PUT /api/v1/repos/:id/spec-policy` round-trip, UI toggles. The other
  four pre-existed (M18/task-138) and are carried forward; the candidate's
  new SQLite test `set_and_get_round_trips_all_five_policies` proves all five
  persist through a fresh DB **including the new migration** (executed here: pass).
- **`spec_ref` on MR (path@sha)** — pre-existing (migration 000057); creation
  validates 40-hex SHA + format; responses expose it. No MR field-edit route
  exists in the product, so creation-time acceptance is the whole surface.
- **`require_spec_ref` / `require_approved_spec` / `require_current_spec`
  block at merge** — enforced in `process_next` before merge execution
  (pre-existing, unchanged semantics; `require_current_spec` has a real-git
  regression test at base). The candidate's restructure preserves behavior:
  blocking violations now funnel through one `violation` path that also
  reports to the author.
- **`require_signed_approval`** — new `verify_signed_spec_ref`: same per-SHA
  ledger query as `verify_spec_ref` (real `spec_approvals` port), requires an
  active approval with non-blank signature; distinguishes "active but
  unsigned" from "no active approval" in the error text; whitespace-only
  signatures treated as unsigned; one signed active approval among several
  rows satisfies. Merge processor blocks with Failed status + reason.
- **`warn_stale_spec`** — advisory, non-blocking: deduped MR comment
  (author `forge:spec-policy`, body names path + referenced SHA + current
  SHA) + pre-existing `StaleSpecWarning` domain event. The new test builds a
  real git repo, moves the spec past the pinned blob, and asserts Merged +
  advisory present.
- **Enforcement errors reported to author** — queue entry Failed with clear
  reason, `GateFailure` workspace event, and inbox notification to the
  author's spawning user (tenant scope resolved from the workspace lookup,
  no fabricated default; same delivery rule as `notify_gate_failure`;
  char-boundary-safe MR-id truncation).
- **Spec-alignment gate receives spec at pinned SHA, not HEAD** —
  `build_review_agent_context` resolves the MR's spec_ref through
  `GitOpsPort::read_file_at_commit`, now handling the *production* pin shape
  (verified: `spawn.rs:2005` pins `spec_ledger.current_sha`, a git blob SHA,
  with a ledger path that is not a tree path — the candidate's git2_ops
  blob-pin resolution is exactly that shape; unknown SHAs error fail-closed,
  not empty-spec). New `GET /api/v1/merge-requests/:id/spec` serves the
  pinned blob (SHA validated 40-hex before reaching the command line);
  `GYRE_SPEC_URL` is exported to AgentReview and AgentValidation gate
  agents; the scoped `review:submit`/`validation:report` token allow-lists
  and the ABAC route registry both include the route
  (`check-abac-route-registry.sh` + `check-abac-exempt-handlers.sh` pass).
- **`cargo test --all`** — deferred to the verifier per the 4 GiB build
  budget (the executor's wrapper rejects gyre-server test codegen); the
  exact required commands are recorded below and in
  `/tmp/stage/review-evidence/summary.md`.

## Probes executed (evidence: /tmp/stage/review-evidence/)

- All 18 repo mechanical invariant scripts: PASS
  (arch, ABAC registry + exempt handlers, MCP write gates, migration
  versions + portability, mem port contracts, dead message kinds, and the
  byte-slice/scope/secret/inert-enforcement/forwarded-header/external-http/
  relative-path/fail-open-ref/forged-scope lint families).
  `check-task-commit-attribution.sh` fails **only against the staged working
  tree**, because stage prep reset task-139.md to the assignment state
  (`commits: []`); against the candidate commit the frontmatter contains
  all five attributed SHAs (extraction verified) — no attribution gap.
- `cargo check -p gyre-server --lib --tests` → exit 0 (type-checks all new
  test code; build.rs also proves the authored web source builds).
- `cargo test -p gyre-adapters` focused: `sqlite::spec_policy` (1 pass —
  proves migration 000059 runs + five-policy round-trip), `read_file_at_commit`
  (4 pass — blob-pin resolution, fail-closed unknown SHA).
- `cargo test -p gyre-domain` → 466 + 8 pass (matches the task's Shipped claim).
- `cd web && npm ci && npx vitest run src/__tests__/RepoSettings.test.js` →
  58/58 pass, including the new round-trip test that kills a real
  pre-existing bug (toggles previously bound `require_approval` /
  `stale_spec_warning`, fields the API never returns — the toggles silently
  did nothing; now bound to the real field names and the save payload is
  asserted).
- Rebase-resolution checks (prior DIRTY-merge durable finding): merge commit
  `38e3372d` present; no conflict markers anywhere in the candidate; the
  only overlapping source file (schema.rs) merged additively (one-line
  column add on top of main's task-163 schema); migration versions unique
  under Diesel's date-qualified semantics and correctly ordered.

## Test-inflation check

None found. The new tests drive real behavior: real git repos on disk for
pin/staleness semantics (blob pinned at v1, HEAD moved to v2 — asserting the
pinned content, not the tip), the real ledger port for approval/signature
checks, the real router (auth + ABAC layers, oneshot) for scoped-token
spec-fetch including a negative containment assertion (still 403 outside the
allow-list), and a fresh SQLite DB for the migration. The negative cases
(unsigned approval must block; ghost blob must 404; no-spec_ref MR must 404;
blank signature treated as unsigned) would each fail if the production
behavior were disabled.

## Verifier-required probes (server test codegen exceeds this sandbox's budget)

- `cargo test -p gyre-server --lib merge_processor::tests` — especially
  `require_signed_approval_blocks_unsigned_spec_approval`,
  `warn_stale_spec_adds_advisory_comment_without_blocking`,
  `require_current_spec_blocks_stale_spec_ref`
- `cargo test -p gyre-server --lib api::gates::tests`
- `cargo test -p gyre-server --lib api::spec_policy::tests`
- `cargo test -p gyre-server --lib api::merge_requests::tests`
- `cargo test -p gyre-server --lib gate_executor::tests`
- `cargo test -p gyre-server --test task134_review_probe`
- `cargo test --all` (task AC)

## Non-blocking observations

- **Generated output (verifier-owned, recorded separately per policy):** the
  committed `web/dist/` is byte-identical to main's while `web/src` changed
  (RepoSettings toggles/locales). The deterministic verifier rebuilds and
  commits the tested assets after integration, so this is not a finding.
- Migration directory reuses sequence number 000059 (third 000059) rather
  than the next unused 000061; Diesel's version is date-qualified
  (`20261109000059`) so versions remain unique and correctly ordered, and
  `check-migration-versions.sh` passes. Cosmetic deviation from the AGENTS.md
  guidance text, no functional effect.
- `verify_signed_spec_ref` checks the signature field is present and
  non-blank; it does not cryptographically verify the Sigstore signature.
  This matches the task's own implementation plan ("reject if approval
  lacks signature field") and the ledger's field-level contract; deeper
  Sigstore verification would be new scope.
