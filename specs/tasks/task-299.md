---
title: "Scope MCP agents and queue resources to callers"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / mcp-resource-knowledge-boundary / 4a3f940495a8"
depends_on: []
progress: ready-for-review
commits: ["33a88bfe67d433fafec0558864745696441d8034", "bd5f3a22c761155c05954e5401cea4eabbfbadcd"]
---

## Required behavior

Per agent-runtime.md §1 Phase 5: 'The agent has no knowledge of other agents, the merge queue, or the broader system state.' (1) Worker agents must not be able to read agents:// or queue:// at all: the MCP resources/list advertisement must omit these resources for worker-tier (task-scoped worker JWT) principals and resources/read must return an error result for them — the boundary is mechanical, not prompt-level. (2) For principals who may read them, per-handler caller-scope enforcement replaces the current unbounded reads in crates/gyre-server/src/mcp.rs: an unscoped agents:// read resolves against the caller's accessible workspaces (a member of workspace A never receives workspace B agents — no cross-workspace enumeration); agents://{workspace_id} requires the caller to have access to that workspace; queue://{repo_id} requires the caller to have access to that repo (load the repo, compare its workspace against the caller's scope, Forbidden on mismatch — per the exempt-handler authorization rule). Orchestrator principals are confined to their JWT's workspace/repo scope. The advertised resource descriptions are updated so they no longer promise 'agents:// for all'.

## Acceptance Criteria

- [ ] A worker agent JWT issuing MCP resources/read for agents:// and for queue://{any repo} receives an error result, and its resources/list contains neither resource.
- [ ] A user who is a member of only workspace A performing an unscoped agents:// read sees only workspace A agents even while workspace B has active agents; reading queue://{repo of workspace B} returns Forbidden for that user.
- [ ] A workspace-scoped orchestrator reading agents:// sees only its own workspace's agents.

## Verification intent

Create active agents in two workspaces, authenticate as a plain member of workspace A, and issue MCP resources/read with uri 'agents://' — at the base the handler calls state.agents.list() with no caller-scope filter and returns every Active/Idle agent across ALL workspaces (a cross-workspace enumeration leak, with the resource advertised as 'agents:// for all'), so workspace B agent ids appear in the response and the probe fails; the same unscoped dispatch also lets any worker agent read the merge queue, violating the Phase-5 knowledge boundary mechanically.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "de73e06638559fcca67b3c5d",
    "decision": "gap",
    "requirement_quote": "The agent has no knowledge of other agents, the merge queue, or the broader system state. It sees its task, its spec, and its repo.",
    "detail": "The worker workflow itself is real: the agent runner reads its task via MCP, edits/commits/pushes, and signals completion (gyre_agent_complete MCP tool and the REST twin both create the completion MR). But the normative knowledge boundary is mechanically violated, not just prompt-level: the MCP resources/read layer advertises and serves 'Workspace Agents' (agents://) and 'Merge Queue' (queue://) resources to every authenticated MCP session with no per-role or per-scope gate at the dispatch. An unscoped agents:// read lists ALL Active/Idle agents across ALL workspaces (state.agents.list() with no caller-scope filter — a cross-workspace enumeration leak), and queue://{repo_id} returns any repository's merge-queue entries. A worker agent can therefore acquire exactly the knowledge of other agents and the merge queue that this section forbids. No spec amendment authorizes prompt-level-only isolation. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"description\": \"List active agents in a workspace. URI: agents://{workspace_id} or agents:// for all.\",",
        "detail": "resources/list advertises the agents:// and queue:// resources to every MCP session including worker agent tokens — the resource layer itself offers the forbidden knowledge."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "let agents_result = if workspace_id.is_empty() || workspace_id == \"*\" {",
        "detail": "The unscoped agents:// read falls through to state.agents.list() with no caller-scope filter — a worker JWT can enumerate Active/Idle agents across every workspace, far beyond 'its task, its spec, and its repo'."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "let result = handle_resource_read(&state, &auth, uri).await;",
        "detail": "The resources/read dispatch calls handle_resource_read with no role gate, so worker-scoped JWTs are served the agents:// and queue:// resources like any other caller."
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "} else if let Some(repo_id) = uri.strip_prefix(\"queue://\") {",
        "detail": "queue://{repo_id} returns merge-queue entries for any repository the caller names — the merge-queue state the spec says a worker agent must not see."
      }
    ]
  }
]

