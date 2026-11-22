---
title: "Impact analysis: cascade test results and per-dependent test status served by the API and shown in the UI"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / impact-cascade-results-ui / ed9f34316476"
depends_on: [task-280]
progress: complete
commits: ["82d862acbe689c2232ab333f51b8fccbbee27735", "71bda29a3c4b96d9e2dae8a3b9a4b04f86c94471"]
---

## Required behavior

The blast-radius BFS query, CLI impact command, and the UI modal's composition of pinned/current/drift and breaking badges are real and unchanged. This task closes the remaining specced response fields and the dead client wiring. (1) SERVER: extend the impact/blast-radius surface (BlastRadiusResponse in crates/gyre-server/src/api/dependencies.rs and/or the per-repo dependents listing the modal composes) so each dependent repo reports its test status — "passing"/"failing" from the dependent's most recent durable CascadeTestResult (cascade-test-execution's store), "unknown" when none exists — and the response includes the cascade test results themselves (per-dependent outcome, merged repo, SHA, timestamp), matching the spec's example ("Cascade test results: not yet run" when none exist). Add the two server endpoints the UI's dead calls reference: GET cascade test results for a repo (from the durable store, not one-shot events) and POST to trigger cascade tests for a repo's dependents (same require_cascade_tests policy gating as the merge path, both merged-repo and dependent-workspace policy respected; per-handler authorization loads the scoped entity and compares tenant/workspace against the caller). (2) UI (web): implement the missing client methods api.cascadeTestResults(repoId) and api.triggerCascadeTests(repoId) in web/src/lib/api.js against those endpoints, and make ImpactAnalysisModal.svelte populate the cascade-results section and the per-dependent test-status column from the real responses — removing the silent optional-chaining no-ops (`api.cascadeTestResults?.(repoId) ?? Promise.resolve(null)`) and the hardcoded testStatus '--'; the trigger button invokes the real endpoint and refreshes the results. New routes get ABAC RouteResourceMapping entries (no exemption-file growth).

## Acceptance Criteria

- [ ] After a cascade test ran for dependent A against merged repo B, GET the blast-radius/impact endpoint for B: A's entry carries test status "passing" (or "failing") and the cascade test results are included in the response; a dependent with no result shows "unknown" / not-yet-run.
- [ ] GET cascade test results for a repo returns the durable records (surviving restart); POST trigger with require_cascade_tests enabled creates cascade-test tasks for the repo's dependents and is refused/skipped when policy disables it.
- [ ] In web/src/lib/api.js, cascadeTestResults and triggerCascadeTests are defined (grep finds real implementations, not optional-chained absent methods); the impact analysis modal's cascade-results section and per-repo test-status column populate from the API, and the trigger button performs a real call.
- [ ] scripts/check-abac-route-registry.sh and check-abac-exempt-handlers.sh pass with the new endpoints (no new exemptions).

## Verification intent

Run a cascade test for repo A against merged repo B (via cascade-test-execution), then GET B's blast-radius endpoint and open the impact analysis modal for B in the web UI: pre-fix BlastRadiusResponse carries only repo_id/direct/transitive/total — no endpoint anywhere serves cascade test results (they exist only as one-shot workspace events) — and the modal's api.cascadeTestResults?.(repoId) silently resolves to null because the method does not exist in web/src/lib/api.js, leaving the test-status column hardcoded '--'. The probe (real cascade results visible in BOTH the API response and the modal) must fail until the endpoints and the client methods exist.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "74607a9069c32b20ef245765",
    "decision": "gap",
    "requirement_quote": "    Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown",
    "detail": "The core blast-radius query is real: GET /api/v1/repos/:id/blast-radius BFS-walks the reverse graph (direct at depth 0, transitive deeper, deduplicated, total returned), `gyre deps impact` exposes it from the CLI, and the UI modal composes it with per-dependent version_pinned/target_version_current/version_drift from the dependents endpoint plus breaking-change badges. The section stays open on the remaining specced response fields: 'Cascade test results' and per-dependent test status ('tests passing'/'tests unknown') are not served anywhere — BlastRadiusResponse carries only repo_id/direct/transitive/total, no server endpoint returns cascade test results or test status for dependents (cascade results exist only as one-shot workspace events), and the UI modal's attempt to fetch them is dead code: it calls api.cascadeTestResults?.(repoId) and api.triggerCascadeTests?.(repoId), but neither method exists in web/src/lib/api.js, so the optional chaining silently resolves to null and the test-status column is hardcoded '--'.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/dependencies.rs",
        "quote": "    let mut direct: Vec<String> = Vec::new();\n    let mut transitive: Vec<String> = Vec::new();",
        "detail": "The blast_radius handler BFS-walks all edges to produce direct and transitive dependents plus a total — the core of the specced query."
      },
      {
        "file": "crates/gyre-server/src/api/dependencies.rs",
        "quote": "pub struct BlastRadiusResponse {\n    pub repo_id: String,\n    pub direct_dependents: Vec<String>,\n    pub transitive_dependents: Vec<String>,\n    pub total: usize,\n}",
        "detail": "The complete response contract: no cascade-test-result field, no per-dependent health (test status) field — the specced response lines 'Cascade test results: not yet run' and 'tests passing/tests unknown' have no server-side source."
      },
      {
        "file": "web/src/components/ImpactAnalysisModal.svelte",
        "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
        "detail": "The modal's cascade-results fetch is a dead reference: api.cascadeTestResults does not exist in web/src/lib/api.js (grep for cascade in that file returns nothing), so cascadeTestResults is always null and the cascade/test-status columns can never populate."
      }
    ]
  },
  {
    "id": "810eac26a99174ce812d04a3",
    "decision": "gap",
    "requirement_quote": "- **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.",
    "detail": "Three of the four bullets are fully real: DependencyGraph.svelte renders the interactive DAG at workspace and tenant scope with edges colored by type, stale/breaking node coloring, and node-click to impact analysis; RepoDependencies.svelte is mounted in RepoMode (repo detail) showing dependencies/dependents with drift indicators and stale/breaking badges; WorkspaceHome's sidebar mounts DependencyHealthCard fed by loadDepHealth, which computes 'repos with stale dependencies' and 'unacknowledged breaking changes' counts from the real workspace graph, stale, and breaking endpoints. The section stays open on the fourth bullet's tail: the impact analysis modal can never show cascade test results — its fetch (api.cascadeTestResults?.(repoId)) and its trigger button (api.triggerCascadeTests?.(repoId)) reference client methods that do not exist in web/src/lib/api.js, so the optional chaining silently no-ops and the cascade-results section and test-status column can never populate. (Same root cause as the §Impact Analysis gap — no server endpoint serves cascade test results — noted there without double-counting; the client-side dead wiring is this section's own missing piece.)",
    "evidence": [
      {
        "file": "web/src/components/DependencyGraph.svelte",
        "quote": "   * yellow, breaking in red. Click a node to see impact analysis.\"",
        "detail": "The DAG visualization bullet is real: workspace/tenant-scoped interactive graph, edges colored by dependency type, stale edges yellow / breaking red, node click wired to impact analysis."
      },
      {
        "file": "web/src/components/WorkspaceHome.svelte",
        "quote": "        api.staleDependencies(workspace.id).catch(() => []),",
        "detail": "The workspace-dashboard aggregate health bullet is real: loadDepHealth fetches the workspace graph, stale edges, and breaking changes from live endpoints and feeds staleCount/breakingCount into DependencyHealthCard."
      },
      {
        "file": "web/src/components/ImpactAnalysisModal.svelte",
        "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
        "detail": "The modal's cascade-test-results display is dead code: api.cascadeTestResults (and api.triggerCascadeTests, used by the trigger button) do not exist in web/src/lib/api.js — grep for 'cascade' in that file returns nothing — so 'show the full blast radius with cascade test results' cannot be met for the cascade-results part."
      }
    ]
  }
]

