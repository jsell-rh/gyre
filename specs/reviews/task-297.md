# Review — task-297 (Gate all implementation work on approved specs)

Spec: `specs/system/agent-runtime.md` §1 The Signal Chain — "There is no other trigger. The `SpecApproved` event is the universal starting signal."
Candidate under review: `ab7e4e6f` (branch `pipeline/task-297/b9a162920c27499489e60092941b12c8-1`, base `82aed3db`; this round: CI-repair `69394abf`, docs `0425c6f3`, process `ab7e4e6f`).
Verdict: **approved** — no findings. All three gates re-verified at HEAD; this round's cascade-test reconciliation is correctly bounded, spec-amended per goal rule 4, and covered by a real kill test.

## Probes run (checkout at candidate `ab7e4e6f`, clean tree)

- `/tmp/stage/bin/cargo check -p gyre-server` → **exit 0** (cold run 8m15s; cached re-verify 0.65s, fingerprint-identical). Test-target compilation deferred to the verifier per the 4 GiB sandbox budget.
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- `bash scripts/check-mcp-write-tools.sh` → exit 0.
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0.
- `bash scripts/check-abac-route-registry.sh` → exit 0.
- `bash scripts/check-task-commit-attribution.sh` → timed out in this sandbox (full-history walk; infrastructure, not a code defect — verifier should run it).
- Full diff read of `82aed3db..ab7e4e6f` (18 files) plus focused reads of every `TaskType::Implementation` construction site, `UpdateTaskRequest`, `Task::transition_status`, `is_system_principal`, `gyre_spawn_worker`, `graph.rs` thorough-preview.
- Evidence under `/tmp/stage/review-evidence/`.

## This round: cascade-test reconciliation (`69394abf`)

CI failure `signal_chain::tests::scheduler_spawns_agent_for_cascade_test_task` was a genuine inter-spec conflict: task-280's `trigger_cascade_tests` files `task_type=Implementation` tasks with no `spec_ref`, which the task-297 Phase-4 gate unconditionally rejected. Resolution follows goal rule 4 — `agent-runtime.md` §1 is amended (not contradicted) to name forge-filed cascade-test execution as the one non-spec class of agent work, authorized by the workspace `require_cascade_tests` policy.

Verified:

- **The exemption is shape-bounded and cannot be forged through the API.** The scheduler match arms: `Some(spec_ref)` → always gated by `verify_implementation_spec_ref` (label irrelevant); `None` + `CASCADE_TEST_LABEL` → exempt; `None` otherwise → rejected. Reachability of the exempt shape (Backlog + Implementation + no spec_ref + cascade label) was checked by enumeration: `UpdateTaskRequest` has no `task_type`/`spec_ref` fields (immutable after creation); both task-creation surfaces reject `implementation` without an approved `spec_ref` regardless of labels; `mcp.rs` decompose/subtask tools pin a resolved `spec_ref`; the only other production creator of Implementation+no-spec_ref tasks (`graph.rs` thorough-preview) spawns its agent directly to InProgress, and `Task::transition_status` has no transition back to Backlog, so the scheduler never sees it.
- **Shared discriminator.** `merge_processor::CASCADE_TEST_LABEL` is the single constant used by the trigger, both completion routers (`spawn.rs`), and the scheduler exemption — no drift.
- **Kill test is real.** `cascade_label_does_not_bypass_spec_approval_gate`: a labeled task pinning a `spec_ref` whose approval was invalidated by a later push (registry moved to new SHA + Pending, approval rows revoked via `revoke_all_for_path`) stays Backlog with the `spec-approval-missing` label and zero agent records after `scheduler_run_once` — fails if the label arm is widened to spec-pinned tasks.
- **task-280 link restored.** `scheduler_spawns_agent_for_cascade_test_task` drives the real `trigger_cascade_tests` (dependency edge + `require_cascade_tests` policy) and asserts the filed task reaches InProgress with a spawned worker.

## Earlier-round gates re-verified at HEAD (no approval transfers; re-inspected)

