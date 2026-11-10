# Review — task-130: Implement token scoping (hierarchy claims + MCP validation)

Round: this round reviewed the rebase-merge repair onto base `b5a872087`
(task-108 admin user impersonation), candidate `cc62f8d56`
(merge `f69bd657a`, pre-merge task-130 head `80a20a720`).
Reviewer scope: `git diff b5a872087..cc62f8d56`, with the merge itself
inspected against both parents. Probe evidence under
`/tmp/stage/review-evidence/` (mcp-scope-tests.log, mutation-probe.log,
lib-tests-rerun.log, auth-integration-rerun.log, oidc-integration-rerun.log,
impersonation-tests.log, gate-*.log, clippy-changed-lines-count.txt).

## Verdict

Approved. `platform-model.md` §1 Token Scoping is implemented end-to-end with
real production code; the rebase onto task-108's impersonation base preserved
both sides' enforcement.

## Independent evidence (this review)

- **Claims struct** (auth.rs:53–107): `tenant_id: String` (serde default for
  legacy-token tolerance), `workspace_id`/`repo_id: Option<String>`,
  `persona`, `attestation_level`, and `scope: Vec<String>`. Gyre-minted tokens
  always populate workspace_id (worker/repo-orchestrator/repo claims, and
  workspace-only on workspace orchestrators); `repo_id: None` is the specced
  workspace-orchestrator shape.
- **Minting from real hierarchy**: `spawn.rs` resolves repo→workspace→tenant
  and fails the spawn closed when the workspace row is missing (404 before
  any agent record) or when the mint fails — no legacy unscoped fallback.
  `mint_orchestrator` mints `["workspace:{id}:read","workspace:{id}:spawn"]`
  / `["repo:{name}:write","repo:{name}:spawn"]` per tier; `stale_agents.rs`
  re-mints with the same resolution.
- **Claims mirrored onto the principal** (`with_hierarchy_from_claims`,
  auth.rs:711): runs on all four JWT-resolved paths; the merge commit's diff
  vs the pre-merge task-130 head shows only additions from base's
  impersonation machinery — no §1 lines removed.
- **MCP scope gate is load-bearing (mutation probe)**: with
  `validate_tool_scope` neutered to `return Ok(())`, 4/10 `mcp_scope_*` tests
  fail (worker-cannot-heartbeat-foreign-agent, worker-cannot-spawn,
  ws-orchestrator-list-mrs-requires-repo, ws-orchestrator-other-workspace).
  The `gyre_create_mr` tests survive the mutation via the independent
  TASK-216 worktree-repo check — defense in depth, still a real rejection,
  and the scope gate is independently pinned by the other four. Tests go
  through `build_router` with real EdDSA-minted tokens (register_worker
  resolves the workspace's tenant, mints `repo:{name}:write`, registers in
  `agent_tokens`) — not stub-backed. Probe source/output saved in
  mutation-probe.log; production file restored byte-identical after the probe.
- **Impersonation interplay (this round's rebase focus)**:
  `validate_impersonation_opt` requires the `impersonating` claim, so agent
  JWTs fall through to the agent-token path (asserted by
  `impersonation_jwt_round_trip_and_expiry`); `resolve_impersonation_auth`
  loads the active session, resolves the target's *stored* roles (not token
  claims), and carries empty §1 scope with `jwt_claims: None` — user
  principals stay ABAC/RBAC-governed and never satisfy `is_agent_jwt`.
- **Tenant containment**: `spawn_*_orchestrator_core` compares the loaded
  entity's tenant against the caller's (Forbidden on mismatch); a dangling
  workspace reference fails closed.
- **Tests**: `cargo test -p gyre-server --lib` 1631/0; `mcp_scope` 10/10;
  impersonation 15/15; `auth_integration` 21/21; `m18_oidc_integration` 8/8.
- **Static gates** (run at candidate HEAD): check-arch,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-inert-enforcement, check-abac-exempt-handlers, check-mcp-write-tools,
  check-task-commit-attribution — all exit 0. Exemption-file changes are
  comment/provenance merges plus base-driven line-number shifts; no entry
  growth. `cargo fmt --all --check` clean. Clippy
  (`-p gyre-server --lib --tests --all-features`, JSON): 0 warnings on lines
  changed vs base (21 pre-existing warnings on unchanged lines of changed
  files — same class the changed-lines gate ignores).

## Non-blocking observations

- Worker mint passes `persona`/`attestation_level` as `None` — no persona
  input exists on the spawn request and attestation is recorded post-spawn;
  the claims and mint parameters exist. Residual is documented in the
  coverage row and was accepted in prior rounds.
- `workspace_id` is `Option<String>` rather than the plan's required
  `String`: deliberate serde-default tolerance for pre-§1 tokens; every
  Gyre mint path populates it.
- Full workspace `cargo test --all` / all-target Clippy on every crate /
  exact-head CI remain with verification (per review-probe scoping; only
  gyre-server Rust files changed, and its lib+tests compile and pass).
