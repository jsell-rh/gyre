# Review — task-339 (Accountability patrol manifest-aware checks: orphans, stale entries, pending-beyond-one-cycle, modified-without-task keyed on the manifest)

Spec: `specs/system/spec-registry.md` §Accountability Agent (gap `spec-c08e139362b99830187a32b4 / accountability-manifest-patrol`).
Candidate under review: `e8a065b6750fec0d47661027063bca08cedf2cd9` (branch `pipeline/task-339/5a2b9019f01145af9db98cd27fb1805a-1`, base `973068a86ab3154b6cba448fda9369125326cab7`; product commits `e23181ef` → `c203ba58` → `501038cc`, all listed in the task's `commits:` frontmatter).
Verdict: **approved** — all four manifest-aware checks are real production behavior keyed on the manifest at HEAD, the pending-since signal is durably recorded by the actual push/approve/reject paths, escalations are durable rows on the workspace orchestrator's stream, and the tests kill every fake class the verification intent names. No blocking findings.

## Probes run (checkout at candidate `e8a065b6`; working tree carries only a pipeline artifact revert of `specs/tasks/task-339.md`, which is not part of the product diff)

- `/tmp/stage/bin/cargo check -p gyre-server --lib` → **exit 0** (8m02s).
- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → **exit 0** (4m21s; test targets type-check — the new patrol tests, the `spec_pending_since` unit tests, and all updated `sync_spec_ledger` call sites compile).
- `/tmp/stage/bin/cargo test -p gyre-server --lib …` → rejected by the sandbox build wrapper (4 GiB budget; "gyre-server code generation plus the model exceeds 4 GiB") — an infrastructure restriction, not a code defect. **Verifier must run** the tests listed in `/tmp/stage/review-evidence/test-attempt-blocked.log`.
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → exit 0 (including the `501038cc` doc/attribute relocation in `spec_registry.rs`).
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-task-commit-attribution.sh` — timed out (full-history scan; verifier job). Manually verified: the only task-labeled product-surface commits on the branch are `e23181ef`, `c203ba58`, `501038cc`, all present in the frontmatter.
- Full diff read of all 7 changed files; grep sweeps over every production `ApprovalStatus::Pending` assignment in `gyre-server` (all remaining hits are `#[cfg(test)]`), the `KvJsonStore` implementations in both SQL adapters, `should_create_lifecycle_task` / `manifest_entry_for_path` / `effective_*` semantics, and all four `sync_spec_ledger` call sites (all pass `Some(&state.kv_store)`).
- Evidence: `/tmp/stage/review-evidence/` (check-lib.log, check-tests.log, test-attempt-blocked.log, mechanical-checks.log, static-review.md).

## What was verified

- **Orphan check (AC 1, probe state a).** `check_orphan_spec_files` reuses `spec_registry::find_unregistered_specs` (visibility-only change private → `pub(crate)`, body untouched) with `manifest_paths` built exactly as the push-time check builds them (`read_manifest_paths` class: raw `entry.path`) — the patrol flags exactly the files a push would be warned/rejected about, with the same exclusions (`index.md`, `prior-art/`, `milestones/`, non-`.md`), so `manifest.yaml` itself is never flagged however registered.
- **Stale-entry check (AC 1, probe state b).** `check_stale_manifest_entries` resolves each manifest entry's blob SHA at HEAD via `get_blob_sha` — the same determination `sync_spec_ledger`'s warning log makes — and surfaces it as an escalated finding instead of a log line.
- **Pending-beyond-cycle (AC 2, probe state c).** `check_pending_beyond_cycle` requires Pending status AND manifest `effective_requires_approval: true` AND a recorded `pending_since` AND `now − pending_since > drift_review_max_age_secs` (the patrol's existing one-cycle bound, overridable per request). The signal is a `spec_pending_since` KV record written through the port-backed `KvJsonStore` (implemented by both `SqliteStorage` and `PgStorage` — genuinely durable, restart-safe, satisfying the task's "equivalent durable state" option with no migration exposure) at every production (re-)entry into Pending (sync sites: new registration, content invalidation, re-pending after deprecation, both extends-drift invalidations) and maintained by `approve_spec`/`revoke_spec` via `on_transition` (prior status captured before mutation; Pending→Pending keeps the clock) and cleared by `reject_spec`/supersession-deprecation/manifest-removal. `updated_at` is never consulted. No signal ⇒ skip and log, never flag.
- **Modified-without-task rekeyed to the manifest (AC 3).** Task-eligibility now resolves through `manifest_entry_for_path(...).effective_auto_create_tasks(defaults)` when the repo's manifest resolves — mirroring `should_create_lifecycle_task` (git_http.rs) exactly — and falls back to the config's watched prefixes only for manifest-less repos (the same fallback the hook applies, so patrol and hook cannot disagree). Both `spec_path` spellings still queried via `spec_has_live_task`.
- **Escalations.** All four findings flow through the pre-existing `escalate_findings` (`MessageKind::Escalation` on the workspace event stream), skipping unresolvable workspaces with a log — no fabricated scope. Tests assert durable rows via `messages.list_by_workspace(kind=escalation)` and schema-validate the payloads.
- **Boundaries.** `spec_patrol.rs` untouched; `check_manifest_coverage` untouched (task-017); no new migration; no new in-AppState bare state (the KV store is port-backed); no `String::from_utf8_lossy` on secrets; `git` SHA decoding via `from_utf8_lossy` is the pre-existing non-secret class.
- **Tests kill the fake classes.** (i) Unconditional or `updated_at`-based flagging: the no-signal `legacy.md` case (Pending, requires_approval:true, `updated_at` old) would be flagged by either fake in the over-bound run — the test asserts exactly one finding; the fresh run (300 s) asserting `is_empty()` kills unconditional flagging independently. (ii) Findings-without-escalations: message-count assertions (2/1/1). (iii) Config-keyed regression: test 3 sets `watched_paths: []` and asserts the manifest-keyed finding still fires — the pre-task-339 code returns nothing. (iv) The pending-since marker in test 2 is recorded through the real `sync_spec_ledger` push path, not written directly. Real git fixtures throughout (`ls-tree`/`rev-parse` against actual commits).
- **Coverage rows.** Row 19 (and row 14's accountability clause) updated with concrete source evidence, status honestly `implemented` (no `verified` claim before independent verification); no unrelated rows touched.

## Non-blocking observations (recorded for completeness; none is a defect against the task contract)

- `get_blob_sha` (pre-existing helper) conflates "git command failed" with "file absent" (both → `None`), so a transient git failure during the stale-entry scan would flag that repo's entries as stale. The sync-time warning it mirrors has identical semantics, HEAD was resolved successfully moments earlier in the same context, and findings are warnings — an inherited wart of the exact scan class the task told the check to reuse.
- Manifest-less repos are skipped for the orphan/stale checks (documented design choice: no manifest ⇒ no manifest key; ledger-keyed checks fall back to the config rules the hook itself uses). The ACs and the verification probe only specify the manifest-present case.
- The Shipped note's phrase "sync signature gained the KV-store param" is inaccurate relative to merged base `973068a8` (the `kv` param pre-existed there; it was added on the branch's original pre-merge base). Doc nit in the task record only — the candidate compiles against the merged base (cargo check exit 0).
- Row 14's prose still opens with "Partial —" while its accountability clause now reads implemented; status stays `implemented` (accurate). Cosmetic; the verifier regenerates SUMMARY from the matrices anyway.

## Verifier must run (sandbox 4 GiB restriction on gyre-server test codegen)

- `cargo test -p gyre-server --lib spec_lifecycle_patrol`
- `cargo test -p gyre-server --lib spec_pending_since`
- `cargo test -p gyre-server --lib spec_registry`
- `cargo test -p gyre-server --lib api::specs` (approve/revoke/reject `on_transition` wiring)
- `bash scripts/check-task-commit-attribution.sh` (full-history; manually pre-verified clean for this branch)
