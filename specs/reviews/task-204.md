# Review — task-204 (spec-lifecycle §Accountability Integration — accountability patrol + orchestrator escalation)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration (coverage row 8).
Candidate: `eefeb2a6a786684deabd3a1577f49a9f40b89fbf` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`).
Verdict: **approved**.

## What was reviewed

Diff base..candidate: new `crates/gyre-server/src/spec_lifecycle_patrol.rs` (1048 lines incl. tests), handler + 2 endpoint tests in `api/specs.rs`, route in `api/mod.rs`, ABAC `RouteResourceMapping` in `abac_middleware.rs`, `SPEC_WATCHED_PATHS` visibility flip in `git_http.rs`, shared `PatrolFinding` gains optional `task_id`/`workspace_id` (backward-compatible, `skip_serializing_if`), docs/api-reference entry, coverage row 8 `task-assigned` → `implemented`, and a workflow-only e2e runner pin (CI repair for the retained Playwright/ubuntu-26.04 failure — matches the durable CI finding's log tail exactly).

## Independent verification (evidence under /tmp/stage/review-evidence/task204-review-*.log)

- `cargo test -p gyre-server --lib spec_lifecycle_patrol` → **11 passed, 0 failed** (stale/fresh, Backlog/non-Backlog, orphaned/covered incl. both path spellings and repo-mismatch masking, cancelled-task coverage, threshold override, broadcast routing, end-to-end flags+escalates).
- `cargo test -p gyre-server --lib api::specs::tests` → **60 passed, 0 failed** (incl. both new endpoint tests through the real router via `oneshot`).
- `cargo test -p gyre-server --lib spec_patrol::` → **19 passed, 0 failed** (regression on the shared `PatrolFinding` shape).
- **Kill-tests (each mutant applied, run, then restored; tree verified clean at candidate afterwards):**
  - `escalate_findings` early-return no-op → **3 tests fail** (`flags_and_escalates_only_the_accountability_gaps`, `spec_lifecycle_patrol_endpoint_flags_and_escalates`, `workspaceless_finding_broadcasts`) — escalation is asserted as persisted/dispatched messages, not logs.
  - Check-1 threshold inverted (`age_secs >= max_age_secs`) → **5 tests fail**.
  - Check-3 no-op (`spec_has_live_task` returns `Ok(true)`) → **2 tests fail**.
  The suite genuinely detects no-op checks, inverted thresholds, and skipped escalation.
- **Live HTTP probe** (listener capability supported in this sandbox; debug server on :3987, `GYRE_AUTH_TOKEN`): `GET /health` → 200; `POST /api/v1/patrol/spec-lifecycle` with no token → **401**, bad token → **401**, dev token → **200** `{"findings":[]}` (fresh DB); request thresholds reach the checks (server log shows `drift_review_max_age_secs=1`); invalid body type → **422**. This covers the full production middleware stack (`require_auth` + ABAC), which the in-process router tests alone do not.
- Mechanical checks: `check-arch.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh` — all pass. `rustfmt --check` clean on every touched Rust file. No new exemption-file entries (spec-lifecycle route is in the resolver, not the frozen exemption list — stronger gating than the exempt spec-links route).

## Findings

None blocking. Notes for the record:

- The endpoint unit tests use the bare `api_router()` (no auth/ABAC layers, which `build_router` adds); the live probe closes that gap — auth and ABAC gating are real over the deployed transport.
- `escalate_findings` counts an escalation even if `emit_event`'s persistence internally warns-and-continues; this matches the platform-wide `emit_event` fire-and-forget pattern used by `api/specs.rs` escalations, so it is consistent, not a regression.
- Check 3 reuses `drift_review_max_age_secs` as the post-receive-hook grace window; the choice is documented in the module and satisfies the task's "key off ledger `updated_at`" requirement.

The three spec checks run against real storage (`TaskRepository::list`/`list_by_spec_path`, `SpecLedgerRepository::list_all`), every finding escalates as a persisted `MessageKind::Escalation` to `Destination::Workspace(ws)` (Broadcast when scope unknown — never a fabricated `"default"`), thresholds are request parameters with documented defaults (24h/7d), terminal tasks are never flagged, and the coverage row update reflects shipped behavior.
