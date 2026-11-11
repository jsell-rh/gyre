# Review — task-204 (spec-lifecycle accountability patrol: task-age checks + orchestrator escalation)

Spec: `specs/system/spec-lifecycle.md` §Accountability Integration — the Accountability agent's patrol checks for (1) `spec-drift-review` tasks open longer than one Ralph loop cycle, (2) `spec-implementation` tasks in Backlog more than N days, (3) specs modified with no corresponding task; any hit escalates to the workspace orchestrator.
Commits under review: the 14 SHAs in the task's `commits:` frontmatter, base `cfeefbda` → candidate `8f88e1b8` (includes the rebase repair round onto task-099's `emit_event` and the round-1 escalation-test repair `bb5e82ee3`).
Verdict: **complete**.

## Round 1 (recorded in durable finding 630bdf88)

Blocking F1: `flags_and_escalates_only_the_accountability_gaps` and `spec_lifecycle_patrol_endpoint_flags_and_escalates` asserted persisted escalations for workspace "ws1" without seeding the workspace; post-rebase `emit_event` (task-099) resolves the tenant scope via `message_tenant_id` and silently skips persistence when `workspaces.find_by_id` yields None — so both tests failed with 0 persisted messages.

## Round 2 (this review, candidate `8f88e1b8`)

### F1 repair verified end-to-end (source trace + probes)

`bb5e82ee3` seeds workspace "ws1" (tenant "default") in both escalation-asserting tests, exactly mirroring the `spec_patrol.rs` harness. Every link in the chain the asserts depend on was independently traced:

- `MemWorkspaceRepository::create` inserts the record; `find_by_id("ws1")` now yields it (mem.rs:1993–2025).
- `AppState::emit_event` → `message_tenant_id(Some("ws1"))` → `Some("default")` → proceeds to `messages.store` instead of the task-099 skip-and-log (lib.rs:541–588).
- `MemMessageRepository::list_by_workspace` filters `workspace_id == "ws1"`, excludes Agent-destined, matches `kind.as_str() == "escalation"` (`MessageKind::Escalation` → `"escalation"`, message.rs:190) — the persisted Workspace-destined escalations are exactly what the tests count.
- Escalation payload supplies required `reason` (String) + optional `context` (String), matching `MessageKind::Escalation`'s schema (message.rs:335–337); the test also asserts `validate_payload` passes on every persisted message.

Probes: `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0 (cold, 6m37s). gyre-server test execution is wrapper-deferred to the verifier (4 GiB budget rule; attempt logged in `/tmp/stage/review-evidence/cargo-test-attempt.log`, exit 64 with the wrapper's deferral message) — verifier must run `cargo test -p gyre-server spec_lifecycle_patrol`, `... spec_lifecycle_patrol_endpoint`, `... patrol`, and `cargo test --all`.

### Contract verification (no findings)

- **All three checks run against real storage.** `run_spec_lifecycle_patrol` (spec_lifecycle_patrol.rs) queries `TaskRepository::list` + `SpecLedgerRepository::list_all`, resolves per-spec coverage via `TaskRepository::list_by_spec_path`, and per-repo watched paths via `SpecLifecycleRepository::get_for_repo` — the same config `git_http::process_spec_lifecycle` consults, so the patrol and the hook cannot disagree about which specs are supposed to have tasks. No in-memory stand-ins introduced; hexagonal boundary respected (`check-arch.sh` passes).
- **Check 1**: non-terminal (`Done`/`Cancelled` excluded) tasks labeled `spec-drift-review` with `now - created_at > drift_review_max_age_secs` (default 24 h, documented as the one-loop-cycle proxy). Label spellings match the hook's (`git_http.rs:1745/1752`).
- **Check 2**: `spec-implementation` AND `status == Backlog` past `implementation_backlog_max_age_secs` (default 7 d); the explicit Backlog equality also excludes terminal statuses.
- **Check 3** (defense in depth): watched-path ledger entries with a `current_sha`, non-`Deprecated`, past a grace window keyed off ledger `updated_at` (so brand-new specs the hook hasn't processed in the same tick are not flagged), and no non-`Cancelled` task referencing the path — queried under both the ledger's `system/x.md` and the hook's `specs/system/x.md` spellings; repo-scoped when both sides know the repo so an identically-named spec in another repo cannot mask a gap. `enabled = false` repos are skipped, mirroring the hook's early return.
- **Escalation is real enforcement.** Every finding goes through `state.emit_event(Option<Id>, Destination::Workspace(id), MessageKind::Escalation, Some(payload))` with a payload identifying the finding (`finding_type`, `task_id`/`spec_path`, `message`, severity, source, suggested_action) that satisfies the message-bus escalation schema. Unresolvable workspace → `Destination::Broadcast`, never a fabricated `"default"` scope (aligned with the task-099 invariant; `escalation_route` unit-tested for blank/None). Reuses the existing `MessageKind::Escalation` — no new variant, so no exhaustive-match wiring needed (`check-dead-message-kinds.sh` passes).
- **HTTP endpoint.** `POST /api/v1/patrol/spec-lifecycle` (`patrol_spec_lifecycle`, api/specs.rs) registered in `api/mod.rs` next to the spec-links route; optional `drift_review_max_age_secs`/`implementation_backlog_max_age_secs` with documented defaults, mirroring `PatrolRequest.stale_threshold_secs`; thresholds are never hardcoded in the check bodies. Storage failures map to 500, never an empty finding list. Gating is *stronger* than the existing patrol endpoint: the new route carries a real `RouteResourceMapping` ("spec" resource, POST → write action) in the ABAC resolver while legacy `/api/v1/patrol/spec-links` sits in the frozen exemption file — `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` both pass, no exemption growth.
- **Tests are hard, not self-confirming.** Module tests (11) assert exact flagged sets across stale/fresh/terminal/Backlog/non-Backlog/repo-mismatched/cancelled-coverage/unwatched/deprecated/grace-window seeds; `thresholds_are_not_hardcoded_in_the_checks` and the endpoint thresholds test prove both directions (tightening changes the verdict — kills no-op and inverted thresholds); the end-to-end test asserts exactly 4 findings AND 4 persisted `escalation` messages with identifying payloads, schema-validated; `workspaceless_finding_broadcasts` asserts on the dispatch channel (broadcast messages are not persisted) and that nothing lands under fabricated scopes. Each would fail if its check were a no-op, thresholds inverted, or escalation skipped. The two escalation-asserting tests now seed the workspace so the persisted-message asserts prove real emission (F1 repair).
- **Diff scope is clean.** `PatrolFinding` gains optional `task_id`/`workspace_id` with serde defaults and all construction sites updated (only spec_patrol.rs constructs it); `cargo fmt --check` clean; no scripts, exemption files, or unrelated tasks touched; the new module mirrors the spec_patrol.rs structure as the plan required.

Findings:

- (none)

Deferred to the independent verifier (wrapper restriction, recorded in `/tmp/stage/review-evidence/`): `SKIP_WEB_BUILD=1 cargo test -p gyre-server spec_lifecycle_patrol`, `SKIP_WEB_BUILD=1 cargo test -p gyre-server spec_lifecycle_patrol_endpoint`, `SKIP_WEB_BUILD=1 cargo test -p gyre-server patrol`, `SKIP_WEB_BUILD=1 cargo test --all`.

— Reviewer, 2026-11-11 (candidate `8f88e1b8`)
