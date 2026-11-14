# Review: task-131 — Implement ABAC policy CLI commands

**Reviewer:** Independent review (stage: review)
**Round:** R3 (rebase-resolution candidate after merge_conflict finding)
**Spec ref:** abac-policy-engine.md §CLI
**Comparison base:** 44cf25bf0bea544979fcae88c4d80b1165964fbe
**HEAD reviewed:** 26012273d40698cb47c1a18817e4ba01db604a1b
**Status:** approved

---

## Scope of the diff

`crates/gyre-cli/src/main.rs` (PolicyCommands enum, run_policy_command,
print tables, helpers, 28 tests), `crates/gyre-cli/src/client.rs` (5 client
methods + mirror types), `crates/gyre-cli/Cargo.toml` (serde_yaml promoted
from dev-dependency to regular dependency — legitimate, it is now
production code for `policy create --file`), `docs/cli.md` (§Policies),
`specs/coverage/system/abac-policy-engine.md` (row 16 flip),
`specs/tasks/task-131.md` (shipment record). No server/domain/adapter/
common/ports files touched — the CLI consumes the task-129/132 policy API
already present in the base, so the previous round's merge conflict is
resolved without re-implementing server code.

## Findings

None blocking.

### Wire-contract verification (CLI mirrors vs server)

Checked every mirrored type against the live server contract in
`crates/gyre-server/src/api/policies.rs` and
`crates/gyre-domain/src/policy.rs`:

- `PolicyScope`/`PolicyEffect` serialize snake_case ("workspace"/"allow")
  — the CLI's `String` fields and lowercase `--scope` values match; the
  server's `parse_scope` is case-insensitive, so the CLI's lowercase
  values are accepted.
- `Id` is a newtype over `String` with derived serde — `scope_id:
  Option<Id>` and `matched_policy: Option<Id>` on the wire are plain
  strings/null, matching the CLI's `Option<String>`. Unknown extra fields
  in the `Policy` JSON (conditions, created_by, …) are ignored by serde.
- `EvaluateRequest` on the server is exactly
  `{subject: {type, id, …}, action, resource: {type, id, workspace_id,
  repo_id, tenant_id}}` — the CLI's
  `EvaluatePolicyRequest`/`EvaluateSubject`/`EvaluateResource` send the
  documented subset the flags carry.
- `PolicyDecision.evaluated_at` is set with `.as_secs()`
  (policy_engine.rs:511-514), the same unit the CLI's `filter_decisions`
  compares against `SystemTime::now().as_secs()` — the `--since` age
  filter is unit-consistent.
- `GET /policies/decisions` query params (`subject_id`, `resource_type`,
  `limit`), `GET /policies/effective` params (`subject` + alias
  `subject_id`, `resource_type`), and the `EffectivePermission` response
  rows all match.

### Acceptance criteria

1. **`policy list` with scope/effect columns** — real; mock-server flow
   test asserts the exact request line
   `GET /api/v1/policies?scope=workspace&scope_id=ws-1` and the table
   prints id/name/scope/scope_id/effect/priority/enabled/built-in.
2. **Filters** — `--scope` is a clap `ValueEnum` (unknown value rejected
   at parse time, killed by `cli_policy_list_rejects_unknown_scope_value`);
   `--workspace-id` is double-gated: clap `requires = "scope"` (killed by
   a parse test) plus a runtime check that the scope is exactly
   `workspace` (killed by
   `policy_list_flow_rejects_workspace_id_without_workspace_scope`, which
   asserts the bail happens with the error naming the flag — code
   inspection confirms the bail precedes any `api.list_policies` call).
3. **`create --file policy.yaml`** — real serde_yaml parse mirroring the
   server's `CreatePolicyRequest` field-for-field, client-side
   scope/effect validation before network I/O; flow test asserts the
   exact POST body derived from the YAML. Reserved-prefix/Admin
   enforcement stays server-side and the 400 propagation is tested.
4. **`evaluate` dry-run** — flow test asserts the exact request body
   (`subject.type/subject.id`, `resource.type/resource.id`) and the
   display fields (decision, matched policy, evaluated count, ms).
5. **`decisions`** — flow test asserts
   `GET /api/v1/policies/decisions?limit=250&subject_id=worker-42`;
   `filter_decisions` outcome/age matrix test covers all filter
   combinations. The `--decision`/`--since` filters are client-side
   because the server API does not offer them; the fetched-window
   limitation is documented in the flag help and docs/cli.md.
6. **`effective`** — flow test asserts
   `GET /api/v1/policies/effective?subject=jsell&resource_type=spec`;
   the matrix prints action/resource-type/decision/matched policy. The
   server resolves real stored attributes for the subject.
7. **Error handling** — 403 propagation tested; 400 propagation tested
   (reserved prefix); malformed `--subject`/`--since`/YAML rejected
   before any request (tested).
8. **Help text** — doc comments on the group, all 5 subcommands, and
   every flag (clap renders them).
9. **`cargo test --all`** — cannot be run here under the 4 GiB budget
   (gyre-server build belongs to the verifier); `cargo test -p gyre-cli`
   passes: 178 unit tests + 1 ws-integration, 0 failed. Focused policy
   subset: 28 tests pass.

### Fakery/test-inflation checks

- The mock HTTP server infra (`client::test_http`) pre-exists in the
  base; the tests use established patterns, not fabricated helpers.
- Flow tests assert exact request lines and bodies — they fail if the
  endpoint, query params, or payload shape regress (killing direction
  verified by inspection of the assertions).
- `filter_decisions`, `parse_subject_ref`, `parse_since_duration`, and
  `load_policy_yaml` tests assert concrete values including rejection
  cases; none are self-confirming.
- `serde_yaml` promotion is a real production dependency change, not
  test-only code shipped as product.

### Coverage bookkeeping

Row 16 (§CLI) flipped to `implemented` with notes that accurately
describe the shipped code. Coverage 16/19 (2 n/a) is consistent with the
table (row 18, MCP Integration, remains `task-assigned` for task-133 —
out of this task's scope). The merge-conflict resolution in the coverage
file combines both sides' audit outcomes without erasing either.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-cli policy` — 23 passed, 0 failed.
- `cargo test -p gyre-cli` — 178 passed + 1 ws-integration, 0 failed.
- `cargo fmt -p gyre-cli --check` — clean.
- `scripts/check-arch.sh`, `check-task-commit-attribution.sh`,
  `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`,
  `check-inert-enforcement.sh` — all pass.
- `git diff --name-only base..candidate` over all server-side crates —
  empty (the CLI work does not touch the already-verified server tree).

## Required of the independent verifier

- `cargo test --all` (final acceptance criterion; not runnable in the
  review sandbox under the 4 GiB budget).
- In particular `cargo test -p gyre-server` — the `api::policies`
  endpoint-contract tests the CLI depends on, including
  `list_policies_scope_query_is_case_insensitive`.
