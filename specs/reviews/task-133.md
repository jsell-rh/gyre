# Review — task-133 (Implement ABAC MCP permission resources for agents)

Spec: `specs/system/abac-policy-engine.md` §MCP Integration.
Assigned base `44cf25bf`; candidate `d60997f1` (merge `b4ded2f9` re-merging main
`44cf25bf` — the previously DIRTY PR 837 head `c76aae92` rebased with the
coverage-matrix conflict resolved — plus process commit `d60997f1`).
Implementation commits `cd372a6c` + `1b63ee4e`, both ancestors of the candidate
and recorded in the task's `commits:` frontmatter.
Verdict: **approved** — no blocking findings. Evidence: `/tmp/stage/review-evidence/`.

## Probes run

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0,
  no warnings (cold check, 8m05s). The candidate, including all four new tests,
  type-checks.
- 18 mechanical invariant scripts on the candidate tree → all exit 0 (arch,
  abac-route-registry, mcp-write-tools, abac-context-parity, abac-exempt-handlers,
  inert-enforcement, fabricated-scope-defaults, scope-literal-defaults,
  lossy-secret-conversion, unbounded-external-http, relative-path-defaults,
  byte-slice-truncation, fail-open-ref-resolution, conditional-test-guards,
  in-memory-state-stores, forwarded-header-trust, forged-scope-fields,
  migration-versions). `check-assertionless-tests.sh` exits 2 identically on the
  BASE commit — a sandbox mawk-vs-gawk incompatibility in the script's awk
  program (3-arg `match()`), not a candidate defect.
- `cargo test` of gyre-server is mechanically blocked for the review stage by the
  build wrapper (4 GiB budget). **Verifier must run:**
  `cargo test -p gyre-server --lib permissions`, `cargo test -p gyre-server --lib mcp`,
  and `cargo test --all` (the task's final acceptance criterion).

## Verified real (no findings)

- **Both resources answer from the real engine.** `resource_permissions_me` and
  `resource_permissions_check` (mcp.rs) call `policy_engine::evaluate`
  (policy_engine.rs:406 — immutable-Deny-first, priority+scope-specificity sort,
  default deny) over `state.policies.list()` — the live policy set. No stub, no
  hardcoded decisions, no audit-only path.
- **`permissions://me` shape and vocabulary.** Returns `[{resource_type,
  allowed_actions: []}]`, one entry per canonical resource type from
  `abac_middleware::all_resource_types()` → `ResourceResolver::distinct_resource_types`
  (distinct non-exempt route-registry types — the probe set cannot drift from what
  the middleware enforces), probed over the spec §Attributes action vocabulary
  (read/write/delete/approve/spawn/push/merge/escalate/generate).
- **`permissions://check?action={action}&resource={type}`** parses both params
  (missing → explicit usage error, never a silent deny), evaluates one real check,
  returns `{allowed, matched_policy}` with `null` on no-match — the spec's
  `string|null`. Registered in `resources/list` with the URI template; `me` also
  registered (`mcp_resources_list_includes_permission_resources`).
- **Own-permissions-only.** Neither resource accepts a subject parameter; the
  subject context is derived exclusively from the caller's `AuthenticatedAgent`,
  mirroring the middleware's extraction discipline (subject.type/id/tenant_id,
  reserved non-spoofable SYSTEM_ABAC_IDENTITY, role-SET semantics with fail-closed
  empty roles, agent-only allowlisted JWT claims + wl_stack_hash normalization,
  membership/team stores for user identities). Killing test
  `permissions_check_is_caller_scoped`: two distinct RS256-minted user JWTs get
  different answers for the same action (subject.id-keyed Allow at priority 2000
  vs the seeded `builtin-default-deny` catchall) — fails under any fixed-subject
  or subject-param implementation.
- **Real-policy test evidence.** `permissions_resources_evaluate_real_policies_for_agent_jwt`
  uses a Gyre-minted worker EdDSA JWT (register_worker → agent_tokens KV):
  read-on-repo allowed with `matched_policy: "builtin-agent-scoped-access"`;
  approve-on-spec denied by the immutable `builtin-require-human-spec-approval`;
  `permissions://me` repo entry carries read/write/approve but not
  push/merge/spawn/delete/escalate/generate; spec/persona entries exclude approve.
  All expectations cross-checked against the actual seeded builtin set
  (m34_builtin_policies + gyre_domain::builtin_policies, seeded into
  mem::test_state) and the composition order (scope_applies + policy_sort_key) —
  the assertions can only pass through the real evaluation engine.
- **Scope is the caller's own.** The resource scope chain comes from
  `agent_secret_scope` (pre-existing, task-098: JWT claims → agent record → task,
  tenant from the governing workspace record — never fabricated), and
  `env.budget_remaining` from the real budget stores via
  `budget_tokens_remaining` (None when uncapped — fail closed). `env.time`,
  `action`, `resource.type` are injected by the engine.
- **Rebase integrity.** `git diff c76aae92..d60997f1 -- mcp.rs abac_middleware.rs`
  is empty — the implementation is byte-identical to the previously published
  head; the rebase only merged main's task-132 UI files. The single prior conflict
  (coverage matrix) is resolved keeping both implemented rows; counts consistent
  (16/19, 2 n/a, row 16 task-assigned). docs/api-reference.md rows added.
- **No collateral breakage:** no test asserts an exact resources/list count
  (existing assertions are `contains` only); no new routes, migrations, message
  kinds, or MCP write tools; all relevant invariant scripts pass.

## Non-blocking observations (recorded, no action required)

- **Type-level pre-flight is inherently approximate for instance-keyed
  policies.** The permission context does not (and cannot, for an action+type
  query without a resource instance) carry `resource.id`/`resource.owner`/
  `resource.visibility`/`subject.workspace_role`/`subject.member_of_resource_workspace`.
  For agent callers (the spec's audience) the middleware sources those from the
  addressed path/entity, so a pre-flight can diverge from a specific call's
  decision in both directions for policies keyed on them. This mirrors the REST
  `/policies/effective` no-scope path (task-129) and is inherent to the spec's
  check shape; for agent subjects the divergence set is limited to
  entity-attribute-keyed policies.
- `PERMISSION_ACTIONS` is the spec's §Attributes example vocabulary (9 actions);
  custom actions outside it won't appear in `permissions://me` (though
  `permissions://check` accepts any action string). Matches the task plan's
  "spec's action vocabulary".
- The doc comment on `agent_permission_context` ("the answer the pre-flight
  check gives is the answer the middleware would give") slightly overstates the
  parity noted above — comment wording only.

## Test-inflation check

None found. All four tests assert concrete, engine-derived outcomes (specific
matched_policy ids, allow/deny asymmetry pinned to immutable vs role policies,
caller-differentiated results, param-error messages) and would fail if the
resources returned fabricated data, ignored the caller, or skipped evaluation.

## Scope check

Diff touches only `mcp.rs`, `abac_middleware.rs` (helper exposure + vocabulary
enumeration), `docs/api-reference.md`, the coverage matrix, and the task file.
No production code, scripts, verifiers, unrelated tasks, or specs modified
beyond the task's own surfaces.