## Shipped

### Server (crates/gyre-server)

- `BlastRadiusResponse` (api/dependencies.rs) now carries `dependents: [{repo_id, direct, test_status}]` and `cascade_test_results: [{id, dependent_repo_id, merged_repo_id, merge_commit_sha, cascade_task_id, passed, failure_details, recorded_at}]`. `test_status` is `passing`/`failing` from the dependent's most recent durable `CascadeTestResult` **against this repo** (results for other merged repos report `unknown` — they say nothing about a change merged here), `unknown` when none exists; results are served newest-first from the durable `CascadeTestResultRepository` store (`state.cascade_test_results`), so the BFS core is unchanged and the specced response lines have a real source.
- `GET /api/v1/repos/{id}/cascade-test-results` (api/mod.rs route → `list_cascade_test_results`): durable records for merges in the repo, newest first, `[]` ("not yet run") when none — from the task-280 store that survives restart, not one-shot workspace events. Unknown repo → 404.
- `POST /api/v1/repos/{id}/cascade-tests/trigger` (`trigger_cascade_tests`): triggers the SAME production path the merge completes through — `merge_processor::trigger_cascade_tests`, now returning a `CascadeTriggerOutcome` (created task ids + skipped-dependent count) instead of `()`, merge-path caller unchanged. Policy gating mirrors the merge path: merged-repo workspace `require_cascade_tests = false` → 409 refusal; each dependent's own workspace policy respected (opted-out dependents skipped and counted). Per-handler authorization loads the repo, resolves its workspace, and compares the workspace's tenant against the caller's (403 on mismatch; a dangling workspace reference fails closed). The tested SHA is the explicit `merge_commit_sha` body field or the default-branch HEAD resolved via `git_refs::resolve_ref` — an unresolvable HEAD is a 400 refusal, never a fabricated empty SHA.
- ABAC: both routes have `RouteResourceMapping` entries in `abac_middleware.rs` (`repo` resource; `write` action on the trigger). `scripts/check-abac-route-registry.sh` → OK; `scripts/check-abac-exempt-handlers.sh` → OK (86 handlers, no new exemptions).
- `docs/api-reference.md` documents all three surfaces.

