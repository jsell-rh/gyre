# Spec Lifecycle Automation

## Principle

"Specs first, always" is bidirectional. Specs must exist before code, AND when specs change, the system must react. Spec changes that go unnoticed produce drift. Drift produces systems that don't match their documentation.

This is not optional. It is enforced by the forge.

## Automatic Task Creation on Spec Change

When a push lands on the default branch that modifies files in `specs/`, the forge automatically creates tasks. This is a **server-side post-receive hook** - no human or agent action required.

### Trigger: New Spec Added

A file is added under `specs/` that didn't exist before.

**Action:** Create a task:
```
title: "Implement spec: {spec_path}"
description: "New spec added at {spec_path} (SHA {blob_sha}).
              Review the spec and plan implementation."
status: Backlog
priority: Medium
labels: ["spec-implementation", "auto-created"]
spec_ref: "{spec_path}@{blob_sha}"
```

The workspace orchestrator picks this up in its next OBSERVE cycle and decomposes it into implementation work.

### Trigger: Existing Spec Modified

A file under `specs/` is modified (content changed, new SHA).

**Action:** Create a task:
```
title: "Review spec change: {spec_path}"
description: "Spec modified at {spec_path}.
              Previous SHA: {old_sha} -> New SHA: {new_sha}.

              Review all code referencing the previous SHA for alignment.
              Affected references:
              {list of MRs/commits/tasks referencing old_sha}"
status: Backlog
priority: High
labels: ["spec-drift-review", "auto-created"]
spec_ref: "{spec_path}@{new_sha}"
```

The forge queries the spec-to-code binding ledger (from `agent-gates.md`) to find all commits, MRs, and tasks that referenced the old SHA. These are listed in the task description so the reviewer knows exactly what might be out of sync.

### Trigger: Spec Deleted

A file under `specs/` is removed.

**Action:** Create a task:
```
title: "Handle spec removal: {spec_path}"
description: "Spec deleted: {spec_path} (last SHA: {old_sha}).

              Determine if:
              1. The implementation should also be removed
              2. The spec was superseded by another spec (link it)
              3. The deletion was accidental (revert)

              Affected references:
              {list of MRs/commits/tasks referencing old_sha}"
status: Backlog
priority: High
labels: ["spec-deprecated", "auto-created"]
```

### Trigger: Spec Renamed/Moved

A file under `specs/` is renamed or moved to a different path.

**Action:** Create a task:
```
title: "Update spec references: {old_path} -> {new_path}"
description: "Spec moved from {old_path} to {new_path}.
              Update all cross-references in other specs, AGENTS.md,
              and specs/index.md."
status: Backlog
priority: Medium
labels: ["spec-housekeeping", "auto-created"]
```

## Task Deduplication

The forge checks for existing open tasks with the same `spec_ref` and label before creating a new one. If a `spec-drift-review` task already exists for the same spec path and hasn't been completed, the forge updates the existing task with the new SHA delta instead of creating a duplicate.

## Accountability Integration

The Accountability agent's patrol checks for:
- `spec-drift-review` tasks that have been open longer than one Ralph loop cycle
- `spec-implementation` tasks that have been in Backlog for more than N days
- Specs that were modified but have no corresponding task (should never happen if the hook works, but defense in depth)

If any of these are found, the Accountability agent escalates to the workspace orchestrator.

## Spec Approval Interaction

From the agent-gates spec: specs have an approval ledger. When a spec is modified:
- The old approval is **automatically invalidated** (the SHA changed)
- The new version requires fresh approval before any code referencing it can merge
- The auto-created `spec-drift-review` task should include re-approval as an acceptance criterion

## What This Does NOT Do

- **Does not auto-assign tasks.** The repo orchestrator decides task decomposition.
- **Does not block spec changes.** Anyone with push access can modify specs. The system reacts, it doesn't prevent.
- **Does not create tasks for non-spec files.** Only files under `specs/` trigger this behavior.
- **Does not create tasks for milestone specs.** Changes to `specs/milestones/` are informational (planning), not implementation contracts. Only `specs/system/` and `specs/development/` trigger tasks. *(Configurable per repo.)*

## Configuration

```toml
# Per-repo spec lifecycle config
[spec_lifecycle]
enabled = true
watched_paths = ["specs/system/", "specs/development/"]
ignored_paths = ["specs/milestones/", "specs/prior-art/", "specs/personas/", "specs/prompts/"]
auto_invalidate_approvals = true
dedup_open_tasks = true
default_priority_new = "Medium"
default_priority_modified = "High"
default_priority_deleted = "High"
```

## Implementation Notes

This is a **forge-native feature** - it runs as part of the post-receive hook in `git_http.rs`, alongside commit provenance recording and domain event broadcasting. The hook:

1. Diffs the push against the previous HEAD
2. Filters for changes in watched spec paths
3. Classifies each change (added/modified/deleted/renamed)
4. Creates tasks via the existing task domain model
5. Broadcasts `SpecChanged` domain events via WebSocket

No external CI, no webhook, no polling. Same-tick as the push.

## Relationship to Existing Specs

- **Agent Gates** (`agent-gates.md`): Spec approval is invalidated on change. Code referencing old SHA can't merge until new version is approved.
- **Source Control** (`source-control.md`): Post-receive hooks gain spec-awareness.
- **Design Principles** (`design-principles.md`): "Specs first, always" and "Reconciliation as a primitive" - the system automatically reconciles spec state with task state.
- **SDLC** (`sdlc.md`): This is an opinionated SDLC decision. The forge doesn't ask if you want a task - it creates one. The ceremony exists because the need is real.
