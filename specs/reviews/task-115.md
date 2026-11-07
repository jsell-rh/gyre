# Review — task-115 (Implement spec approval signal chain & orchestrators)

Spec: `specs/system/agent-runtime.md` §1 Agent Lifecycle — The Model, The Signal
Chain, Phases 1–3 (spec approval triggers orchestration; workspace orchestrator
cross-repo coordination; repo orchestrator task decomposition).

Candidate under review: `933ac8ea43f88772866c4a1cea5860d4836ff6a1`
(base `86a98a29c47b5f1d2f5f8a80e2954e1df63158ee`, branch
`pipeline/task-115/53f316a10c0544adba3f548540911a42-1`, clean tree).
This is round 5: the candidate adds the round-4 fix commit `86415e4b`
(migration renumber) plus the docs commit `933ac8ea` on top of the four prior
implementation/repair rounds.

Verdict: **complete** — approved, no findings.

## What ran (this checkout, head `933ac8ea`)

All evidence under `/tmp/stage/review-evidence/`:

- `bash scripts/check-migration-versions.sh` → **OK** (the round-4 F1 pair is
  gone; `2026-10-10-000057_task_spec_ref` is the next unused sequence).
- All 19 other mechanical invariant scripts → **OK**
  (`check-migration-sql-portability`, `check-arch`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-fabricated-scope-defaults`,
  `check-scope-literal-defaults`, `check-unbounded-external-http`,
  `check-in-memory-state-stores`, `check-mcp-write-tools`,
  `check-dead-message-kinds`, `check-task-commit-attribution`,
  `check-relative-path-defaults`, `check-byte-slice-truncation`,
  `check-fail-open-ref-resolution`, `check-mem-port-contracts`,
  `check-lossy-secret-conversion`, `check-inert-enforcement`,
  `check-forwarded-header-trust`, `check-forged-scope-fields`).
- `cargo test -p gyre-server --lib signal_chain` → **19 passed, 0 failed**.
- `cargo test -p gyre-server --lib` (full crate) → **1334 passed, 0 failed**,
  including `approve_spec_emits_spec_approved_and_triggers_chain`,
  `reject_spec_cancels_chain_created_delegation_task`,
  `stale_detector_refreshes_system_orchestrator_instead_of_churning`.
- `cargo test -p gyre-adapters --lib sqlite::` → **266 passed, 0 failed**
  (incl. `sqlite::meta_spec` 10 — the round-4 F1 breakage class — and
  `sqlite::task` incl. `create_persists_workspace_scope`, the round-3 F1
  regression).
- `cargo test -p gyre-domain --lib` → **400 passed, 0 failed**.
- `cargo build -p gyre-cli` → OK (the diff adds a public `AppState` field;
  downstream crate still compiles).
- Fresh-probe (temporary test, reverted; `git status` clean after):
  a `spec_ref` value round-trips through the real SQLite schema + migration
  `2026-10-10-000057_task_spec_ref` (insert → find_by_id returns
  `path@sha` and bare `spec_path`), closing the one gap no existing adapter
  test covered (the mem-adapter chain tests assert the field end-to-end, but
  nothing asserted the new column's value on a real DB).

Sandbox transport restriction (per `/tmp/stage/capabilities.json`, unchanged
from base): listener-bound tests (`tty::`, `ws::`, `git_http::`, CLI
integration) cannot run here; `cargo test --all` on the exact head remains for
the verification gate. The full `gyre-server --lib` suite above did execute
1334 tests including the WS-auth unit tests that don't bind external
listeners.

## Verified working (no findings)

- **Round-4 F1 (duplicate Diesel migration version) genuinely fixed.** The
  directory is renumbered to `2026-10-10-000057_task_spec_ref` (next unused
  sequence after the base's `…-000056_meta_spec_approved_content_hash` and
  `2026-10-08-000056_spec_assertion_results`, which derive distinct versions).
  The script passes; the full `sqlite::` suite (fresh temp DBs per test)
  proves both columns exist and both migration bodies run on SQLite; the
  portability script proves the shared dir stays dialect-clean for PG.
- **Exemption-count reductions are real fixes, not registry laundering.**
  Both edits lower `FROZEN_EXEMPTION_COUNT` and delete the corresponding
  exemption lines — the sanctioned direction. Underlying sites verified in
  the diff: every reqwest client in `rig_vertexai.rs` (completion client,
  metadata server, refresh-token exchange, JWT exchange) is now built with
  `.timeout(...)`; the `reject_spec` fanout resolves `tenant_id` from the
  workspace record and skips+logs on lookup failure instead of
  `unwrap_or("default")`.
- **Phase 1:** the approve handler records the ledger approval, emits the
  `SpecApproved` bus message with the spec'd payload
  (`repo_id, spec_path, spec_sha, approved_by, approval_id`) and
  `Destination::Workspace`, then routes the same payload through
  `signal_chain::on_spec_approved`. (The payload additionally carries
  `workspace_id` for the server-side routing hook — additive, documented in
  code, and the bus destination already encodes it; not a spec
  contradiction.)
- **Phase 2:** `OrchestratorRegistry` provides per-workspace and per-repo
  tokio mutexes with stable lock identity across `AppState` clones (asserted
  by `registry_locks_are_per_scope_and_stable` /
  `registry_workspace_lock_serializes`). `on_spec_approved` holds the
  workspace lock, ensures exactly one live workspace orchestrator (reusing
  the task-093 spawn core with scoped JWT, or reusing the live/Idle one —
  `second_approval_reuses_then_respawns`), delivers the message to its
  inbox, creates the Delegation task for the spec's repo (spec_ref pinned
  `path@sha`, bare `spec_path` preserved for rejection cancellation — proven
  by `reject_spec_cancels_chain_created_delegation_task`), Coordination
  tasks for dependent repos via the pre-existing `spec_links_store`, and
  p4 `CrossWorkspaceSpecChange` notifications with the tenant resolved from
  the workspace record (never fabricated).
- **Phase 3:** the scheduler (`scheduler_run_once`, 30 s loop in `main.rs`,
  registered in the job registry so `/healthz` + admin trigger work)
  serializes whole cycles on the registry run lock, distinguishes
  Delegation/Coordination (claimed and processed) from Implementation and
  untyped tasks (left Backlog for Phase 4), claims via Backlog→InProgress,
  re-reads the task after taking the per-repo mutex, decomposes into ordered
  Implementation sub-tasks carrying `spec_ref`/`parent_task_id`/`order`/
  `depends_on` chaining, and marks the delegation Done. Failure paths are
  fail-closed: empty-SHA and no-ledger-SHA spec_refs hard-fail and release
  the claim to Blocked (`delegation_with_empty_sha_spec_ref_stays_blocked`,
  `delegation_without_resolvable_sha_stays_blocked`); a failed run releases
  to Blocked for retry (`scheduler_failure_releases_claim_and_retries`);
  stale InProgress claims are crash-recovered exactly once
  (`stale_claim_crash_recovery_processes_once`); concurrent cycles cannot
  double-process (`concurrent_scheduler_cycles_do_not_double_process`,
  `scheduler_cycles_serialize_on_the_run_lock`).
- **Budget accounting is balanced on the reuse path.** The Idle→Idle
  transition returns `Err` from `transition_status`, so the
  `decrement_active_agents` guard in `run_workspace_orchestrator` cannot
  double-decrement a reused orchestrator (spawn increments once for Active;
  the Active→Idle completion decrements once). The stale-agent detector's
  new Abort-path decrement captures `was_active` before the Dead transition.
- **No lock-ordering inversion:** the workspace lock is never held while
  acquiring the cycle or repo locks, and vice versa; the only nesting is
  cycle → repo, acquired in one place.
- **LLM latency bounds are real:** `decompose`/`coordinate` wrap
  `predict_json` in `tokio::time::timeout(LLM_CALL_TIMEOUT)` with
  deterministic fallbacks, and the Vertex AI HTTP clients now carry
  `.timeout(...)` — the unbounded-external-http invariant holds with zero
  exemptions.
- **System-orchestrator liveness:** the stale-agent detector refreshes
  heartbeats for `spawned_by="system"` orchestrators instead of
  kill/restart churning them (regression-tested over two consecutive
  cycles); user-spawned orchestrators still age out normally.

## Notes (non-blocking)

- Coverage rows 3–6 of `specs/coverage/system/agent-runtime.md` still carry
  the 2026-09-30 pre-implementation audit notes; in this repo's process the
  coverage matrix is refreshed by separate `audit(...)` commits, not by
  implementation tasks, so this is the auditor's next step, not a defect of
  this candidate.
- `cargo test --all` and the listener-bound suites remain for the
  verification gate (sandbox transport restriction, not a code defect).
