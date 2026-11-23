# Review — task-339 (Accountability patrol manifest-aware checks)

Spec: `specs/system/spec-registry.md` §Accountability Agent (gap `2236f6ecfe981117d57f7990`).
Candidate under review: `48580e26` (base `0b33cb3a`; product commit `c203ba58`, checkpoint `e23181ef`, process commits `7e803722`/`de812fe3`/`48580e26`).
Verdict: **approved** — all four manifest-aware checks are real production behavior keyed on the manifest at each repo's HEAD, the pending-since signal is durable and maintained by every production transition path, findings escalate through the existing `MessageKind::Escalation` route, and the task boundaries (`spec_patrol.rs`, push-time coverage rejection) are untouched. No findings.

## Probes run

- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m54s).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (4m12s) — covers the three new patrol tests and the `spec_pending_since` unit tests at the type level.
- `cargo test -p gyre-server --lib spec_lifecycle_patrol` → **blocked by the sandbox wrapper** (exit 64: gyre-server code generation exceeds the 4 GiB budget; infrastructure restriction, not a code defect). Runtime execution deferred to the verifier — see the list below.
- Mechanical invariants: `check-arch`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-in-memory-state-stores`, `check-inert-enforcement`, `check-byte-slice-truncation`, `check-lossy-secret-conversion`, `check-dead-message-kinds`, `check-relative-path-defaults` → all exit 0. `check-task-commit-attribution` fails only against the harness's uncommitted working-tree revert of `specs/tasks/task-339.md`; against the committed candidate both task-labeled product commits (`c203ba58`, `e23181ef`) are listed in the frontmatter — passes.
- Full diff read of all 7 touched files plus grep sweeps (below). Evidence: `/tmp/stage/review-evidence/`.

## Verified working

1. **Orphan spec files** (`check_orphan_spec_files`, spec_lifecycle_patrol.rs:356) — reuses `spec_registry::find_unregistered_specs` (now `pub(crate)`, otherwise unchanged: `git ls-tree -r` at HEAD, `index.md`/`prior-art/`/`milestones/`/non-`.md` excluded, so `manifest.yaml` is never flagged) against the manifest paths at each repo's HEAD. Findings carry the repo's real workspace scope.
2. **Stale manifest entries** (`check_stale_manifest_entries`, :399) — per-entry `get_blob_sha` at HEAD; absent blob → finding, escalating what was previously only the sync-time warning log. Detection class identical to `sync_spec_ledger`'s own "no corresponding file at HEAD" warning.
3. **Pending beyond one cycle** (`check_pending_beyond_cycle`, :455) — requires `approval_status == Pending` AND effective `requires_approval: true` (manifest at the entry's repo HEAD, per-entry override or `defaults:`) AND a recorded pending-since whose age exceeds `drift_review_max_age_secs` (the same one-cycle bound check 1 uses; endpoint defaults to `DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS`, overridable per request). No recorded signal ⇒ skip + log, never flag unconditionally, never `updated_at`.
   - **Durability of the signal** (`spec_pending_since.rs`): port-backed `KvJsonStore` record (`kv_store` table, shared migration `2026-03-23-000009`, runs on both SQLite and PostgreSQL — the "equivalent durable state" the task text allows). Written at every production (re-)entry into Pending: sync content-invalidation, re-pending after deprecation, new registration, extends-drift invalidation; cleared on manifest-removal deprecation, rejection, and supersedes-deprecation; `on_transition` handles approve/revoke (entering pending restarts the clock, leaving clears, unchanged status preserves — a partial approval must not restart it). Grep sweep confirmed no other production site sets a ledger entry to `Pending`. All four production `sync_spec_ledger` call sites (git_http push, mirror_sync, both api/repos sync endpoints) pass `Some(&state.kv_store)`. Read path propagates errors (patrol fails closed); writes are best-effort with logging, matching the ledger saves' own contract.
4. **Modified-without-task, manifest-keyed** (`check_modified_specs_without_tasks`, :559) — task-eligibility now mirrors `should_create_lifecycle_task`'s manifest branch exactly: `manifest_entry_for_path` (both path spellings normalized) + `effective_auto_create_tasks`, `unwrap_or(false)` for undeclared paths; the config watched-prefix keying survives only as the fallback for manifest-less repos — the same fallback the hook itself applies, so patrol and hook cannot disagree. Both `spec_path` spellings still queried via `spec_has_live_task`. The `enabled = false`, Deprecated, empty-SHA, and grace-window exclusions are retained.
5. **Escalation** — all new findings flow through the pre-existing `escalate_findings` (:695)/`run_and_escalate` path: `MessageKind::Escalation` on the workspace event stream, resolvable-workspace-only (skip + log otherwise; no fabricated scope).
6. **Boundaries** — `spec_patrol.rs` and the push-time coverage rejection (`check_manifest_coverage`) have empty diffs; only `find_unregistered_specs` visibility changed.
7. **Tests** — the three new tests map 1:1 onto the ACs and are not self-confirming: real git fixtures (`make_git_repo`), real repo rows, real `sync_spec_ledger` push-path writes for the pending markers, escalation rows asserted via `messages.list_by_workspace(..., kind=escalation)` with payload-identification and `MessageKind::Escalation::validate_payload` checks. AC2's test asserts both directions (fresh ⇒ zero findings; over-bound ⇒ exactly one) plus the requires_approval:false and no-signal skips — it fails if the check flags unconditionally or falls back to `updated_at`. AC3's test sets the repo config to watch NO prefixes and asserts the manifest-keyed finding plus the `auto_create_tasks: false` non-finding. Logical trace of each test against the implementation confirms the asserted finding sets are exact.
8. **Coverage rows** — row 19 (and row 14's overlap note) updated with accurate source evidence; line numbers verified against the candidate; status stays `implemented` (not `verified`) — no independent-verification claim.

## Minor (non-blocking, recorded for completeness)

- The stale-entry check inherits `get_blob_sha`'s conflation of "path absent at HEAD" with "git command failed" (a transient git failure would flag entries as stale). Same detection class as the sync-time warning it escalates; `resolve_head_sha` already succeeded for the repo in that run, so the window is narrow.
- The orphan check inherits the push-time scan's exact-match manifest-path spelling (a manifest that spells paths with a leading `specs/` would orphan-flag its own files). Faithful reuse of the class the task directs it to reuse; the other three checks normalize both spellings.

## Verifier must run

(Test-target compilation exceeds this sandbox's budget; wrapper rejects gyre-server test builds.)

- `cargo test -p gyre-server --lib spec_lifecycle_patrol` — the three new tests (`flags_orphan_files_and_stale_manifest_entries_with_escalations`, `flags_pending_beyond_cycle_only_when_the_recorded_age_exceeds_it`, `modified_without_task_is_keyed_on_the_manifest_not_the_config`) plus the pre-existing patrol tests (config-fallback behavior for manifest-less repos must stay green).
- `cargo test -p gyre-server --lib spec_pending_since` — the three transition-semantics unit tests.
- `cargo test -p gyre-server --lib specs::` — the approve/revoke/reject suites in `api/specs.rs` (the `on_transition`/`clear_pending` additions must not disturb them) and the patrol endpoint tests (`spec_lifecycle_patrol_endpoint_flags_and_escalates`, `spec_lifecycle_patrol_endpoint_honours_thresholds`).
- `cargo test -p gyre-server --lib spec_registry` — sync suites (marker writes ride the sync paths).
- Full workspace suite + Clippy per standard verification.