## Shipped

Commit `bd5f3a22c` (checkpoint implementation, this branch) plus the
finalizer commit on top of it enforce the agent-runtime.md §1 Phase-5
knowledge boundary mechanically at the MCP resource layer in
`crates/gyre-server/src/mcp.rs`.

### Worker-tier exclusion (mechanical, not prompt-level)

`is_worker_tier` (mcp.rs) identifies task-scoped worker principals —
Gyre agent JWTs carrying a `task_id` claim without `orchestrator_type`
(worker tokens and gate-agent capability tokens, via
`AuthenticatedAgent::agent_task_claim`), plus legacy UUID per-agent
tokens (`AgentToken` mechanism with no JWT claims). For these
principals `resource_definitions` omits the `agents://` and `queue://`
resources from `resources/list`, and `handle_resource_read` returns an
error result ("forbidden: worker-tier principals cannot read …") for
every `agents://` and `queue://` URI spelling before any store is
touched.

### Caller-scope enforcement for permitted principals

- Unscoped `agents://` (and `agents://*`) resolves through
  `caller_agent_workspaces`: the system principal keeps the unfiltered
  listing; Gyre agent JWTs get their signed `workspace_id` claim (none →
  fail closed); user principals get their real workspace memberships
  (`workspace_memberships.list_by_user`; tenant Admins get every
  workspace of their tenant, the same rule `authorize_repo_access`
  applies); other agents get their signed workspace claim or an error.
  The unfiltered `state.agents.list()` cross-workspace enumeration is
  gone — the listing is per-workspace via `agents.list_by_workspace`.
- `agents://{workspace_id}` requires `caller_can_read_workspace`:
  membership lookup (or tenant-admin), signed workspace claim for agent
  principals, deny otherwise.
- `queue://{repo_id}` requires `caller_can_read_queue`: load the repo,
  compare its real workspace against the caller's scope, Forbidden on
  mismatch (unresolvable repo fails closed); agent-JWT principals go
  through the id-precise token-scope check (`check_repo_read_target`) —
  a repo-tier token only its own repo, a workspace-tier token any repo
  of its signed workspace.
- The advertised descriptions no longer promise "agents:// for all":
  `agents://{workspace_id}; an unscoped read resolves against the
  caller's accessible workspaces only`, and `queue://{repo_id}` states
  the caller must have access to the repo's workspace.

### Test evidence

Three focused tests in the gyre-server lib test binary map one-to-one
onto the acceptance criteria:
`mcp_worker_jwt_cannot_read_agents_or_queue_resources` (AC1 —
resources/list omits both resources, resources/read errors on all URI
spellings, Active agents seeded in both workspaces so any leak is
visible),
`mcp_agents_resource_scoped_to_member_workspaces` (AC2 — plain
workspace-A member with a signed Keycloak JWT and a real membership row
sees only ws-a agents while ws-b has an Active agent;
`agents://ws-b` and `queue://{ws-b repo}` forbidden),
`mcp_agents_and_queue_scoped_to_orchestrator_token` (AC3 —
workspace-scoped orchestrator sees only its own workspace's agents;
queue reads confined per orchestrator tier).

`cargo check -p gyre-server --tests`: clean (type-checks the lib
including the cfg(test) targets). `cargo fmt -p gyre-server`: clean
(finalizer round reformatted the checkpoint's new code — whitespace
only, behavior preserved). `check-mcp-write-tools`,
`check-inert-enforcement`, `check-arch`: OK. Executing the gyre-server
test binary is deferred to the independent verifier per the
build-budget rule (probe commands and per-test acceptance mapping
recorded in /tmp/stage/review-evidence/task-299-finalizer.md).
