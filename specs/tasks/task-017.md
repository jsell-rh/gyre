# TASK-017: Spec Registry — Manifest Enforcement on Push

**Spec reference:** `spec-registry.md` §The Manifest  
**Depends on:** None (manifest parsing and push-time spec registry hook already exist)  
**Progress:** `not-started`

## Spec Excerpt

From `spec-registry.md`:

> **Status:** `specs/manifest.yaml` is not yet enforced server-side (push rejection for unregistered spec files is future work).

The spec defines that the forge should reject pushes that add spec files under `specs/` without a corresponding entry in `specs/manifest.yaml`.

## Current State

The push-time spec registry hook (`crates/gyre-server/src/spec_registry.rs:680-713`) already:
- Reads `specs/manifest.yaml` from the pushed commit ✓
- Scans for `.md` files under `specs/` ✓
- Excludes `index.md`, `prior-art/`, `milestones/` ✓
- Checks if each spec file has a manifest entry ✓

**But** when a spec file is not in the manifest, it only **logs a warning** (`warn!`) instead of rejecting the push:
```rust
if !manifest_paths.contains(relative) {
    warn!(
        spec_path = %relative,
        "spec-registry: file under specs/ is not registered in manifest.yaml — \
         add it to specs/manifest.yaml to enable lifecycle tracking"
    );
}
```

The function `check_manifest_coverage` returns `()` — it cannot signal a rejection.

## Implementation Plan

1. **Change `check_manifest_coverage` return type** from `()` to `Result<(), Vec<String>>`:
   - Collect all unregistered spec paths
   - Return `Err(unregistered_paths)` if any are found
   - Return `Ok(())` if all specs are registered

2. **Wire rejection into the push handler** (`crates/gyre-server/src/git_http.rs`):
   - Call `check_manifest_coverage` during the push post-receive hook
   - If it returns `Err`, reject the push with a clear error listing the unregistered specs
   - Include the fix hint: "add entries to specs/manifest.yaml for: [list]"

3. **Make enforcement configurable** via spec policy (`spec_lifecycle` config):
   - Add `enforce_manifest: bool` field (default `true` for new repos, `false` for existing)
   - When `false`, keep the current warn-only behavior
   - This allows gradual rollout without breaking existing repos

4. **Add tests:**
   - Push with unregistered spec file → rejected (when enforcement enabled)
   - Push with all specs registered → accepted
   - Push with non-spec files in specs/ (e.g., `manifest.yaml`, `index.md`) → accepted
   - Push with enforcement disabled → warning only (not rejected)

## Acceptance Criteria

- [ ] Push adding spec files not in `specs/manifest.yaml` is rejected with clear error
- [ ] Error message lists unregistered files and includes fix hint
- [ ] `index.md`, `prior-art/`, `milestones/` are excluded from enforcement
- [ ] Enforcement is configurable via spec policy
- [ ] Tests cover enforcement and graceful degradation
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/spec_registry.rs` — `check_manifest_coverage` function (line ~680)
3. Read `crates/gyre-server/src/git_http.rs` — the push post-receive hook where spec registry is invoked
4. Read `crates/gyre-server/src/api/spec_policy.rs` — spec lifecycle policy configuration
5. Change the function return type and wire the rejection
6. Add the configurable enforcement flag
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
