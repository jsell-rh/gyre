---
title: "Dependency graph: transition dependents' edges to Orphaned when the target repo is archived or deleted"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / dep-edge-orphaned-on-target-archive-delete / ed9f34316476"
depends_on: []
progress: not-started
commits: ["9ef33dea19f70e2a10cb8b121d5d81905f5299be"]
---

## Required behavior

Per dependency-graph.md §Dependency Entity, DependencyStatus::Orphaned means "Target repo was deleted or archived" — that transition must actually happen in production code, on all four surfaces where it is currently missing: (1) archive_repo (crates/gyre-server/src/api/repos.rs): after the repo row transitions to archived, every stored edge with target_repo_id == repo.id that is not already Orphaned is marked Orphaned and persisted through the existing DependencyRepository port (state.dependencies.list_dependents + save); edges are never deleted; a per-edge save failure logs a warning without aborting the archive, matching the handler's existing best-effort cleanup discipline. (2) delete_repo (same file): edges whose target is the deleted repo are marked Orphaned, not removed — the edge rows survive the repo row deletion, consistent with the spec's reconcile philosophy ("don't delete - might be a detection gap"). (3) detect_dependencies_on_push (crates/gyre-server/src/git_http.rs): archived repos are excluded from the known-repo match set on every detection path (Cargo.toml path/git refs, package.json, go.mod, pyproject.toml, manifest spec links, openapi/proto/mcp contract matching), so no new edge is created targeting an archived repo and reconcile does not resurrect an Orphaned edge whose target is archived back to Active (git_http.rs:4446 currently un-orphans on re-detection); unarchive makes the target matchable again and the next push to the source re-activates the edge through the existing reconcile un-orphan path. (4) dep_staleness_check (crates/gyre-server/src/dep_staleness.rs): the continuous monitor transitions edges whose target repo no longer resolves in the repo list (deleted, including edges stranded by pre-fix deletions) or is archived to Orphaned, evaluated before the existing Breaking/Orphaned skip so the skip only ever sees already-orphaned edges. No port-trait changes (list_dependents already exists on DependencyRepository with SQLite and PostgreSQL adapters), no spec changes, no new routes — the spec contract is implemented exactly as written.

## Acceptance Criteria

- [ ] Storage-backed test (real SQLite DependencyRepository, not a mem stand-in): repos A and B with an edge A→B; archiving B through the archive_repo handler transitions the edge to status Orphaned, visible via the dependency port / GET /api/v1/deps/repo/{B}/dependents, and the edge row still exists afterwards.
- [ ] Pushing to A while B is archived (dependency declaration still present in A's tree) leaves the edge Orphaned — no resurrection to Active and no new edge created targeting the archived repo.
- [ ] Unarchiving B and pushing to A returns the edge to Active via the existing reconcile un-orphan path.
- [ ] Deleting B (after archive) leaves the edges targeting B present in storage with status Orphaned — they are not deleted with the repo.
- [ ] An edge whose target repo row no longer resolves (or is archived) is transitioned to Orphaned by running dep_staleness_check.
- [ ] cargo build and cargo test pass; scripts/check-arch.sh passes; no new entries in any scripts/*-exemptions.txt; the per-edge port contract (save) is honored by both storage adapters unchanged.

## Verification intent

Create a real detected Active edge A→B (push A containing `repo-b = { path = "../repo-b" }` in Cargo.toml with B a known tenant repo), then archive B and push A again. On current code this fails the specced behavior: archive_repo never touches dependency edges (the handler's post-archive work only cancels tasks and emits events), and detect_dependencies_on_push matches archived repos because state.repos.list() still includes them, so reconcile re-confirms the edge and would even un-orphan it (git_http.rs:4446). The probe asserts status == Orphaned after archive + re-push — it must fail before the fix and pass after it. A second probe for the monitor: remove the target repo row (a pre-fix deletion leaves exactly this state) and run dep_staleness_check — current code leaves the edge untouched (the job only skips already-Orphaned edges and never orphans anything; target_repo None yields target_version None with no status change), so asserting Orphaned there kills the real defect rather than checking a symbol exists.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `4baa14e92efd107253381d13cd29b4038d0f21d30c35d5f0da35d4eaf776b808`.

[
  {
    "id": "4390a1b94a03e318854cbbec",
    "decision": "gap",
    "requirement_quote": "    Orphaned,     // Target repo was deleted or archived",
    "detail": "The entity shape is complete: the production DependencyEdge carries every specced field (id, source/target repo ids, dependency_type, source_artifact, target_artifact, version_pinned, target_version_current, version_drift, detection_method, detected_at, last_verified_at, status) and all four statuses exist and are set by real transitions (Active on create, Stale by drift/staleness, Breaking by the breaking-change cascade). The gap is the documented semantics of Orphaned itself: \"Target repo was deleted or archived\". No production path marks a dependents' edge Orphaned when the target repo is archived or deleted. archive_repo cancels the repo's tasks and updates the repo row but never touches dependency edges; delete_repo cleans up spec links and spec assertion results but leaves dependency edges untouched; the daily dep_staleness_check job explicitly skips edges already Orphaned and never orphans anything itself; the only Orphaned transition is reconcile_dependencies' no-longer-detected check, which runs only when the SOURCE (dependent) repo is pushed and only for detection methods that ran in that push. Consequences: a deleted target's edges keep their prior status indefinitely until the dependent happens to push again (lazy, best-effort coverage of the \"deleted\" half), and an archived target is never orphaned at all — archived repos remain in RepoRepository::list, so detection keeps matching them and reconcile re-confirms the edge as detected. The spec's own comment names both causes; neither is wired.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "quote": "    // Clean up the spec-link graph rows sourced from this repo (spec-links.md",
        "detail": "delete_repo's cleanup list, inspected in full: it deletes the repo row, spec-link graph rows, spec assertion results, and the on-disk git directory — dependency edges pointing at the deleted repo are absent from the handler; nothing marks them Orphaned."
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "quote": "    repo.archive();\n    repo.updated_at = now_secs();",
        "detail": "archive_repo transitions the repo row to archived and proceeds to cancel active tasks — the inspected handler contains no dependency-edge handling, so edges whose target is this repo remain Active/Stale despite the spec's Orphaned semantics for archived targets."
      },
      {
        "file": "crates/gyre-server/src/dep_staleness.rs",
        "quote": "if edge.status == DependencyStatus::Breaking || edge.status == DependencyStatus::Orphaned {",
        "detail": "The daily dep_staleness_check job (the continuous monitor) treats Orphaned as \"managed by other systems\" and skips it — it never orphans edges itself, including edges whose target repo no longer resolves (target_repo None yields target_version None, which produces no status change to Orphaned)."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "if !detected_keys.contains(&key) && edge.status != DependencyStatus::Orphaned {",
        "detail": "The only Orphaned transition in production: reconcile_dependencies marks an edge Orphaned when its declaration is no longer detected — but this runs only on a push to the SOURCE repo, and only for edges whose detection_method ran in that push, so target deletion is covered lazily at best and target archival never (the archived target still matches a known repo and is re-detected as present)."
      }
    ]
  }
]
