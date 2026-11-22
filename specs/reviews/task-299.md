# Review — task-299 (Scope MCP agents and queue resources to callers)

Spec: `specs/system/agent-runtime.md` §1 Phase 5 knowledge boundary (gap `de73e06638559fcca67b3c5d`).
Candidate under review: `c90c7f89` (branch `pipeline/task-299/06181dc8ad6e4723a4c93ec4e52c6605-2`, base `f0f2ba5d`; product commits `bd5f3a22` + rustfmt finalizer `33a88bfe`, both attributed in the task frontmatter).
Verdict: **approved** — no findings. Full probe evidence: `/tmp/stage/review-evidence/task-299-review.md`.

## Probes run (this sandbox)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (10m07s cold; type-checks the lib including the three new cfg(test) tests).
- `scripts/check-arch.sh`, `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`, `check-abac-exempt-handlers.sh` → all exit 0.
- Full diff read (3 files: mcp.rs, agent-runtime.md row 8, task-299.md); grep sweeps for other `agents://`/`queue://` consumers (none outside mcp.rs), `resource_definitions` call sites (one), and resources-read dispatch paths.
- `check-task-commit-attribution.sh` timed out in this sandbox (needs full history); attribution verified by hand.

## What was verified

1. **Worker-tier exclusion is mechanical and first.** `is_worker_tier` classifies exactly the task-scoped principal classes: `agent_task_claim().is_some()` (worker JWTs and gate-agent capability tokens — cross-checked against both mints: `mint` always sets `task_id` with `orchestrator_type: None`, `mint_orchestrator` always sets `orchestrator_type`, so every Gyre agent JWT is classified) plus legacy UUID agent tokens (`AgentToken` + no claims — no scope list to enforce, fail closed). For these principals `resources/list` omits `agents://`/`queue://` (retain filter, all other resources stay) and `resources/read` returns an error result for every URI spelling BEFORE any store access. No other MCP method or production surface serves these resources.
2. **Caller-scope enforcement replaces the unbounded reads.** Unscoped `agents://` resolves through `caller_agent_workspaces`: real workspace memberships (`workspace_memberships.list_by_user`; tenant-Admin rule mirrors `authorize_repo_access`), signed workspace claim for agent JWTs (none → error), system principal only via the non-spoofable `agent_id=="system" && user_id.is_none()` discriminator. The base's unfiltered `state.agents.list()` cross-workspace enumeration is gone (per-workspace `list_by_workspace` over the resolved set). `agents://{ws}` requires membership/tenant-admin/signed claim. `queue://{repo}` loads the repo, compares its real workspace against the caller's scope (unresolvable repo fails closed); agent JWTs go through the pre-existing id-precise `check_repo_read_target` (repo-tier: own repo; workspace-tier: signed workspace). Descriptions no longer promise "agents:// for all".
3. **Tests are real, not self-confirming.** All three drive `build_router` through the real auth extractor with real crypto (EdDSA worker/orchestrator mints; RS256 Keycloak JWT verified against jwt_config, user auto-provisioned, real membership row). Active agents seeded in BOTH workspaces so any leak is visible; each assertion fails at the base (unfiltered listing / advertisement / unconditional queue read). Pre-existing resource tests use the global dev token (system principal) and keep full access — no regression.
4. **Coverage honesty.** Row 8 updated with concrete source evidence and an explicit "Independent verification pending" disclosure; no unrelated rows touched; no exemption files modified; the rustfmt finalizer is whitespace-only on mcp.rs (verified hunk-by-hunk).

Non-blocking observations (recorded, not findings): legacy UUID and gate-agent tokens are worker-tier blocked (safe classification, no production consumer); denials are JSON-RPC error results inside HTTP 200 — the resource layer's established convention.

## Verifier must run

- `cargo test -p gyre-server mcp_worker_jwt_cannot_read_agents_or_queue_resources`
- `cargo test -p gyre-server mcp_agents_resource_scoped_to_member_workspaces`
- `cargo test -p gyre-server mcp_agents_and_queue_scoped_to_orchestrator_token`
- `cargo test -p gyre-server mcp_resources` (system-principal access retained)
- Full workspace suite + Clippy per standard verification.
