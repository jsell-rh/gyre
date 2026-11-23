# Review — task-329 (§9 MetaSpecPolicy in full: warn/block/tolerance, version-count reasoning, workspace round-trip)

Spec: `specs/system/meta-spec-reconciliation.md` §9 (MetaSpecPolicy struct, lines 403–421).
Candidate under review: `91a12d61` (branch `pipeline/task-329/515cafc89a124d93883cf5cd22f8c5b7-2`, base `87377fe1`; product commits `53268114` pre-merge and `1d00568b` post-merge, both attributed in the task frontmatter; docs `cbafd491`).
Verdict: **approved** — every deliverable is real production code on the task-302 versioned basis, the surfaces round-trip for real, and the tests assert hard end-state outcomes that fail at the base. No findings. gyre-server test execution is deferred to the verifier (wrapper-blocked in review stage; commands recorded in `/tmp/stage/review-evidence/task-329-probes.md`).

## Probes

- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → **exit 0** (10m08s; lib + all test code).
- Full diff read of all 9 touched files (+1455/−235); wiring greps for the enforcement call site (`selection_meta_spec_drift_hold` at merge_processor.rs:2079), the web payload shape (WorkspaceSettings.svelte sends exactly `{meta_spec_policy: {warn_on_drift, block_on_drift, drift_tolerance}}` through `api.updateWorkspace`, which passes the body through), and the Admin-gate parity between the workspace surface and `PUT /workspaces/:id/merge-policy` (both `auth.roles.contains(&UserRole::Admin)`).
- Commit attribution: both product-surface `implement(task-329)` commits are in the frontmatter `commits:` list; docs/process commits are out of scope per the check script's own definition.
- The checkout's unstaged `specs/tasks/task-329.md` change is the executor's protected-task-text restoration (progress reset to not-started); not part of the product diff, left untouched.

## Verified (per deliverable)

