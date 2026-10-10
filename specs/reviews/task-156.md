# Review — task-156 (Reconciliation controller + conformance sweep, meta-spec-reconciliation.md §6/§10/§11)

Spec: `specs/system/meta-spec-reconciliation.md` §6 (Reconciliation: The Slow Rollout), §10 (Conformance Sweeps), §11 (Observability).
Candidate: `9be46de67ed3446565c9313e5ad96c82c337bb2f` (base `8c2d177505852b3e39cd77f4f782fb355de245aa`). All 11 frontmatter commits are present and are the only product-surface commits in the range (verified by `git log base..candidate` against `commits:`).
Verdict: **needs-revision** (3 code findings — 2 confirmed live, 1 confirmed live; evidence under `/tmp/stage/review-evidence/`).

## What was run (all at the candidate, working tree)

- `cargo test -p gyre-server --lib reconciliation` → **19 passed** (0.64–1.30s).
- `cargo test -p gyre-server --lib registry` → **49 passed**; `registry_writes_reject_non_admin` → **1 passed**.
- Mechanical gates: check-abac-exempt-handlers (89 handlers, 0 violations — the three meta_specs.rs exemption lines were correctly deleted with real per-handler authorization replacing them), check-abac-route-registry, check-arch, check-mcp-write-tools, check-dead-message-kinds — all OK.
- check-task-commit-attribution FAILS on `a781ede2 task-210`, but that commit is inside the assigned **base** (`git merge-base --is-ancestor a781ede2 8c2d1775` → in base) — pre-existing, not a candidate defect.
- Probe methodology: three temporary `#[cfg(test)]` modules registered in `lib.rs`, run, then removed; tree verified byte-identical to the candidate afterwards (`git diff 9be46de6` empty) and the 19 candidate tests re-run green post-cleanup. No production code, scripts, or specs were modified.

## Findings (all live-confirmed by probe, not inferred)

**F1 — `required` field silently dropped from PUT /meta-specs-registry/{id} (regression from 7aa240c0).**
`update_meta_spec_registry` lost base's `if let Some(required) = req.required` branch (base meta_specs.rs:1080-1082); `UpdateMetaSpecRequest.required` is still declared, so the field is accepted and ignored. Probe: create spec → PUT `{"required": true}` → 200 OK, `required` stays false (evidence: task-156-required-regression-probe.txt). Real caller: `MetaSpecs.svelte handleRequiredToggle` — toasts success while persisting nothing. Out-of-scope cleanup damage from the authorization-repair commit, exactly the class an independent review exists to catch.

**F2 — second reconciliation wave never emits ReconciliationCompleted (reconciliation.rs:104-136).**
Wave identity = `min(created_at)` over **all** reconciliation tasks, terminal ones included. Wave 2 completes → same wave_started_at as wave 1 → dedup matches the already-emitted message → suppressed. The in-code comment ("a later wave has a strictly later start and emits its own completion") is wrong. Probe: wave 1 emits (1 msg), wave 2 (new title, genuinely created) completes → still 1 msg, expected 2 (evidence: task-156-second-wave-probe.txt). Breaks §11 for every workspace reconciling more than once — the normal steady state. Fix direction: exclude terminal tasks from the cohort, or stamp a wave id on tasks at creation.

**F3 — registry-approval trigger misses the UI's edit→approve two-step flow (meta_specs.rs:1214).**
Trigger condition `ms.approval_status == Approved && req.prompt.is_some()` requires prompt and approval in ONE request. The shipped UI does `saveEdit` (PUT {prompt}, resets approval to Pending) then `handleApprove` (PUT {approval_status:"Approved"}) — the approve request has no prompt, so no reconciliation. `publishPersona` (api.js:556) also sends prompt only. Probe: two-step flow on a set-bound workspace → 0 reconciliation tasks; combined single request → 1 (evidence: task-156-approval-trigger-probe.txt). Acceptance criterion "Meta-spec approval triggers reconciliation task creation for affected repos" fails for the product's own flow. Fix direction: at approval time, compare the approved content_hash against the hash approved previously (real version change), not the shape of the current request.

## What is genuinely good (verified, not taken on trust)

- **§6 core is real**: `run_reconciliation` creates one task per repo (title/label/priority/Delegation type exactly per task contract), dedup is (workspace, repo, title) against non-terminal tasks — cross-workspace and cross-repo suppression fixed by 2197283c and pinned by tests (`reconciliation_creates_task_per_repo`, `reconciliation_two_workspaces_same_spec_not_suppressed`).
- **§10 sweep is real DB work**: registered job `meta_spec_conformance_sweep` (daily default, `GYRE_META_SPEC_SWEEP_INTERVAL_SECS` override) + `spawn_job` in main.rs; compares active set SHA against `chain_attestations.find_by_repo` provenance (30-day window); empty-SHA and no-set workspaces correctly skipped; drift-review task dedup is workspace+set-sha scoped; SQLite adapter for `find_by_repo` is real (tenant-filtered Diesel query) — only Pg is stubbed, and Pg stubbing is pre-existing for the entire authorization-provenance port family.
- **SHA stability fix is load-bearing and tested**: BTreeMap canonical serialization of MetaSpecSet with the identical-re-PUT / reordered-keys / changed-pin test triplet — without it every re-PUT would false-drift all provenance.
- **Per-handler authorization repair is real enforcement**: developer-JWT 403 on create/approve/delete, non-member workspace-scoped list 403, approval unchanged after rejection — the strongest test in the diff (uses real signed JWTs, asserts persisted state, not just status codes).
- Tests are not mirrors: each sweep/dedup test asserts persisted task counts, notification rows, and event messages through the real router or real state.

## Non-blocking observations

- `git_http.rs` spec-lifecycle dedup checks `!Done` (not `!Done && !Cancelled`); reconciliation's is stricter — no defect, just noting the "follow the same deduplication pattern" instruction diverges deliberately and the stricter form is correct.
- The drift-review task's `repo_id` is `drifted_repos[0]` (a workspace-level review anchored to the first drifted repo) — §10's "ensure a reconciliation task exists" is workspace-granular here; acceptable reading, noted for the coverage auditor.
- `list_meta_specs_registry` user_id=None (agent/system token) skips membership but keeps tenant containment — matches the established explorer_views pattern.
