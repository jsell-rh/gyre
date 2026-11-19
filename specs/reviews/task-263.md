# Review — task-263 (Agent JWT claims completion and hard task-scope boundary)

Spec: `specs/system/identity-security.md` §Task-Scoped JWT Claims (Layer 2 + hard boundary + revocation).
Candidate under review: `d610b0a8` (branch `pipeline/task-263/fc4dd1b6c1c746378004ba304bf87767-2`, base `61b6080d`; product commit `f338cfc1`, merge `d9e41f1b`, format `37f458b9`, docs `ce5f02d5`/`d610b0a8`).
Verdict: **needs-revision** — one finding (F1, evidence integrity: a documented mint mechanism does not exist in the code). All enforcement surfaces, the revocation wiring, and the audience/persona/TTL claims verified clean by inspection and type check.

## Round 1

Probes run (checkout at candidate `d610b0a8`; working tree carries only the executor's protected-task-text restoration of `specs/tasks/task-263.md`, which is not part of the product diff):

- `/tmp/stage/bin/cargo check -p gyre-server` → **exit 0** (8m01s; `--all-targets` is rejected by the sandbox wrapper — test-target compilation deferred to the verifier; see "Verifier must run").
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-abac-route-registry.sh` → exit 0 (exemption file untouched by the diff — no grandfathering).
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- Full diff read of all 21 touched files; grep sweeps for task-mutating surfaces (`tasks.update`, `transition_status`, `assign_task`, MCP tool dispatch arms, `agent_tokens` writers, `sub`/`aud` consumers, federation paths).
- Evidence saved under `/tmp/stage/review-evidence/` (cargo-check log, mechanical-check logs, no-post-launch-remint grep).

Verified working (no findings):

- **Layer-2 claims mint.** `AgentJwtClaims` gains `aud` + `stack_hash` alias (`auth.rs`); every mint path (`mint_worker_scoped`, `mint_orchestrator`, generic `mint`) sets `sub = agent:<uuid>`, `aud` from the key's configured audience, and both stack-hash spellings from one fingerprint. `AgentSigningKey` is `Clone` and carries `audience` ('gyre' / `GYRE_OIDC_AUDIENCE` wired in `lib.rs` `build_state`); `validate()` uses `validation.set_audience` — audience validation is live for self-minted tokens, and the federated path (`validate_federated_jwt`) keeps `validate_aud = false` as required. Default `GYRE_AGENT_JWT_TTL` restored to 3600 s (`lib.rs`), docs updated. Spawn mint populates `persona` (validated `persona_slug`/bare persona), `wl_compute_target`, `wl_hostname` (SSH host), `attestation_level` — verified in `spawn_agent_core` and the new test `spawn_minted_jwt_carries_task_scoped_claims` (real route stack, real mint, decodes and asserts each claim). `sub` prefix is safe downstream: identity resolution keys off the `agent_tokens` KV entry, and all `format!("agent:{id}")` sites use the bare `auth.agent_id`, not the claim.
- **Audience kill test is real.** `agent_jwt_audience_validation_is_live` hand-encodes claims signed by the SAME key with wrong/absent `aud` and asserts `InvalidAudience` — if `validate_aud` were still false the `expect_err` fails.
- **Revocation.** `agent_audit::revoke_agent_token` writes the jti to the durable `revoked_tokens` SQL table (`SpawnLogRepository::revoke_token` — SQLite + Postgres pre-existing with adapter tests, mem adapter implements both methods in code) in addition to the KV removal; `auth.rs` `jti_revocation_denial` runs on both the REST extractor and the WS `authenticate_token` path BEFORE signature/expiry validation; `revoked_jti_rejects_token_before_expiry` proves 401-with-future-exp with the KV entry deliberately left intact (kills a removal of the jti check). All teardown paths (complete/stop/fail/admin kill/budget/gate) funnel through the one writer. Table has no delete path.
- **By-effect sweep is complete and per-call.** REST: `update_task`/`transition_task_status` take auth extractors + `require_task_claim` (403 on claim mismatch, full mutation set covered by `agent_jwt_cannot_mutate_foreign_task` — content update AND status transition of the foreign task, own-task still passes); `create_task` clamps/rejects targets against verified claims; `complete_agent` self-or-task-claim (mirrors the task text; MCP twin stays self-only); `stop_agent`/`fail_agent` self-only + tenant containment (routes are registry-exempt — per-handler authorization now present, `check-abac-exempt-handlers` passes); `update_agent_status` self-or-task-claim; `spawn_agent_core` requires own-task claim or an explicit `repo:{name}:spawn`/`workspace:{id}:spawn` scope for agent principals (policy-independent); `create_mr` (REST) and MCP `gyre_create_mr` check the claim's task repo (`check_agent_mr_task_scope`); compose apply/teardown and `archive_repo` deny task-bound agents + tenant containment. MCP: `gyre_update_task`/`gyre_decompose_spec` compare the `task_id` claim in `validate_tool_scope` before the tier check (PERMISSION_DENIED, foreign task unmutated — `mcp_agent_jwt_task_scope_on_task_and_mr_tools`). git push: `authorize_repo_access` requires agent JWTs to hold the target repo (name-matched `repo:{name}:write|spawn`) or covering workspace scope (`agent_push_outside_token_repo_scope_rejected` proves both the 403 and that an own-repo token passes). No task-mutating surface found outside the sweep (no REST task delete; signal-chain/reconciliation effects are server-internal; admin kill/reassign is role-gated).
- **Non-agent principals keep their rights.** Every new check gates on `agent_task_claim()` (worker-tier, `orchestrator_type`-exempt) or `is_gyre_agent_jwt()`; the lifecycle test asserts the operator's stop right and that the denied teardown did not touch the target agent's status, task, secrets, or MRs. Existing tests were only amended by adding `Authorization` headers / direct-call auth args — no assertion weakened (spot-checked all hunks); the gate-agent `sub` assertion was updated to the new specced `agent:<uuid>` form.
- **AC 6 consumer side.** Pre-existing e2e `agent_only_requires_exact_stack_hash_match_e2e` (specs.rs) drives the REST approval path with JWT-embedded `wl_stack_hash` and proves match/mismatch against a pinned requirement; the new producer-side test `respawn_mint_carries_stored_stack_fingerprint` proves a production mint (Ralph re-spawn) carries the stored fingerprint in both spellings.

Findings:

- [-] **F1 (evidence integrity / mint-side gap): the documented "post-launch re-mint" of the spawn JWT does not exist in the code, so a first-session agent JWT never carries `stack_hash`/`wl_stack_hash`.** The candidate's task text (`specs/tasks/task-263.md`, "Shipped → Layer 2 mint completion") states that `spawn_agent_core` "after the process launch, when the workload attestation record with the real stack fingerprint exists — re-mints the token with `wl_stack_hash`/`stack_hash`", and coverage row 4 of `specs/coverage/system/identity-security.md` repeats "post-launch re-mint + re-spawn mint". No such code exists — in the candidate or in the original product commit `f338cfc1` (grep evidence: `/tmp/stage/review-evidence/no-post-launch-remint.txt`): `spawn_agent_core` mints exactly once, BEFORE launch (`spawn.rs:949`, token stored at `:994`); `launch_agent_workload` writes `workload_attestations` but never touches `agent_tokens`; the only production re-mint is `ralph_loop.rs` `relaunch_agent_session` (a later Ralph iteration, not the same spawn); the stack self-report path (`api/stack_attest.rs`) never refreshes the token either. Consequence: for a fresh agent the token's stack-hash claims are absent for its entire first session — even after the agent self-reports its stack mid-session — so the spec-registry stack-pinning consumer's input is live only for Ralph re-spawned sessions (iteration ≥ 2). The candidate's own test `spawn_minted_jwt_carries_task_scoped_claims` asserts the claims are `None` for first spawns, directly contradicting the documented claim, and the task text's out-of-scope list covers `wl_pid`/`wl_container_id` but not this first-session stack-hash absence. AC 1's clause "decoding a spawn-minted agent JWT shows ... stack_hash and wl_stack_hash equal to the stored workload-attestation stack fingerprint" is therefore satisfied only on the re-spawn path.

  **Repair path (either is acceptable; no other production code needs to change):**
  (a) Implement the re-mint so the documentation becomes true — e.g. refresh the agent's token (same claims, `wl_stack_hash`/`stack_hash` from `stored_stack_fingerprint`) when a non-empty fingerprint first becomes available (post-launch when the attestation record carries one, and/or in the stack self-report path), updating the `agent_tokens` KV entry; or
  (b) Stand by the honest-absence reading and correct the evidence: rewrite the coverage-row-4 phrase and the Shipped note to the actual mechanism (single pre-launch mint reading any pre-existing stored fingerprint + Ralph re-spawn mint only), and add an explicit, justified out-of-scope statement for the first-session stack-hash absence to the task text (as the task's own sweep discipline requires for by-effect gaps).

  Either way the false "post-launch re-mint" claim must not survive into the published coverage matrix — the matrix is the canonical evidence artifact and must reflect reality (goal rule 3).

Minor (non-blocking, recorded for completeness):

- Federated agent JWTs (remote Gyre instances) now fall under the git-push scope check (`is_gyre_agent_jwt` is true for them — their tokens carry scope arrays). This is a tightening consistent with the spec's hard boundary ("an agent cannot write to a repo it was not assigned"), and scope entries match by repo NAME, so same-named local repos still pass; but cross-instance push flows that relied on the old unconditional allow will now 403 unless the names align. Documented in the `authorize_repo_access` comment; no spec text requires the old behavior.
- Legacy UUID agent tokens (pre-JWT, `deprecated_token_auth`) still pass `authorize_repo_access` unconditionally — pre-existing class, documented in-code; out of this task's claim-based boundary.

Test-inflation check: none found. Every new boundary test drives the real router (or the real MCP dispatch) with a minted, registered worker JWT and asserts 403/PERMISSION_DENIED plus non-mutation of the target (status, task, MR, secret) and a passing own-scope/own-task counterpart — each fails if its check is deleted. The audience and jti tests are same-key kill tests, not structural.

Verifier must run (test-target compilation exceeds this sandbox's budget; wrapper rejects `--all-targets`):

- `cargo test -p gyre-server agent_jwt` — auth.rs: `agent_jwt_mints_specced_task_scoped_claims`, `agent_jwt_audience_validation_is_live`, `revoked_jti_rejects_token_before_expiry`.
- `cargo test -p gyre-server agent_jwt_task_scope_boundary_on_lifecycle_endpoints agent_jwt_cannot_spawn_onto_foreign_task spawn_minted_jwt_carries_task_scoped_claims` (api::spawn::tests).
- `cargo test -p gyre-server agent_jwt_cannot_mutate_foreign_task agent_jwt_create_task_clamped_to_claims` (api::tasks::tests).
- `cargo test -p gyre-server mcp_agent_jwt_task_scope_on_task_and_mr_tools` (mcp::tests).
- `cargo test -p gyre-server agent_push_outside_token_repo_scope_rejected` (git_http::tests).
- `cargo test -p gyre-server agent_jwt_cannot_create_mr_outside_task_scope` (api::merge_requests::tests).
- `cargo test -p gyre-server respawn_mint_carries_stored_stack_fingerprint` (ralph_loop::tests).
- Pre-existing suites in every touched file (abac_middleware, agents, compose, discover, merge_requests, orchestrator, repos, spawn, tasks, constraint_check, gate_executor, rbac, mcp, git_http, auth, agent_audit) plus the spec-approval e2e (`agent_only_requires_exact_stack_hash_match_e2e`) — the ~54 test builders that gained `Authorization` headers must all still pass, proving non-agent principals are not locked out.
- Full workspace suite + Clippy per standard verification.

## Round 2

Candidate: `a1dde704e68c7038b3f715b980ea2899413371ab` (branch `pipeline/task-263/6cf050cd02334f8eacdb71fa0603a7d5-1`, base `4cfbe20d`; includes repair `356184ee` wl_hostname, merge rounds `07d01c55`/`050ef5e0` onto current main, test repairs `544b2755`/`585d459e`, admin-seed gate `5cdce3aa`).
Verdict: **approved** — round-1 F1 verified repaired; no findings. Evidence: `/tmp/stage/review-evidence/` (round2-summary.md, cargo-check log, mechanical-check logs, full base-relative diffs).

Probes run:

- Full diff read `4cfbe20d..a1dde704` (all 24 files; security hunks in detail).
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (8m23s).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh` (exemption file untouched), `check-abac-exempt-handlers.sh` (stop/fail now carry mandated per-handler authorization), `check-inert-enforcement.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh` → all exit 0.
- `cargo check -p gyre-server --all-targets` → rejected by the executor wrapper (exit 64); test-target compilation deferred to the verifier (infrastructure restriction, not a code defect). Verifier test list in round2-summary.md.
- By-effect sweep audit by grep: every production task-mutating site maps to a gated handler or a server-internal system-authority job; `reject_spec` denies Agent-role in-handler; no `revoked_tokens` delete path exists; all `AgentJwtClaims` constructions set `aud`/`agent:<uuid>` sub; `with_hierarchy_from_claims` mirrors verified claims onto the principal.

F1 disposition — **repaired on both axes**: the first-spawn mint resolves the session stack evidence once (`spiffe::resolve_session_stack_evidence`, spawn.rs:946) and binds the SAME value to the JWT `wl_stack_hash`/`stack_hash` claims (:955), the session SVID (launch ctx :1414), and the launch-written workload-attestation record (:1385) — `spawn_minted_jwt_carries_task_scoped_claims` asserts mint == stored record on the FIRST spawn; the coverage row and Shipped text now describe the actual single-mint mechanism (the phantom "post-launch re-mint" claim is gone). The re-spawn path is proven by `respawn_mint_carries_stored_stack_fingerprint`; the spec-registry consumer e2e (`agent_only_requires_exact_stack_hash_match_e2e`) is pre-existing and its producer input is now live on every session.

Repair `356184ee` verified: `wl_hostname` is minted from the COMPUTE TARGET (Ssh → configured remote host, Kubernetes → honestly absent, local/Container → local host) on both the spawn and Ralph re-spawn mints; `ssh_spawn_mints_configured_remote_host_as_wl_hostname` kills the forge-hostname regression (asserts `worker-box`, not the server hostname).

Enforcement, revocation, audience, TTL (3600 s), persona, sub/aud forms — re-verified clean this round by inspection + type check (unchanged in substance from the round-1 verification, plus the admin-surface gate `deny_task_bound_agent` whose test runs under a permissive priority-860 policy and asserts the 403 comes from the handler gate, not middleware).

Minor (non-blocking, recorded):

- `fail_agent` performs no token revocation (pre-existing at base; the task contract requires revocation on complete, and stop/kill/budget/gate all revoke through the one writer). A Failed agent's JWT stays valid until exp, confined to its own task by the new boundary. Candidate for a follow-up task. (This also corrects round 1's loose phrase "all teardown paths … funnel through the one writer" — the fail path does not revoke at all.)
- Legacy UUID agent tokens bypass the claim-based checks — pre-existing class, documented in-code.
- Federated agent JWTs now fall under the git-push scope check (tightening, documented).
- `jti_revocation_denial` falls through on a table-lookup error with a warning; the KV removal remains the live gate — documented in-code.

Test-inflation check: none. The repair-round commits touch test fixtures/names and spec-form assertions only (`544b2755` removes a hardcoded `"worker-1"` name; `585d459e`/gate_executor update `sub` assertions to the specced `agent:<uuid>`); no assertion weakened.