- **Task-creation gate (REST).** `create_task` (api/tasks.rs): `task_type=Implementation` requires `verify_implementation_spec_ref` before `state.tasks.create`; `None` spec_ref and every unapproved/Pending/invalidated/unresolvable ref reject with 4xx naming the single-trigger requirement. Delegation/Coordination/untyped unaffected.
- **Task-creation gate (MCP).** `gyre_create_task` (mcp.rs): identical gate, `SPEC_APPROVAL_REQUIRED` tool error.
- **`verify_implementation_spec_ref` is real.** Two independent fail-closed checks: an active durable approval row at exactly `(path, sha)` (`find_by_spec_sha` + `is_active`), and the registry entry's `current_sha` matching the pinned SHA with `approval_status == Approved` — so an approval invalidated by a later push fails both.
- **Scheduler gate.** `spawn_implementation_workers` (signal_chain.rs): gate runs before repo/order/budget checks and before the Backlog→InProgress claim; on failure the task stays Backlog with the observable `spec-approval-missing` label (freshest-row re-read defense) and a logged reason; no agent record.
- **Direct-spawn gate.** `spawn_agent` (api/spawn.rs): non-interrogation spawn onto an Implementation task is 403 for every non-system principal (message names the requirement); `is_system_principal` is hardened against the user-named-"system" spoof (`agent_id == "system" && user_id.is_none()`, with a discrimination test); the in-process scheduler calls `spawn_agent_core` directly, and the MCP `gyre_spawn_worker` requires a repo-orchestrator token (system-spawned chain), so neither is an external bypass.
- **Approval side preserved.** `api/specs.rs` approve handler untouched by the diff; modified-spec invalidation (`spec_registry.rs`) untouched.

## Acceptance-criteria tests (all genuine, black-box or real-state)

- `api_integration::tasks_create_implementation_requires_approved_spec` — 400 + no task row before approval; real `POST /specs/:path/approve`; identical request 201 with pinned `spec_ref`; delegation unaffected.
- `signal_chain::scheduler_skips_unapproved_spec_and_spawns_approved_twin`, `scheduler_stops_spawn_when_approval_invalidated_by_push`, `cascade_label_does_not_bypass_spec_approval_gate`, `scheduler_spawns_agent_for_cascade_test_task`.
- `spawn::spawn_rejects_implementation_worker_for_external_callers` — human-principal 403, no agent record, task stays Backlog unassigned; interrogation spawn via an agent principal 201.
- MCP unit test asserting `SPEC_APPROVAL_REQUIRED` on the tool path.
- Test fixtures seed durable ledger state via `mem::seed_approved_spec` (`#[cfg(test)]`, mirrors what the approve handler writes) — not a gate stub.

Minor (non-blocking, recorded for completeness):

- Residual edge in the shape-based (not provenance-based) exemption: if a `thorough-preview` spawn fails AND its compensating task-delete also fails, an orphaned Backlog `Implementation`+no-`spec_ref` task labeled `spec-preview` persists; a caller with task-update rights could PATCH `labels` to add `cascade-test` and make it scheduler-spawnable. Exploitation requires a double failure plus an authenticated internal actor, and the exposure is strictly narrower than at base (where ANY Backlog Implementation task spawned unconditionally). A provenance check (e.g. the per-task cascade KV written at trigger time) would close it entirely if a future round touches this area.
- `check-task-commit-attribution.sh` could not complete in this sandbox (history walk timeout) — verifier should run it.

## Verifier must run

- `cargo nextest run -p gyre-server signal_chain::` (includes the four scheduler-gate/cascade tests).
- `cargo nextest run -p gyre-server tasks_create_implementation_requires_approved_spec` (api_integration).
- `cargo nextest run -p gyre-server spawn_rejects_implementation_worker_for_external_callers`.
- MCP create-task gate unit tests (`mcp_create_task_implementation_requires_approved_spec` and neighbors) and the merge_processor cascade suites (`trigger_cascade_tests_*`, `report_cascade_test_result_*`).
- `bash scripts/check-task-commit-attribution.sh`.
