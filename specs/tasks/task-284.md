---
title: "AIBOM: dependency-graph snapshot and stale-dependency flags in release reports"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / aibom-stale-dependency-flags / ed9f34316476"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

The drift computation, daily dep_staleness_check job, Stale classification, and update auto-tasks are real and unchanged. This task closes the third bullet — the AIBOM flags stale dependencies in release reports — and the dependency-graph snapshot the audit homes in this section. build_release_aibom (crates/gyre-server/src/api/aibom.rs) gains a dependencies section built from the REAL dependency store at AIBOM build time: a snapshot of the released repo's outgoing DependencyEdges (target repo, dependency type, detection method, pinned version, target current version, version drift, status) plus its dependent count, with stale edges EXPLICITLY flagged as supply-chain risk (a stale/risk list naming each stale dependency with its drift and versions). The data comes from state.dependencies (list_by_repo) using the status/drift values the push-time pass and the daily job already maintain — no re-derivation drift between the graph and the AIBOM. The working-summary shape (AibomResponse) carries the stale-dependency flags (list + count) as well. The section is always present when the repo has edges (an all-current dependency set shows the edges with no stale flags — an omitted section would hide the snapshot), and a repo with no edges shows an empty snapshot with that stated. COORDINATION/PARTITION with active work: the staged supply-chain plan (plan_id b2576f0bb5cd13421ed650b13f3912dad0c8bb4f739145bc108b9e2fc2a0a571, task release-artifacts-aibom-sbom, status planned) wires AIBOM/SBOM generation into the release flow and also edits api/aibom.rs; THAT task owns per-release artifact generation, THIS task owns the dependency/stale CONTENT of the AIBOM shapes. The two compose: whichever lands second must preserve the other's fields; do not implement release-MR/tag/publish machinery here.

## Acceptance Criteria

- [ ] A repo with an edge whose status is Stale (drift exceeding the workspace max_version_drift): GET /api/v1/repos/:id/aibom?format=release&release={id} includes the dependencies section listing that edge flagged stale / as supply-chain risk, with its pinned version, current version, and drift.
- [ ] A repo whose edges are all current shows the dependency snapshot with no stale flags (section present, flags empty — not omitted).
- [ ] The flags track the live store: marking an edge stale (or resolving its drift) changes the next AIBOM build's output for the same release inputs.
- [ ] The working-summary AIBOM (default format) carries the stale-dependency list/count for the repo.

## Verification intent

Make repo A's dependency on repo B stale (A pins 1.2.0, B's latest tag is 1.5.0, drift over the workspace policy so the dep_staleness pass marks the edge Stale), then GET A's release AIBOM: pre-fix AibomReleaseResponse contains agents, commits, attestation chains, stack policy, spec registry, and summary — no dependency-graph snapshot and no stale-dependency flag anywhere (grep for stale/dependency content in api/aibom.rs returns nothing) — so the probe must fail until the dependencies section with the stale flag is built from the real DependencyRepository store.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "551185c9c05657eddf50405e",
    "decision": "gap",
    "requirement_quote": "- The AIBOM flags stale dependencies in release reports",
    "detail": "The drift computation, continuous monitoring, and auto-tasking are real: compute_version_drift derives semver minor-delta drift (major bumps weighted x10), latest_semver_tag resolves the target repo's current version from git tags, drift is applied both at push time and by the daily dep_staleness_check job (registered in jobs.rs at an 86400s interval), edges exceeding max_version_drift or the alert-days threshold are marked Stale, and 'Update {repo} dependency from {pinned} to {current}' auto-tasks are created when auto_create_update_tasks is set. The section stays open on its third bullet: the AIBOM does not flag stale dependencies. Both AIBOM shapes (working summary and format=release artifact) were inspected end-to-end — AibomResponse and AibomReleaseResponse contain agents, commits, attestation chains, stack policy, spec registry, and summary sections, with no dependency-graph snapshot and no stale-dependency flag; grep for stale/dependency content in api/aibom.rs and api/release.rs returns nothing.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/version_compute.rs",
        "quote": "pub fn compute_version_drift(pinned: &str, current: &str) -> Option<u32> {",
        "detail": "Real drift computation over parsed semver triples, used by both the push-time pass (git_http.rs) and the daily dep_staleness job."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "                name: \"dep_staleness_check\".to_string(),",
        "detail": "The daily background job (86400s interval) re-checks every edge for version drift and time-based staleness — the 'continuously monitors' requirement."
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "quote": "pub struct AibomReleaseResponse {",
        "detail": "Absence evidence: the release-artifact AIBOM struct's complete field list (aibom_version, release, generated_at, repo_id, agents, commits, stack_policy, spec_registry, summary) was inspected — no dependency-graph snapshot and no stale-dependency flag anywhere in the release report; the same holds for the summary format and api/release.rs."
      }
    ]
  }
]
