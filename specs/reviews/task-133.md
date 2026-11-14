# Review — task-133 (ABAC MCP permission resources for agents)

Spec: `specs/system/abac-policy-engine.md` §MCP Integration.
Candidate: `34c1838a01cdd81edd5b6d968b6e5d01d583184e` (base `5984177c`).
Product commits: `cd372a6c` (implementation), `1b63ee4e` (test-attribution repair after the first verification round).
Verdict: **approved** — every acceptance criterion is met by real production code; no findings.

## What was reviewed

Diff `5984177c..34c1838a`: `mcp.rs` (+528: two resources, context builder, read-handler branch, 4 tests), `abac_middleware.rs` (+36: `distinct_resource_types`/`all_resource_types` exposure, two `pub(crate)` visibility widenings), `docs/api-reference.md`, coverage matrix row 18, task file.

## Verification of each acceptance criterion

- **`permissions://me` returns effective permissions** — `resource_permissions_me` builds the CALLER's `AttributeContext` (`agent_permission_context`) and runs the real task-129 engine (`policy_engine::evaluate`) for every canonical resource type × the spec's §Attributes action vocabulary (read/write/delete/approve/spawn/push/merge/escalate/generate), returning `[{resource_type, allowed_actions: []}]` exactly as the plan specifies. The resource-type vocabulary is derived from the live route registry (`ResourceResolver::distinct_resource_types`, filters `exempt` so the `"exempt"` pseudo-type never appears), so the probe set cannot drift from what the middleware enforces.
- **`permissions://check?action=push&resource=repo` pre-flight** — parses both query params (missing param → explicit usage error, not a silent deny), single real evaluation, returns `{allowed, matched_policy}`.
- **Both listed in resources/list** — `resource_definitions()` registers both, `permissions://check` with the correct `uriTemplate`; covered by `mcp_resources_list_includes_permission_resources`.
- **Correct results from real ABAC evaluation** — `permissions_resources_evaluate_real_policies_for_agent_jwt` drives the real router with a Gyre-minted worker JWT (`register_worker`) against the production-parity seeded policy set (`mem.rs` `shared_workspace_policy_pair_seeded` = the exact `seed_builtin_policies` set from `main.rs`): read-on-repo → allowed by `builtin-agent-scoped-access` (real priority-700 role Allow, verified in `m34_builtin_policies`) with that exact matched_policy id; approve-on-spec → denied by the immutable `builtin:require-human-spec-approval` (Deny-beats-Allow actually exercised). Exact-value matched_policy assertions — not self-confirming.
- **Own-permissions-only** — there is no subject parameter on either resource; the subject context is derived exclusively from the caller's `AuthenticatedAgent` with the same extraction discipline as the middleware (subject.type/id/tenant_id/global_role with role-SET semantics and the reserved `SYSTEM_ABAC_IDENTITY`, agent-only allowlisted JWT claims via `merge_agent_jwt_claims` + `normalize_agent_jwt_attributes`, membership/team stores for user identities; verified side-by-side against `abac_middleware.rs` lines 2199-2260). `permissions_check_is_caller_scoped` is a genuine differential test: two real signed JWTs, one allowed push-on-repo by a `subject.id`-keyed custom Allow (matched pinned to it), the other denied the same action — a fixed-subject or subject-param bug would flip it.
- **`cargo test --all`** — see verifier-owned probes below. The prior full verification run (attempt `1e1f9ec4`, candidate `ad60be27`) failed ONLY `permissions_check_is_caller_scoped` with `left: "builtin-default-deny", right: Null` — i.e. the engine deterministically produced exactly the value the repaired assertion now pins. Between that verified tree and this candidate, `mcp.rs` differs only by that assertion + comment (`git diff cd372a6c..1b63ee4e`), and the base merge (`09f8d106`, task-124) touched `mcp.rs` not at all and added only same-typed route mappings to `abac_middleware.rs` (no new distinct resource types, no policy changes), so the permissions tests' inputs are unchanged. The `matched_policy: "builtin-default-deny"` pinning is correct engine semantics (the priority-1 no-condition catchall genuinely matched) and matches the REST `policies/evaluate` convention (`api/policies.rs:905`).

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --lib --tests` → **exit 0** (9m34s; full log `cargo-check.log`).
- `cargo test -p gyre-server --lib permissions` → **exit 64, mechanically blocked by the build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB. Use cargo check for type checking or a focused smaller -p package. Server builds/tests belong to the independent verifier") — infrastructure restriction, not a code defect (`cargo-test-permissions.log`).
- `cargo fmt --check -p gyre-server` → exit 0.
- In a throwaway worktree at the exact candidate `34c1838a`: `check-task-commit-attribution.sh` → **exit 0** (both product SHAs recorded in frontmatter); `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh` → all exit 0. In the main checkout: `check-arch.sh` → exit 0, `check-mcp-write-tools.sh` → exit 0 (permissions resources are reads, no `needs_write` impact).

Note: `check-task-commit-attribution.sh` fails in the main checkout only because of a pre-existing unstaged working-tree modification to `specs/tasks/task-133.md` (present at review start, controller artifact reverting the file to the assignment's initial state — not part of the candidate). On the exact candidate commit the gate passes. I did not touch that file.

## Test-inflation check

None found. All four tests drive the real router via `app().oneshot(...)` with real bearer tokens (minted worker JWT / ring-signed test JWTs); assertions are exact-value (matched_policy ids, action-list membership and exclusion). Disabling the real evaluation, hardcoding a subject, or dropping the param-error path each fails a specific assertion.

## Verifier-owned probes (this sandbox cannot compile gyre-server tests)

```
cargo test -p gyre-server --lib permissions
# full gate:
cargo test --all
```

## Scope check

No out-of-scope edits: diff touches only the five files above; no scripts, exemptions, or unrelated tasks modified; `web/dist/` untouched (verifier-owned).
