---
title: "Manifest-driven spec lifecycle task creation (auto_create_tasks, per-spec policy)"
spec_ref: "spec-registry.md §15 Spec Lifecycle Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §15"
commits: ["1a07e1837d98a4433d67f238eaf3e7cd7cdd7e58", "06a78d19e07aea4c0a7fe9a489b814e5a3e28df7", "a13cf69d74a1a2d2a4852c94b488c8386a90675e", "236056d9510cffefda2c3143321b5b92ba8bb142"]
---

## Spec Excerpt

From `specs/system/spec-registry.md` §14/§15 (Integration with Existing Specs → Spec Lifecycle) and §Manifest Rules:

> The spec lifecycle hooks now use the manifest instead of path prefix matching:
> - **Which files trigger hooks:** files listed in the manifest with `auto_create_tasks: true`
> - **Per-spec priority:** manifest can override default task priority
> - **Per-spec gates:** manifest declares which gate agents review MRs referencing this spec
> - The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section

> 4. **The manifest is the single source of truth for policy.** The `spec_lifecycle` config block in the spec-lifecycle spec is superseded by per-spec manifest entries.

## Problem

Hollow. `process_spec_lifecycle()` (`crates/gyre-server/src/git_http.rs:1360-1540`) classifies changed spec files purely by **path** via `parse_spec_changes` / `classify_spec_change` (`git_http.rs:1272-1358`). It never reads `specs/manifest.yaml`. Consequences that violate §15:

- A changed spec whose manifest entry sets `auto_create_tasks: false` (e.g. `trusted-foundry-integration.md`, personas) **still spawns a task**. `parse_spec_changes` only filters by watched path prefixes (`specs/system/`, `specs/development/`), not by the manifest.
- Files under `specs/` that are **not** in the manifest but sit in a watched prefix still create tasks; files in the manifest under a non-watched prefix are missed.
- Per-spec priority declared in the manifest is ignored — priority is hardcoded by change kind in `classify_spec_change`.

## Implementation Plan

1. In `process_spec_lifecycle`, after computing the git diff for the update but before creating tasks, read the manifest at the push's new HEAD: `let manifest = crate::spec_registry::read_manifest(repo_path, &update.new_sha).await;` (helper introduced by task-193).

2. For each changed `(status_char, path, old_path)`:
   - Resolve the manifest `SpecEntry` for `path` (normalize the `specs/`-prefix exactly as task-193's resolution and `sync_spec_ledger` do). For renames (`R`), match on the new `path`.
   - **Gate task creation on the manifest, not path prefixes:** create a task only when the changed file has a manifest entry with `effective_auto_create_tasks(&manifest.defaults) == true`. If the file is not in the manifest, do **not** create a lifecycle task (the manifest is the source of truth); keep emitting a warning for unregistered `specs/` files if that behavior already exists elsewhere, but do not spawn work.
   - When a manifest is genuinely absent (manifest-less repo), preserve today's path-prefix behavior as a fallback so existing repos keep working — log at debug that manifest-driven filtering was skipped.

3. **Per-spec priority:** if the manifest entry (or `defaults`) carries a priority override, apply it to `task.priority` instead of the hardcoded value from `classify_spec_change`. If the manifest schema currently has no priority field, DO NOT invent one silently — either (a) use the existing `classify_spec_change` priority when no override is declared, or (b) if adding a `priority` field is in scope per the spec's "manifest can override default task priority", add it to `SpecEntry`/`ManifestDefaults` as `Option<TaskPriority>` and thread it through. Prefer the minimal real change: read an override if present, else fall back.

4. Keep auto-invalidation of stale approvals (git_http.rs:1421-1461) intact — that logic is correct and independent of task creation. Only the **task-creation** decision must become manifest-driven.

5. Preserve the existing dedup (skip if a non-Done task with the same title exists) and the `SpecChanged` / `TaskCreated` event emission for specs that DO get tasks.

## Acceptance Criteria

- A push modifying a spec whose manifest entry has `auto_create_tasks: false` creates **no** lifecycle task (and emits no `TaskCreated`/`SpecChanged` for it), while a push modifying a spec with `auto_create_tasks: true` still creates exactly one task.
- A push modifying a `specs/…` file that has **no** manifest entry creates no lifecycle task (manifest is the source of truth), given a manifest exists at HEAD.
- In a repo with no `specs/manifest.yaml`, lifecycle task creation still works via the existing path-prefix fallback (no regression).
- A test (in `git_http.rs` tests) that FAILS if the manifest gate is removed: given a fixture manifest marking spec A `auto_create_tasks: false` and spec B `true`, exercising the lifecycle over a diff touching both yields a task for B only. Prefer testing the manifest-filtering decision as an extracted, testable helper if `process_spec_lifecycle` is hard to invoke directly.
- `cargo build --all`, touched-crate tests, and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Reuse `crate::spec_registry::read_manifest` from task-193; do not add a second manifest reader.
- Extract the "should this change create a task?" decision into a pure, unit-testable function taking `(Option<&SpecManifest>, status_char, path)` so the behavior can be tested without spawning git.
- Do NOT weaken the test into asserting only that some task exists — assert the specific inclusion/exclusion by title/path.
- Do NOT hardcode a priority; only apply an override when the manifest declares one.
- Run only touched-crate tests plus `scripts/check-arch.sh`; skip formatters and the full suite.

## Shipped

### Rebase repair (this round)

The branch's task-194 implementation (manifest-driven lifecycle gating, from
commit 236056d95 and its review repairs) was rebased onto the new base
3af80bd21 (task-158: per-repo `SpecLifecycleConfig`, repo-defined personas,
task-104 SpecChanged audit events), which conflicted in
`crates/gyre-server/src/git_http.rs`. The merge commit e61a835ce resolves the
conflict by combining both sides' behavior:

- **Task creation is manifest-driven** (spec-registry.md §15): `process_spec_lifecycle`
  reads `specs/manifest.yaml` at the pushed HEAD via task-193's
  `crate::spec_registry::read_manifest` and gates every change through the pure
  helper `should_create_lifecycle_task(manifest, config, status_char, path, old_path)`.
  With a manifest present, a task is created only for files with a manifest entry
  whose effective `auto_create_tasks` (per-entry override, else `defaults:`) is true;
  unregistered `specs/` files create no task. Renames match on the NEW path.
- **Manifest-less fallback uses the repo's `SpecLifecycleConfig`** watched/ignored
  prefixes (either side of a rename), preserving task-158's config behavior
  (including custom watched paths outside `specs/`) for repos without a manifest —
  exactly the "`[spec_lifecycle]` config block is superseded by the manifest" rule:
  config governs only when no manifest exists.
- **Diff parsing is unfiltered** (`parse_name_status`); the decision moved entirely
  into the gate so manifest-registered specs under non-watched prefixes are no
  longer missed. The old `parse_spec_changes_with_config` pre-filter was removed
  and its config assertions ported to gate-level tests
  (`should_create_lifecycle_task_no_manifest_custom_watched_paths`,
  `..._ignored_overrides_watched`).
- **Per-spec priority override**: `SpecEntry.priority` / `ManifestDefaults.priority`
  (task-193 schema) override the change-kind default from `classify_spec_change`,
  which itself honors the config's `default_priority_*`. No priority is hardcoded.
- **Kept intact from the base**: `config.enabled` master switch, config-gated
  approval invalidation (now scoped to `specs/` paths, with `ledger_path`
  normalization), config-gated dedup with the task-104 SpecChanged audit event,
  per-push workspace-scope resolution, persona push processing, and
  `SpecChanged`/`TaskCreated` event emission for specs that do get tasks.

### Test evidence

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` pass
  (test code compiles; exit 0).
- `cargo test -p gyre-domain` passes (466 + 8 tests).
- `bash scripts/check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-inert-enforcement.sh`, `check-dead-message-kinds.sh` pass.
- `cargo fmt -p gyre-server -- --check` clean.
- The gyre-server test binary cannot be executed in this sandbox (build wrapper
  blocks server test compilation under the 4 GiB budget). The independent
  verifier must run:
  `cargo test -p gyre-server --lib git_http::`
  covering in particular:
  `should_create_lifecycle_task_manifest_gate`,
  `should_create_lifecycle_task_defaults_section_applies`,
  `should_create_lifecycle_task_no_manifest_falls_back_to_prefixes`,
  `should_create_lifecycle_task_no_manifest_rename_either_side_watched`,
  `should_create_lifecycle_task_no_manifest_custom_watched_paths`,
  `should_create_lifecycle_task_no_manifest_ignored_overrides_watched`,
  `spec_lifecycle_manifest_gates_task_creation_e2e` (fixture manifest marks
  spec-a `auto_create_tasks: false`, spec-b `true` + priority override, spec-c
  unregistered; asserts exactly one task for spec-b by title/path/priority and
  exactly one SpecChanged/TaskCreated event pair),
  `spec_lifecycle_without_manifest_uses_prefix_fallback_e2e`,
  `spec_lifecycle_without_manifest_rename_from_watched_prefix_e2e`,
  `spec_lifecycle_per_repo_config_drives_filtering_and_priority`,
  `spec_lifecycle_disabled_skips_task_creation`, and
  `spec_lifecycle_dedup_skip_still_records_spec_changed_audit`.
