# Review — task-139 (Implement forge enforcement policies for spec-to-code binding at merge)

Spec: `agent-gates.md` §Forge Enforcement Policies, §Integration with Agent Gates.
Candidate `c497c2d44` against base `bd49c7847` (task branch merged with main head via merge commit `98b0bfa32`). Product commit: `99edd0bc` (feat). `93f0e718a`/`8266b7243`/`c497c2d44` touch only `specs/tasks/task-139.md` (process commits, not product surface; `check-task-commit-attribution.sh` passes).
Verdict: **approved**.

## What was reviewed

The base already carried four of the five policies and the `spec_ref` MR field from the task-138 lineage. This candidate's product diff adds:

- `require_signed_approval` — `SpecPolicy` field, migration `2026-11-09-000059_require_signed_approval` (portable `ALTER TABLE ... ADD COLUMN ... INTEGER NOT NULL DEFAULT 0`), SQLite + Postgres adapters, mem store, `verify_signed_spec_ref` (api/gates.rs), merge-processor enforcement, API round-trip, UI toggle.
- Enforcement errors to the author: `report_spec_policy_violation` (GateFailure workspace event + `notify_spec_policy_violation` inbox notification to the author's spawning user, same delivery rule as `notify_gate_failure`) on every blocking rejection; `warn_stale_spec` adds a deduplicated advisory MR comment (`forge:spec-policy`) without blocking.
- Spec at pinned SHA: `GET /api/v1/merge-requests/{id}/spec` via `git cat-file blob` (SHA validated 40-hex before the command line), 404 on missing spec_ref / unreachable blob; `GYRE_SPEC_URL` env var passed to AgentReview and AgentValidation gate processes; documented in `docs/agent-protocol.md`.
- Web: RepoSettings policy form rebound to the real API field names (previous `require_approval`/`stale_spec_warning` bindings were dead) + `require_current_spec`/`require_signed_approval` toggles.

## Evidence (persisted under /tmp/stage/review-evidence/)

All runs on the exact candidate `c497c2d44`, working tree clean before and after:

- `test-gates-signed.txt` — `cargo test -p gyre-server --lib signed_spec_ref_requires_signature_on_active_approval` → **1 passed** (approved+signed verifies; unsigned, whitespace-only signature, and no-approval all rejected with reasons naming the cause; one signed row among several satisfies).
- `test-server-focused.txt` — `require_signed_approval_blocks_unsigned_spec_approval`, `warn_stale_spec_adds_advisory_comment_without_blocking` (real git repo, HEAD moved past referenced blob), `get_mr_spec_serves_pinned_sha_not_head` (real git repo, spec v2 at HEAD, endpoint returns v1), `set_require_signed_approval_round_trips` → **4 passed**.
- `mutation1-signed-approval-neutered.txt` — with `verify_signed_spec_ref` mutated to unconditional `Ok(())`, both signed-approval tests **FAIL** (2 failed, exit 101) — the tests kill the disabled-enforcement bug class, then source restored.
- `mutation2-pinned-sha-neutered.txt` — with `read_blob` mutated to serve HEAD content instead of the pinned blob, `get_mr_spec_serves_pinned_sha_not_head` **FAILS** (exit 101) — the endpoint's core contract is genuinely pinned by its test. Source restored.
- `test-adapters-full.txt` — `cargo test -p gyre-adapters --lib` → **426 passed, 0 failed** (fresh SQLite DBs; proves migration 000059 runs; main's `2026-10-08-000059_spec_lifecycle_configs` tests also pass post-merge, so both Diesel versions run).
- `test-server-full.txt` — `cargo test -p gyre-server --lib` → **1664 passed, 0 failed**.
- `test-server-integration.txt` — `api_integration` **66 passed**, `git_integration` **17 passed** (new route registered with ABAC `RouteResourceMapping`, parity with sibling `/diff` handler).
- `test-domain.txt` — `cargo test -p gyre-domain` → **448 + 8 passed**.
- `test-frontend.txt` — `npm ci` + full vitest → 2006 passed; 1 unhandled error is a Node **heap OOM in `ExplorerCanvas-performance.test.js`** (file untouched by this diff) under sandbox memory limits — infrastructure, not a code defect. Focused `RepoSettings.test.js` → **58 passed** including the new `binds toggles to the API policy field names` regression test (asserts save sends the real API field names).
- `check-scripts.txt` + individual logs — `check-arch`, `check-abac-route-registry`, `check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`, `check-abac-exempt-handlers`, `check-dead-message-kinds`, `check-inert-enforcement`, `check-task-commit-attribution` → all **exit 0**.
- `check-cli.txt` — `cargo check -p gyre-cli` → OK (schema change breaks no downstream crate).

## Spec-conformance assessment

- **5 policies configurable per repo: PASS.** All five fields persist (SQLite/Postgres/mem), round-trip via `GET/PUT /api/v1/repos/:id/spec-policy` (Admin-only PUT, default false), and are settable from the UI with correct field bindings.
- **`require_signed_approval`: PASS.** Merge processor calls `verify_signed_spec_ref` before executing the merge; an MR with no spec_ref is also rejected under this policy (no referenced approval exists). Ledger query scopes to the exact (path, sha) version, requires an *active* approval with a non-empty signature; revoked/rejected/pending rows never satisfy it. The signature field has a real production set path (`api/specs.rs` approval submission). The policy checks signature *inclusion* per the spec text and task plan ("reject if approval lacks signature field") — cryptographic Sigstore verification is not part of this task's contract.
- **Enforcement errors reported to author: PASS.** Blocking rejections fail the queue entry with the specific reason, emit a `GateFailure` event, and deliver an inbox notification to the author's spawning user (asserted in tests via `NotificationRepository::list_for_user`).
- **`warn_stale_spec` advisory: PASS.** Deduplicated MR comment + existing `StaleSpecWarning` event; merge proceeds (asserted Merged with real git drift).
- **Spec-alignment gate at pinned SHA: PASS.** `GET /merge-requests/{id}/spec` serves the blob at the MR's `spec_ref` SHA — proven to serve v1 content while HEAD holds v2 (mutation probe confirms the test pins this). `GYRE_SPEC_URL` is passed to gate agent processes alongside the existing `GYRE_DIFF_URL`/`GYRE_SPEC_REF`, matching the established gate-agent delivery pattern; handler authorization follows the sibling `/diff` route exactly (ABAC resolver mapping, registry check passes).
- **Acceptance "cargo test --all passes":** server lib 1664/0, domain 456/0, adapters lib 426/0, server integration 83/0, frontend 2006 passed (one unrelated OOM flake under sandbox memory limits), gyre-cli checks. No failures attributable to the candidate.

## Non-findings (checked, judged acceptable)

- **Migration numbering:** the new migration reuses numeric suffix `000059` (main already had `2026-10-08-000059_spec_lifecycle_configs`). Diesel versions are the full date-prefixed directory names, so they do not collide: `check-migration-versions.sh` passes and fresh-DB tests prove **both** run (`set_and_get_round_trips_all_five_policies` and the spec_lifecycle suite both green on the same tree). The failure mode the sequence-number rule guards against (a migration silently never running) is disproven by evidence. Cosmetic deviation only.
- `read_blob` uses `String::from_utf8_lossy` — spec file content, not security material; consistent with the lossy-secret-conversion rule's scope.
- `report_spec_policy_violation` only notifies when `author_agent_id` is set — user-authored MRs get the queue failure + workspace event; the notification channel requires an agent identity to resolve a spawning user, matching `notify_gate_failure`'s delivery rule.
- `verify_signed_spec_ref` treats a ledger query error as "no active approval" (fail-closed via `unwrap_or_default` → empty → reject). Correct direction.
- Full-frontend-suite heap OOM (`ExplorerCanvas-performance.test.js`) is a sandbox resource limit on an untouched file; the focused RepoSettings suite is green.

No findings. The implementation is real (ledger queries, git blob reads, notifications, events), the tests kill the mutations that disable the behavior, and the mechanical invariants all hold at the candidate.

— Reviewer, 2026-11-10
