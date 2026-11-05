# Task-130 Review — candidate 31792de1 (round 5)

**Task:** Implement token scoping: hierarchy claims in `AgentJwtClaims` + MCP scope
validation (platform-model.md §1 Token Scoping).
**Candidate:** `31792de1fbc8eb28bf42693bdf3b91415b955413` (base `d81b0fe4`).
**Verdict:** NOT APPROVED — 1 finding (candidate-caused gate regression + false
shipped-evidence claim). All §1 functional work verified correct by independent probes.

## What was verified (independent evidence, /tmp/stage/review-evidence/)

- **Claims/minting**: `AgentJwtClaims` carries tenant_id, workspace_id, repo_id,
  persona, attestation_level, `scope: Vec<String>`; workers mint `["repo:{name}:write"]`,
  workspace orchestrators `["workspace:{ws}:read","workspace:{ws}:spawn"]`, repo
  orchestrators `["repo:{name}:write","repo:{name}:spawn"]`, all resolved from real
  hierarchy (repo → workspace → tenant). `AuthenticatedAgent` mirrors workspace_id/
  repo_id/scope from signature-verified claims on every JWT-resolved path.
- **MCP scope gate**: `validate_tool_scope` runs pre-dispatch on every `tools/call` for
  scoped tokens. Six discriminating probe tests (temporary, restored after run) all
  PASS at the candidate: `gyre_update_task` by task id on a foreign repo → PERMISSION_DENIED
  with store read-back unchanged; `gyre_search` with foreign workspace_id → denied;
  `gyre_list_tasks` filtered to the caller's tier; `gyre_create_task` foreign-repo target
  denied and absent targets clamped to the caller's claims; `gyre_decompose_spec`
  foreign-parent denied. All prior-round MCP findings (d87848ae, 240553e8) confirmed fixed.
- **Fail-closed spawn repair (this round)**: `spawn_agent_core` returns 404 before any
  agent record exists when the repo's workspace row is missing, and 500 on mint failure —
  the legacy UUID fallback that bypassed the §1 gate is gone. The replacement test
  `spawn_unresolvable_workspace_fails_closed` is discriminating (asserts 404 naming
  ws-ghost, no agent record, no env dump).
- **Tests**: `cargo test -p gyre-server` (lib + all integration binaries) fully green at
  the candidate: lib 1262/0; **m18_oidc_integration 8/8** (the 7be3363d/4e8e1b2
  regression is fixed; fixtures now create real workspaces; §1 claims asserted).
- **Static gates**: all CI-enforced gates pass (arch, route-registry, migrations,
  byte-slice, dead-message-kinds, hierarchy, sandbox-sweep, sql-portability), as do the
  advisory gates the diff touches (mcp-write-tools, fabricated-scope, lossy-secret,
  inert-enforcement, relative-path, scope-literal, mem-port-contracts, task-attribution).

## Finding (blocks approval)

**F1 — `check-abac-exempt-handlers.sh` regressed by this diff (passes at base, fails at
candidate).** The orchestrator.rs insertions shifted `spawn_workspace_orchestrator`
(208→260) and `spawn_repo_orchestrator` (304→364) past their line-anchored entries in
`scripts/abac-exempt-handlers-exemptions.txt`, which was not re-pointed — although the
same diff re-pointed four other exemption files for identical line drift. Result: the
check reports 2 violations at the candidate (exit 1) vs 0 at base (exit 0, verified in a
clean worktree at d81b0fe4). The gate is advisory in CI (`|| true`) but is an AGENTS.md
invariant, and the exemption file's contract is that the check runs green on main. This
also falsifies the shipped claim that all non-listed failing gates "fail identically at
the pre-change base". The underlying authorization hazard is pre-existing and owned by
task-093 (not this task's scope) — the candidate-caused defect is the gate regression
plus the inaccurate evidence claim. Remedy: re-point the two entries to :260/:364
(cheap), or land the tenant containment the exemption file describes (better).

## Non-blocking notes

- Disclosed residual stands: `persona`/`attestation_level` are accepted by
  `mint_with_workload` but no production caller populates them (spawn request carries no
  persona input; attestation recorded post-spawn). Same residual accepted in prior rounds.
- `gyre_analytics_query` / messaging tools remain in the default (allowed) arm for scoped
  callers; mcp.rs documents the per-tool read-surface rationale inline (the inline comment
  points at the coverage row for the justification, which is slightly stale — the row does
  not repeat it). Pre-existing class, messaging enforced same-workspace in the handler.
- assertless/self-confirming/mirrored/dead-test gate scripts exit 2 identically at base
  and candidate (environment/script issue, not a candidate finding).
- Known scim_delete_user parallelism flake passed in this full run.

Evidence: `/tmp/stage/review-evidence/` (summary.md, full-server-output.txt,
probe-output.txt, gate-*.log incl. base-vs-candidate differential for F1).