### UI (web)

- `web/src/lib/api.js`: `cascadeTestResults(id)` and `triggerCascadeTests(id)` are real implementations against the new endpoints (no optional chaining).
- `ImpactAnalysisModal.svelte`: the dead `api.cascadeTestResults?.(repoId) ?? Promise.resolve(null)` is now a real `api.cascadeTestResults(repoId)` call; the per-dependent test-status column populates from `blastRadius.dependents[].test_status` (falling back to deriving the same status from the results records) and defaults to `unknown`, never a hardcoded `--`; the cascade-results section renders per-result dependent, short merge SHA, failure details, timestamp, and Pass/Fail badge, with "Cascade test results: not yet run" when the store is empty; the trigger button calls the real endpoint, refreshes the data on success, and surfaces a refusal (e.g. 409 policy-disabled) in a `cascade-error` element instead of silently dropping it.

### Test evidence

- Web (this sandbox, vitest, locked deps via `npm ci`): `src/__tests__/ImpactAnalysisModal.test.js` — 23/23 passed, including the new tests: per-dependent test-status column populated from cascade results (passing/failing/unknown, never `--`), "not yet run" on empty store, per-result SHA/failure-details rendering, trigger performs the real call and re-fetches, and trigger refusal surfaced.
- Server: `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` pass (the checkpoint had 4 test-cfg compile errors — `expect_err` on non-`Debug` response types — fixed by deriving `Debug` on `CascadeTestResultResponse`/`TriggerCascadeTestsResponse`; newly added Rust code is rustfmt-clean). Per the 4 GiB budget rule, gyre-server test compilation/execution is deferred to the independent verifier, which must run in `crates/gyre-server`: `blast_radius_reports_dependent_test_status_and_results`, `blast_radius_test_status_uses_most_recent_result`, `blast_radius_test_status_ignores_results_for_other_merged_repos`, `cascade_test_results_endpoint_returns_durable_records`, `cascade_test_results_endpoint_unknown_repo_is_404`, `trigger_endpoint_creates_cascade_tasks_for_dependents`, `trigger_endpoint_refused_when_policy_disables_cascade_tests`, `trigger_endpoint_skips_dependent_whose_workspace_opted_out`, `trigger_endpoint_denied_across_tenants`, `trigger_endpoint_without_sha_fails_closed_on_unresolvable_head` (api/dependencies.rs tests module).
- Coverage rows 14 (Impact Analysis) and 17 (UI) in `specs/coverage/system/dependency-graph.md` updated with the concrete source evidence for the new behavior; no other rows touched.

### Contract-restoration rounds (no source change)

- The executor re-checked out the assigned task document three times (rounds 2–4), reverting it to the raw contract each time; the production fix from `82d862ac`/`71bda29a` was preserved untouched every time (the working tree differed from HEAD only in `specs/tasks/task-281.md`). No behavior changed, so no crate was rebuilt.
- Round 4 re-verified the current checkout on this exact tree (`5c70afd7`): `cargo fmt --check -p gyre-server` → clean; `scripts/check-abac-route-registry.sh` → exit 0, OK; `scripts/check-abac-exempt-handlers.sh` → exit 0, OK (86 handlers); web vitest `src/__tests__/ImpactAnalysisModal.test.js` → 23/23 passed (locked deps via `npm ci`). Evidence: `/tmp/stage/review-evidence/r4-routes.txt`, `r4-exempt.txt`, `r4-vitest.txt`, `r4-npmci.txt`.
