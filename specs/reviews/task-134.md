# Review — task-134 (AgentReview / AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Gate Types (Extended) + §AgentReview Gate.
Candidate under review: `4ee7b5a5b9df27c26783c75c90c3acfb83886cb8` (base `63aa1486`).
Verdict round 2 (round 1 was an infrastructure non-completion): **findings — one code defect; not approved this round.**

## Independent evidence (this sandbox, exact candidate HEAD)

All commands run with `/tmp/stage/bin/cargo` on the clean checkout of `4ee7b5a5`; logs under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-server --lib gate_executor` → **35 passed, 0 failed** (`gate_executor-tests.log`).
- `cargo test -p gyre-server --test task134_review_probe` → **7 passed, 0 failed** (`task134-review-probe.log`) — oneshot-router probes, no listener needed.
- `cargo test -p gyre-server --lib agent_review_end_to_end` → **2 passed, 0 failed** (`agent-review-e2e.log`). Unlike the implementer's sandbox, loopback `accept()` works here (capabilities.json records the listener probe as supported), so both real-server e2e tests genuinely executed: real axum server with the full auth+ABAC stack, real bare git repo with the spec pinned at a pre-tip SHA, the fixture `review_agent_driver.sh` driven through the live Review API (`http_code=201` asserted in gate output), reviewer identity bound to `gate-review-*` (forged body id ignored), ChangesRequested → Failed, token revoked after verdict.
- `cargo test -p gyre-server --lib abac_middleware` → 9/9; `api::gates` → 6/6; `git_http` → **37/37** (the errno-95 clone failure reported by the implementer does not occur in this sandbox); `merge_requests::tests` → 21/21; `cargo test -p gyre-adapters read_file_at_commit` → 3/3; `sqlite::quality_gate` → 1/1 (upsert round-trip incl. `required` drift guard).
- Mechanical checks all exit 0 at this HEAD: arch, abac-route-registry, abac-exempt-handlers, mcp-write-tools, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults (exemption-file delta is line-number drift only — same four entries, no additions), scope-literal-defaults, inert-enforcement, lossy-secret-conversion, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, forged-scope-fields, migration-versions, migration-sql-portability, task-commit-attribution.

## Verified real (no fakery found)

- **Gate types / CRUD / persistence**: `validation_type` column (migration `2026-10-09-000056`, next unused number, portable SQL), round-tripped by SQLite and Postgres adapters; gate CRUD accepts/returns both types plus `validation_type` with a list round-trip assertion.
- **Spawn protocol**: persona resolved nearest-wins (repo → workspace → tenant; unresolvable workspace skips tenant scope instead of fabricating `"default"`); unresolvable persona or pinned spec_ref fails the gate before any process spawn or token mint (asserted: no `gate-*` tokens in kv). MR context is real: full diff via `git_ops.diff`, spec content at the pinned SHA via the new `read_file_at_commit` port (pre-tip SHA pinned-content test proves v1-not-v2; unknown SHA is an error, not `Ok(None)`), MR title, task description via MR → author agent → task.
- **Scoped token**: `mint_scoped` JWTs carry `review:submit` (reviewers) / `validation:report` (validators) only. Enforcement is layered and exact-match: `git_receive_pack` 403s scoped tokens before repo resolution; the ABAC middleware allow-lists each capability's route patterns only (write routes like `/status`, `/dependencies`, `/atomic-group` are outside the list; `/merge-requests/:id` itself is GET-only so no method widening); `submit_review` binds reviewer identity to the token subject. The probe suite proves each layer discriminatingly, including lookalike-scope (`review:submit:admin`) non-inheritance and validator denial on review submission with no persisted review.
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the review body surfaced; only the gate's own agent's verdict counts (other agents' approvals ignored); exit-0-without-review fails. No-command gates fail closed (the old auto-approve/auto-pass stubs are deleted — verified gone from the diff).
- **Teardown**: token revoked via `kv_remove` on every exit path (verdict, timeout, spawn failure) — asserted by tests; revoked tokens 401 (probe).

## Finding (blocking)

**F4 — scoped gate-agent JWT TTL is fixed while the process timeout is per-gate configurable.**
`crates/gyre-server/src/gate_executor.rs`: both mint sites pass `AGENT_GATE_TIMEOUT_SECS + 60` (360s; lines 577 and 852), but both process waits use `gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS)` (lines 680 and 921). `timeout_secs` is an uncapped `Option<u64>` accepted by `POST /api/v1/repos/:id/gates`. For any gate configured with `timeout_secs > 360`, a review agent that submits its verdict after t=360s authenticates with an expired JWT — `auth.rs` validates `exp` for kv-registered JWTs ("Invalid or expired agent token", 401) — so an Approved decision submitted within the configured window is discarded and the gate falsely fails with "exited without submitting a review". That breaks the spec's blocking condition ("Blocks Until: Agent submits Approved decision") and the acceptance criterion "Review verdict maps to gate status" under a supported configuration. Reproduction: create an AgentReview gate with `timeout_secs: 700` and a command that sleeps ~400s, then POSTs an approved review with `GYRE_REVIEW_TOKEN`; the POST returns 401 and the gate fails. Fix: mint with `gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS) + 60` at both sites (and mirror in the validation path). No test covers a configured `timeout_secs` exceeding the token TTL — add one (e.g. a gate with `timeout_secs` large and a TTL-bound mint assertion, or an e2e variant with a short configured timeout and a late-submitting driver).

## Non-blocking observations (recorded, not findings for this task's contract)

- The reviewer allow-list is route-scoped, not MR-scoped: a `review:submit` token can submit a review on any MR id it can address (the reviewed MR is delivered via `GYRE_MR_ID`, not enforced server-side; `task_id` claim carries the gate id, not the MR id). The spec excerpt for this task requires only "scoped OIDC token with `review:submit` permission only", which is satisfied; MR-level identity scoping belongs to §Gate Agent Lifecycle (task-135). Flag for that task.
- Exempt-from-ABAC routes (`RouteResourceMapping::exempt`) return before the scoped-token allow-list, so scoped tokens reach them and rely on the per-handler authorization those routes must already enforce (mechanically checked). No violation observed.
- `cargo test --all` is owned by verification; every touched module is covered by the focused suites above.

## Verdict

`approved: false` — one code finding (F4). Everything else in the task contract is implemented for real and independently verified, including both real-server e2e tests executing and passing in this sandbox. The one-line TTL fix (plus a regression test) should clear this task in the next round.
