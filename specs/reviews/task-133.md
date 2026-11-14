# Review: task-133 — Implement ABAC MCP permission resources for agents

Comparison base: adf4df3d6823d80641e83fb5c2bbe1caa9262e43 → candidate 58cecef16bbfb472ebf6b1b3fb5e0fa33394cb14.
Task-labeled product commits: cd372a6c (feat), 1b63ee4e (test-attribution fix) — both
recorded in the task frontmatter on the candidate. Diff touches `mcp.rs` (+528),
`abac_middleware.rs` (+36, additive), `docs/api-reference.md`, coverage matrix row 18,
and the task record. No `scripts/` changes, no exemption-list changes, no deleted tests.

## Verdict: approved

No material findings. Probe evidence: /tmp/stage/review-evidence/probe-record.md.

## R1 Findings

None blocking. Evidence reviewed:

### Spec conformance (abac-policy-engine.md §MCP Integration)

- **`permissions://me`** (`resource_permissions_me`, mcp.rs): registered in
  `resources/list`; on `resources/read` builds the CALLER's attribute context
  (`agent_permission_context`) and runs the REAL task-129 engine
  (`policy_engine::evaluate`) for every canonical resource type × the spec §Attributes
  action vocabulary (read/write/delete/approve/spawn/push/merge/escalate/generate),
  returning `[{resource_type, allowed_actions: []}]` — the exact spec format.
  The resource-type vocabulary is derived from the live route registry
  (`ResourceResolver::distinct_resource_types`, non-exempt entries, sorted+deduped),
  so the probe set cannot drift from what the middleware enforces — a good
  structural choice, not a hardcoded list.
- **`permissions://check?action={action}&resource={type}`**
  (`resource_permissions_check`): registered with the URI template; parses both
  query params with the same non-decoding `split_once('?')`/`split('&')` parse the
  existing parameterized resources (`briefing://`, `notifications://`) use; missing
  params produce an explicit usage error, not a silent deny. Returns
  `{allowed: bool, matched_policy: string|null}` — `Option<String>` serializes to
  null when no policy matches, per the spec shape.
- **Own-permissions-only**: neither resource accepts a subject parameter; the
  subject context is derived exclusively from the caller's `AuthenticatedAgent`.
  Inherently satisfies "agent can only query its own permissions".

### Attribute-extraction fidelity (the pre-flight must match the middleware)

`agent_permission_context` replicates the middleware's extraction discipline
(abac_middleware.rs ~line 2194) attribute-for-attribute: subject.type/id/tenant_id
from the auth context, the reserved `SYSTEM_ABAC_IDENTITY` for the genuine system
principal, role-SET semantics for `subject.global_role` (empty → unset, fail
closed; multi-role → list with membership semantics), the allowlisted
`merge_agent_jwt_claims` + `wl_stack_hash` normalization gated on
`subject.type == "agent"`, membership/team store lookups for user identities
(failed lookup → unset, fail closed), and `env.budget_remaining` from the real
budget stores via the same `budget_tokens_remaining` helper (absent when no cap is
configured — no fabricated number). `action`/`resource.type`/`env.time` are
injected by the engine itself. The resource scope chain comes from the caller's
own resolved scope (`agent_secret_scope`: claims → agent record → task, tenant
from the governing workspace — never fabricated), the same no-scope conservative
path as the REST `/policies/effective` endpoint. For agent callers (the spec's
audience) there is no user identity, so the middleware's membership-block
attributes are unset on live agent requests too — the pre-flight answer for
agents is the middleware's answer.

### Test quality (not self-confirming)

- `permissions_resources_evaluate_real_policies_for_agent_jwt` — a real
  Gyre-minted worker JWT through the HTTP+JSON-RPC path; asserts EXACT matched
  policy ids of real seeded builtins (`builtin-agent-scoped-access` for
  read-on-repo, the immutable `builtin-require-human-spec-approval` for
  approve-on-spec — the immutable Deny beating the role Allow can only happen
  through the real engine), plus `permissions://me` content (repo allows
  read/write/approve, not push/merge/spawn/delete/escalate/generate; spec/persona
  exclude approve). Verified the referenced builtin ids/actions/priorities exist
  in the seed (`m34_builtin_policies`, `gyre-domain` builtin set) and that
  `orch_state()` seeds them.
- `permissions_check_is_caller_scoped` — two JWT callers, one allowed push-on-repo
  by a `subject.id`-keyed Allow at priority 2000, the other denied with the
  denial attributed to the `builtin-default-deny` catchall (priority 1, no
  conditions, `actions: ["*"]` — verified in the seed; `make_test_state_with_jwt`
  uses the seeded policy pair). Kills any fixed-subject or subject-param bug.
  Verified the JWT provisioning path sets `subject.id` = `preferred_username`
  (`find_or_create_user` → `agent_id: user.display_name`), so the keyed condition
  genuinely matches caller A and not B.
- `mcp_resources_list_includes_permission_resources` and
  `permissions_check_requires_both_params` cover registration and error paths.

### Independent execution evidence

- `cargo check -p gyre-server --lib --tests` → exit 0 (clean, no warnings).
- `cargo test -p gyre-server --lib permissions` → exit 64: the executor's build
  wrapper mechanically blocks server test compilation in-model (4 GiB budget) —
  infrastructure restriction, not a code defect; recorded for the verifier.
- **Transferable run evidence**: the recorded full-suite verification of candidate
  34c1838 (durable finding 18115aed, `cargo test --all`: 2238 passed / 1 failed)
  executed these exact four tests — `git diff 34c1838 58cecef1` shows ZERO
  changes to `mcp.rs`'s permissions code or to any file in the evaluation
  environment (`auth.rs`, `policy_engine.rs`, `abac_middleware.rs`, `mem.rs`,
  `gyre-domain`). The single failure in that run was the
  `notify_rich_falls_back_to_workspace_home_link` wall-clock-second ordering
  flake, unrelated to task-133 and fixed on this branch by de74a59f (selection by
  source entity_id, verified present at HEAD).
- Mechanical invariants all pass: check-mcp-write-tools, check-abac-route-registry,
  check-abac-exempt-handlers, check-inert-enforcement, check-forged-scope-fields,
  check-byte-slice-truncation, check-relative-path-defaults,
  check-fail-open-ref-resolution, check-lossy-secret-conversion,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-dead-message-kinds.

### Non-blocking observations (recorded, not defects)

1. `permissions://me` probes the spec's §Attributes action vocabulary (9 actions),
   not every action string the route registry produces (e.g. `complete`,
   `transfer_ownership`). The shipped notes document this; the spec table itself
   only enumerates those example actions, and `permissions://check` answers for
   ANY action, so the pre-flight contract is fully covered.
2. For user-JWT callers (not the spec's audience) the type-level pre-flight cannot
   know a future request's addressed workspace, so `subject.workspace_role` /
   `member_of_resource_workspace` are unset — a type-level Allow may be reported
   that an instance-level request would deny. Advisory-only direction (the
   middleware still enforces on the real call); identical conservatism to the
   REST `/policies/effective` no-scope path.

### Verifier-owned probes (build-budget restriction)

```
cargo test -p gyre-server --lib permissions
cargo test -p gyre-server --lib notifications
cargo test --all
```
