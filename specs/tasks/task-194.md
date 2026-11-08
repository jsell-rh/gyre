---
title: "Manifest-driven spec lifecycle task creation (auto_create_tasks, per-spec policy)"
spec_ref: "spec-registry.md §15 Spec Lifecycle Integration"
depends_on: [task-193]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §15"
commits: ["236056d9510cffefda2c3143321b5b92ba8bb142"]
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

§15 manifest-driven spec lifecycle task creation is implemented in commit `236056d95` (assignment base `124a852be`).

### Behavior

- `process_spec_lifecycle` (`git_http.rs`) reads `specs/manifest.yaml` at the pushed HEAD via task-193's `crate::spec_registry::read_manifest` (single reader, reused) and gates every task-creation decision on it. A manifest present but unparseable is treated as absent (`read_manifest` logs and returns `None` → prefix fallback).
- New pure gate `should_create_lifecycle_task(Option<&SpecManifest>, status_char, path)` in `git_http.rs`: with a manifest, a task is created only when the changed file resolves to a manifest entry whose effective `auto_create_tasks` (per-entry override, else the `defaults:` section) is true; unregistered `specs/…` files create no task (manifest is the single source of truth). With no manifest at HEAD, the legacy `specs/system/` + `specs/development/` prefix fallback applies (logged at debug) — no regression for manifest-less repos.
- New `manifest_entry_for_path()` in `spec_registry.rs` resolves diff paths (`specs/system/foo.md`) to entries using the same `specs/`-relative identity `sync_spec_ledger` uses; renames match on the new path.
- `parse_spec_changes` now admits all `specs/` changes (eligibility decided by the manifest), fixing the missed case of manifest-registered specs under non-watched prefixes.
- Per-spec priority: optional `priority` field on `SpecEntry` and `ManifestDefaults` with `effective_priority()` — entry override, else `defaults:`, else the `classify_spec_change` change-kind default. No priority is hardcoded; an override applies only when declared.
- Auto-invalidation of stale approvals, title dedup, and `SpecChanged`/`TaskCreated` emission are unchanged and now fire only for specs that actually get tasks.

### Test evidence

- `should_create_lifecycle_task_manifest_gate`, `…_defaults_section_applies`, `…_no_manifest_falls_back_to_prefixes` (pure unit tests): per-entry `false` excludes, `true` includes, unregistered excluded, non-watched-prefix registered included, defaults section applies, manifest-less fallback preserved.
- `spec_lifecycle_manifest_gates_task_creation_e2e`: drives `process_spec_lifecycle` against a real git repo; a diff touching spec-a (`auto_create_tasks: false`), spec-b (`true`, `priority: Critical`) and unregistered spec-c yields exactly one task — `Review spec change: specs/system/spec-b.md` with priority `Critical` — and exactly one `SpecChanged` (payload spec_path == spec-b) + one `TaskCreated` on the broadcast bus; nothing for spec-a/spec-c by title and spec_path.
- `spec_lifecycle_without_manifest_uses_prefix_fallback_e2e`: manifest-less repo still creates the watched-prefix task (priority defaults to High, no override).
- Mutation check: replacing the manifest branch of the gate with prefix matching makes 3 tests fail (both gate unit tests + the e2e, which then sees tasks for spec-a and spec-c) — the tests kill a removed manifest gate.
- `spec_registry::tests`: `manifest_entry_for_path_resolves_with_and_without_prefix`, `effective_priority_entry_override_beats_defaults`, `effective_priority_falls_back_to_defaults_then_none`.
- Gates: `cargo build --all` ok; `cargo test -p gyre-server --lib` 1381 passed / 0 failed; `bash scripts/check-arch.sh` passed.
