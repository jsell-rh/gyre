# Review — task-130: Token Scoping (platform-model.md §1)

Candidate: `80a20a720cb364e644fcb6a5c18d01069630e51b` (head of
`pipeline/task-130/d3f574c85f2e44ec86e915ccf0e14676-1`), base `774737638`.

**Verdict: approved.** Independent re-review of the rebase-repair round; all §1
behavior re-verified against the merged tree, not the executor's labels.

## Evidence (logs under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib mcp_scope`: 10/10 — worker cross-repo MR
  denied + own-repo allowed, no-arg `gyre_list_mrs` clamped to the signed repo,
  workspace-tier forced to pass `repository_id`, foreign-agent heartbeat denied
  (foreign `last_heartbeat` untouched), worker cannot spawn, workspace
  orchestrator cannot repo-write, other-workspace denied naming the target,
  global token unaffected.
- `cargo test -p gyre-server --lib`: 1541/0. Integration: auth_integration 21/21,
  m18_oidc_integration 8/8 (§1 claims asserted), e2e_ralph_loop 1/1.
- Static gates all exit 0: check-arch, check-mcp-write-tools,
  check-scope-literal-defaults, check-inert-enforcement,
  check-fabricated-scope-defaults, check-abac-exempt-handlers,
  check-forged-scope-fields. Exemption files carry no new entries.
- `check-mcp-wrapper-parity.sh` fails with the identical 22-site set as base
  `774737638` (verified in a detached temp worktree, site sets diff-identical) —
  pre-existing, not a regression of this candidate.

## Structural checks

- `validate_tool_scope` runs at the single `tools/call` dispatch before any
  handler (mcp.rs:3982); `mcp_sse_handler` is broadcast-only — no dispatch
  bypass. Mismatch returns a `PERMISSION_DENIED` JSON-RPC error naming the
  target.
- Scope decisions resolve real state (task/repo/agent lookups) and fail closed
  on unresolvable targets and store errors; ambiguous multi-repo/multi-workspace
  scope lists are rejected.
- `AgentJwtClaims` carries `tenant_id`, `workspace_id`, `repo_id`, `persona`,
  `attestation_level`, `scope: Vec<String>`; worker scope
  `repo:{name}:write`, workspace tier `workspace:{id}:read|spawn`, repo tier
  `repo:{name}:write|spawn`. `AuthenticatedAgent` mirrors
  `tenant_id`/`workspace_id`/`repo_id`/`scope` from signature-verified claims on
  every JWT-resolved path (`with_hierarchy_from_claims`).
- Spawn paths mint from real hierarchy (repo→workspace→tenant) and fail closed:
  missing workspace row → 404 before any agent record; mint failure → 500 (the
  legacy UUID-token fallback that would bypass the scope gate is gone);
  orchestrator cores enforce tenant containment; stale-agent re-mint skips
  rather than fabricating a tenant identity.

## Non-blocking observations

1. Worker mint passes `persona`/`attestation_level` as `None` (no spawn-time
   persona input exists; attestation is recorded post-spawn). Fields and mint
   parameters exist; honestly disclosed as a residual in the coverage row.
2. Workspace-orchestrator JWTs are now denied the MCP worker-tier tools
   (heartbeat/complete/record_activity carry no workspace-tier right). The
   shipped container runtime heartbeats and completes via the REST endpoints
   (`docker/gyre-agent/entrypoint.sh:76`, `agent-runner.mjs:377`), so no runtime
   regression; consistent with `workspace:{id}:read|spawn` tier semantics.

Full workspace suite, all-target Clippy, and exact-head CI remain with
verification/publication.