1. **Policy shape + storage** — `MetaSpecPolicy {warn_on_drift = true, block_on_drift = false, drift_tolerance = 1}` (merge_policy.rs), persisted per workspace through the pre-existing port-backed KV store (namespace `workspace_merge_policies` — no new in-memory state, no schema migration needed). Read-compat: a stored legacy `require_current_meta_spec_set: true` document maps to `block_on_drift: true, drift_tolerance: 0` (`StoredPolicyDoc → MetaSpecPolicy`), preserving block-on-any-drift for existing strict workspaces; new writes store the full shape only (legacy field never re-persisted — `new_write_replaces_legacy_strict_document`).
2. **warn_on_drift** — `selection_meta_spec_drift_hold` records the `[WARN]` gate result only when `policy.warn_on_drift` (non-strict), and always records it alongside the block in strict mode ("Always on in strict mode") before returning the hold reason.
3. **drift_tolerance version-count reasoning** — `mr_meta_spec_drift` (reconciliation.rs) now resolves full registry records and produces one `MetaSpecDriftTransition` per drifted required meta-spec: `versions_behind = current_version − recorded_version`; the MR blocks only when some transition is MORE than `drift_tolerance` behind (`exceeds_tolerance`). Legacy/missing/hash-disproved records surface as `recorded_version: None` and exceed ANY tolerance — never fail-open. Workspace/registry resolution errors propagate (fail closed). A recorded version newer than current yields `versions_behind = 0` (warn, not block) — correct per §9 ("behind"), and unreachable while registry versions are monotonic.
4. **Version-named warnings** — format is `[WARN] meta-spec-drift: {kind} {name} v{old} -> v{new}. Reconciliation may be needed.` (kind's `meta:` prefix stripped), the block reason names the transitions plus the tolerance, and the queue graph carries `transitions` labels. The parser round-trips (right-aligned splits for names with spaces; malformed labels fail closed to `None`).
5. **UI wiring** — `UpdateWorkspaceRequest` gains `meta_spec_policy` (partial update; `drift_tolerance` as a raw JSON number so negative/fractional/out-of-range are 400-grade `parse_drift_tolerance` errors, not serde 422s), validated and materialized BEFORE any workspace-row write (no partial application on 400), persisted via the same store the merge processor reads, Admin-gated identically to the merge-policy endpoint, and returned by GET workspace / list / create (create seeds the §9 default, accurate for a fresh workspace). The merge-policy endpoint exposes the full shape plus the legacy `require_current_meta_spec_set` view (`block_on_drift && drift_tolerance == 0`); the legacy PUT flag maps to tolerance-0 blocking with an explicit same-request `drift_tolerance` winning.

## Test quality (read line-by-line; all would fail at the base)

- AC1 `workspace_meta_spec_policy_roundtrip_via_update_workspace`: PUT the exact UI payload → GET returns exactly it; `-1` tolerance → 400; Developer JWT → 403; both rejected requests leave the stored policy unchanged (re-GET asserts). At the base the field is silently dropped, so the fresh-workspace default assertions alone fail.
- AC2 `meta_spec_drift_tolerance_one_warns_within_and_blocks_beyond`: registry bound at v4; the v3 MR merges with the exact warning string and a Passed drift gate result; the v2 MR stays `Open`, its queue entry `Queued` with a reason naming `persona backend-developer v2 -> v4` and `drift_tolerance 1`. At the base the strict boolean blocks both.
- AC3 `legacy_strict_flag_maps_to_block_with_tolerance_zero` (stored legacy doc read) + `meta_spec_drift_hard_blocks_under_strict_workspace_policy` (recorded v0 vs current v1 — one behind — blocked at tolerance 0, entry requeued, warning recorded alongside the block, relaxation merges on the next cycle) + `missing_policy_defaults_to_warn_only` / `meta_spec_drift_records_warn_gate_result_and_merge_proceeds` (default warns and merges).
- AC4 `warn_on_drift_false_suppresses_warning_except_in_strict_mode`: both halves — non-strict suppression with the MR still `Merged` and no drift gate result; strict block with the warning gate result present despite `warn_on_drift: false`.
- Decision units: `drift_check_two_versions_behind_exceeds_tolerance_one`, `drift_check_unverifiable_recorded_version_never_fails_open` (hash-disproved and missing-entry classes, `exceeds_tolerance(u32::MAX)`), `drift_check_older_recorded_version_reports_drift`, format round-trips including `v?` and non-parsing retired/trailing formats; `parse_drift_tolerance_rejects_invalid_numbers`; policy storage/legacy-mapping round-trips.

## Verifier must run

gyre-server test binaries cannot be compiled inside the review sandbox (wrapper-enforced 4 GiB budget). Commands and full rationale in `/tmp/stage/review-evidence/task-329-probes.md`:

    cargo test -p gyre-server --lib merge_policy
    cargo test -p gyre-server --lib meta_spec_drift
    cargo test -p gyre-server --lib warn_on_drift_false_suppresses_warning_except_in_strict_mode
    cargo test -p gyre-server --lib drift_check_
    cargo test -p gyre-server --lib workspace_meta_spec_policy_roundtrip_via_update_workspace
    cargo test -p gyre-server --lib merge_policy_full_shape_and_tolerance_validation

## Minor (non-blocking, recorded for follow-up)

- **Pre-existing web staleness, not introduced here:** `MergeQueueGraph.svelte` `driftLabel` reads `d.produced_set_sha`/`d.current_set_sha` — a shape the backend served in neither the base (`produced`/`current` snapshot-label arrays, task-302) nor this candidate (`transitions`); the queue-graph drift badge has rendered empty SHAs since task-302. task-329's contract covers the [WARN] output/block-reason naming and the WorkspaceSettings round-trip (both server-side, both delivered); recommend a follow-up to render `transitions`.
- `WorkspaceSettings.svelte` falls back to `drift_tolerance ?? 0` when the field is absent; the §9 default is 1. Inert now — the server always returns `meta_spec_policy` — but the fallback constant disagrees with the spec default.
- `merge_policy_or_default` degrades a policy-store outage to the warn-only default on the enforcement path — pre-existing, documented behavior, unchanged by this candidate.
