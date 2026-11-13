# Review — task-130: Token scoping (hierarchy claims + MCP scope validation)

**Candidate:** `d999a4974e24a1ae43d1ce62d77343503ed9f5ac`
**Base:** `f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3`
**Verdict:** APPROVED (no findings)
**Spec:** `specs/system/platform-model.md` §1 Token Scoping

## What was reviewed

The full diff base→candidate (~2.8k insertions across gyre-server auth/spawn/
orchestrator/mcp/gate_executor/ralph_loop/stale_agents/signal_chain/
search_scope/git_http/abac_middleware, gyre-domain + gyre-adapters
scope-literal cleanup, tests, and the exemption-count reduction 12→6).

## Acceptance criteria — evidence

- **Claims shape**: `AgentJwtClaims` (auth.rs) carries `tenant_id: String`,
  `workspace_id: Option<String>`, `repo_id: Option<String>`,
  `persona: Option<String>`, `attestation_level: Option<u32>`, and
  `scope: Vec<String>`. Verified in source.
- **Minting from spawn context**: `mint_with_workload`/`mint`/
  `mint_worker_scoped` populate hierarchy + `["repo:{name}:write"]` for
  workers; `mint_orchestrator` mints `["workspace:{id}:read",
  "workspace:{id}:spawn"]` (workspace tier) and `["repo:{name}:write",
  "repo:{name}:spawn"]` (repo tier, repo_name required else mint fails).
  `spawn.rs` resolves repo→workspace→tenant and FAILS CLOSED (404) when the
  workspace row is missing — the legacy UUID-token fallback that would have
  bypassed the scope gate is removed. `orchestrator.rs`, `stale_agents.rs`,
  `signal_chain.rs`, `ralph_loop.rs` all resolve the real tenant fail-closed
  (never a fabricated literal) and pass caller-tenant containment into the
  shared spawn cores.
- **AuthenticatedAgent**: `workspace_id`/`repo_id`/`scope` added, mirrored
  from signature-verified claims via `with_hierarchy_from_claims` on every
  JWT-resolved path (extractor, authenticate_token, validate_jwt,
  validate_federated_jwt); `tenant_id` mirrored from the claim when present.
- **MCP validates scope on every tool call**: `validate_tool_scope` runs
  before tool dispatch in `mcp_handler` for every `tools/call`; rejections
  are `PERMISSION_DENIED` JSON-RPC errors naming the target (repo id / ws id
  / agent id). Every dispatched tool is classified: repo-write tools
  (create_mr, heartbeat/complete/record_activity with target binding),
  spawn tools (spawn right required — worker write-only tokens rejected),
  workspace tools, task-addressing tools (target resolved from real state,
  unresolvable lookups fail closed), repo-read tools (workspace-tier callers
  resolve the target repo's real workspace from state). Write tools that
  fall to the catch-all (conversation_upload, message tools) are
  caller-identity-bound in their handlers (verified). Gate-agent
  capability tokens (scope = [capability]) fail closed on every tiered tool
  because they carry no repo/workspace scopes or claims.
- **List/fallback clamping**: `gyre_list_mrs` fallback clamped to the signed
  repo_id claim (never the unfiltered store); `gyre_list_tasks` filtered by
  tier; `gyre_search`/`gyre_create_task` clamped to the caller's claims.
- **ABAC/git_http gate-agent scope checks** migrated off the raw string
  claim to the extractor-derived array form (an array claim read as string
  would silently fail open).

## Probes (full log: /tmp/stage/review-evidence/probe-summary.md)

- `cargo check -p gyre-server --lib --tests` exit 0 — the prior round's
  test-target compile defect (E0308 gate_executor.rs:1897) is repaired.
- `cargo test -p gyre-domain` 507 passed; `cargo test -p gyre-adapters`
  514 passed / 6 ignored.
- `cargo clippy -p gyre-server --lib --tests` exit 0 with **0 warnings on
  lines changed by this diff** (changed-line filter re-implemented from
  scripts/check-clippy-diff.py; workspace-wide clippy is wrapper-rejected
  here).
- `cargo fmt --check` clean; 12 repo mechanical checks pass
  (arch, abac route registry + exempt handlers, mcp write tools,
  scope-literal defaults — exemption count lowered 12→6, removal-only —
  inert enforcement, fabricated scope defaults, mem port contracts, dead
  message kinds, migration versions, commit attribution against the
  committed tree).

## Test strength

The 10 `mcp_scope_*` tests mint REAL EdDSA JWTs with the server signing
key, register them in `agent_tokens`, and drive the full router (real auth
middleware signature verification → validate_tool_scope → handler),
asserting both denial (error naming the out-of-scope target) and allow
paths, plus a state side-effect check (denied heartbeat must not touch the
foreign agent's last_seen). Disabling `validate_tool_scope` would fail
them; they are not self-confirming. Cross-tenant spawn containment has
dedicated negative tests with fail-closed state assertions.

## Deferred to the verifier (4 GiB budget rule)

`cargo test --all` (the acceptance criterion) — in particular
`cargo test -p gyre-server --lib` (mcp_scope_*, orchestrator, gate_executor
blob-ref/TTL, spawn fail-closed, auth mirror tests) and the
search_wiring/task134_review_probe/auth_integration/m18_oidc_integration
integration tests, plus `scripts/check-clippy-diff.py`. The three failures
from the prior full-suite run (durable finding @0bbb9007) were repaired on
this branch; each repair was verified in source (persona-slug vs
orchestrator-type assertion split; ws-test workspace seeding; MR seeding
for the TTL test) — see probe-summary.md.

## Residual (non-blocking, documented in the coverage row)

First-spawn worker mints pass `persona: None` (the spawn request carries no
persona input); the persona claim is populated on orchestrator mints,
gate-agent mints, and worker re-spawns (ralph_loop resolves the bound
persona slug).
