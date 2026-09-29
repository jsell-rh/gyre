---
title: "Manifest-driven spec lifecycle task creation"
spec_ref: "spec-registry.md §15 Spec Lifecycle Integration"
depends_on: []
progress: not-started
coverage_sections:
  - "spec-registry.md §15"
commits: []
---

## Spec Excerpt

From `specs/system/spec-registry.md` §15 (Integration → Spec Lifecycle):

> The spec lifecycle hooks now use the manifest instead of path prefix matching:
> - **Which files trigger hooks:** files listed in the manifest with `auto_create_tasks: true`
> - **Per-spec priority:** manifest can override default task priority
> - **Per-spec gates:** manifest declares which gate agents review MRs referencing this spec
> - The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section

Manifest rule §4: "The manifest is the single source of truth for policy."

## Problem (current state)

`process_spec_lifecycle` in `crates/gyre-server/src/git_http.rs:1361` drives task creation entirely by **path prefix** matching:
- `parse_spec_changes` (git_http.rs:1311) filters changed files against the hardcoded `SPEC_WATCHED_PATHS` prefix list — the manifest is never read.
- `classify_spec_change` (git_http.rs:1273) produces a task title/labels/priority purely from the git status char and path.
- Consequences the spec forbids: a manifest entry with `auto_create_tasks: false` still spawns a task; a spec file not registered in the manifest still spawns a task if it matches the prefix; per-spec priority is impossible to express.

## Implementation Plan

1. **Add optional per-spec priority to the manifest schema.** In `crates/gyre-server/src/spec_registry.rs`, add `priority: Option<String>` (or a typed enum deserialized to `gyre_domain::TaskPriority`) to `SpecEntry`, and an `effective_priority(&self, defaults)` helper. Add a matching optional default to `ManifestDefaults` if a manifest-wide default is warranted. Keep it `#[serde(default)]` so existing manifests parse unchanged.

2. **Read the manifest inside `process_spec_lifecycle`.** For each ref update, read `specs/manifest.yaml` at `update.new_sha` (for D/R cases where the file is gone at HEAD, read at the new HEAD anyway — the manifest reflects post-push policy) via `crate::spec_registry::read_git_file` + `parse_manifest`. Build a lookup from spec path → `SpecEntry`.

3. **Gate task creation on manifest policy.** For each changed spec path:
   - If the path has no matching manifest entry, do NOT create a lifecycle task (still perform approval auto-invalidation — that behavior at git_http.rs:1420-1461 stays, since a removed/modified file must stale its approvals regardless).
   - If the matching entry's `effective_auto_create_tasks(&defaults) == false`, do NOT create a task.
   - Otherwise create the task as today, but override `task.priority` with the entry's `effective_priority` when set (falling back to the current `classify_spec_change` default).

4. **Preserve existing behavior** for approval auto-invalidation, dedup, `SpecChanged`/`TaskCreated` event emission, and cross-workspace notification.

## Acceptance Criteria

- A modified spec file whose manifest entry has `auto_create_tasks: false` produces NO lifecycle task (but its active approvals are still invalidated).
- A changed file under `specs/` that is NOT registered in the manifest produces NO lifecycle task.
- A registered spec with `auto_create_tasks: true` (or defaulted true) produces a task, and when the entry declares a `priority`, the created task carries that priority.
- Existing `SpecChanged` / `TaskCreated` emission and approval auto-invalidation are unchanged.
- `cargo build --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Real work only. Task creation MUST be gated by the parsed manifest, not the `SPEC_WATCHED_PATHS` prefix list. It is acceptable to keep `SPEC_WATCHED_PATHS` only as a cheap pre-filter for which git-diff entries to even parse, but the authoritative create/skip and priority decisions MUST come from the manifest entry.
- Write a hard test that fails if the manifest gating regresses: e.g. given a manifest where `system/foo.md` has `auto_create_tasks: false` and `system/bar.md` has `auto_create_tasks: true`, a push modifying both creates exactly one task (for bar) with the manifest-declared priority. Test the decision logic directly (factor a pure helper that takes the manifest + change list and returns the tasks-to-create) so the test does not need a live git repo. Do NOT write self-confirming/assertionless tests.
- If §15's "per-spec priority" cannot be expressed without a spec/schema change, add the `priority` field (this task authorizes it); do not silently drop the requirement.
- Skip project-wide formatters/linters and the full test suite; run only the targeted tests plus `cargo build --all` and `scripts/check-arch.sh`.
- Respect hexagonal boundaries; this work lives in `gyre-server` (manifest + lifecycle hook) and reuses existing ports.
