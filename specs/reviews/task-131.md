# Independent review — task-131 (ABAC policy CLI commands)

Candidate: 62f7dc1105368d58aaa0c73a17c945f51ab2725d (merge: policy CLI branch + main 8d07a41a/task-126)
Base:      8d07a41a3db43148290bfdbb29e599e122a48978
Spec:      abac-policy-engine.md §CLI

## Verdict: APPROVED

## Scope of the diff (6 files, all reviewed)

- crates/gyre-cli/Cargo.toml — serde_yaml dev-dep → regular workspace dep (needed for `policy create`)
- crates/gyre-cli/src/client.rs — 5 real HTTP client methods + wire types for
  GET/POST /api/v1/policies, POST /policies/evaluate, GET /policies/decisions,
  GET /policies/effective
- crates/gyre-cli/src/main.rs — `gyre policy {list,create,evaluate,decisions,effective}`
  command group, {type}:{id} ref parser, --since duration parser, YAML loader,
  client-side decision filters, output tables, 23 focused tests
- docs/cli.md — §Policies docs with a YAML example
- specs/coverage/system/abac-policy-engine.md — row 16 (CLI) flipped to implemented
- specs/tasks/task-131.md — Shipped notes + commits frontmatter

## Evidence

1. **Wire contracts cross-checked against server source** (gyre-server/src/api/policies.rs):
   - list: `?scope=&scope_id=` matches ListPoliciesQuery; parse_scope is case-insensitive,
     CLI sends lowercase ValueEnum strings ("tenant"/"workspace"/"repo") — valid.
   - create: body field-for-field the server's CreatePolicyRequest (name, description,
     scope, scope_id, priority, effect, conditions[{attribute,operator,value}],
     actions, resource_types, enabled).
   - evaluate: {subject:{type,id}, action, resource:{type,id}} matches EvaluateRequest
     (SubjectAttrs/ResourceAttrs); response shape matches EvaluateResponse.
   - decisions: `?subject_id=&resource_type=&limit=` matches ListDecisionsQuery.
   - effective: `?subject=&resource_type=` matches EffectiveQuery (subject is the
     documented param name; server loads real stored attributes).
   - Response types match domain serialization: PolicyScope/PolicyEffect are
     #[serde(rename_all = "snake_case")]; Id is a newtype over String (serializes
     as a plain string), so Option<String> for matched_policy is correct.
2. **Merge resolution is a true union**: line-set comparison shows every line of both
   parents' main.rs and client.rs is present in the merge; the only changed line is
   `use anyhow::Result;` → `use anyhow::{Context, Result};` (Context added by policy
   code). Nothing from either the policy side or task-126's side was dropped.
   This resolves the prior DIRTY merge_conflict durable finding.
3. **Focused tests** (`/tmp/stage/bin/cargo test -p gyre-cli`): 217 passed + 1
   ws-integration, 0 failed (includes both policy tests and task-126's user-management
   tests). Log: cargo-test-gyre-cli.log.
4. **Mutation probes (both killed, source restored after each)**:
   - Disabling the outcome filter in `filter_decisions` →
     `filter_decisions_applies_outcome_and_age_filters` FAILED.
   - Removing the runtime `--workspace-id` scope guard →
     `policy_list_flow_rejects_workspace_id_without_workspace_scope` FAILED.
   The tests are not self-confirming; they kill real behavior breaks.
5. **Mock-server flow tests assert exact request lines and bodies**
   (e.g. `GET /api/v1/policies?scope=workspace&scope_id=ws-1`, the evaluate JSON
   shape, `?subject_id=worker-42&limit=250`, `?subject=jsell&resource_type=spec`)
   and error propagation for 403/400 plus pre-request rejection of malformed input.
6. **Help text verified by running the built binary** (`gyre policy --help`,
   `gyre policy list --help`): all five subcommands and flags documented.
   Logs: policy-help.txt, policy-list-help.txt.
7. **Mechanical checks**: cargo fmt -p gyre-cli --check clean; check-arch.sh,
   check-byte-slice-truncation.sh, check-inert-enforcement.sh,
   check-relative-path-defaults.sh, check-unbounded-external-http.sh,
   check-lossy-secret-conversion.sh all pass. check-task-commit-attribution.sh
   passes against the committed candidate (the working-tree failure observed
   during review was caused solely by the review harness's reset of the task
   frontmatter to `commits: []`, not by the candidate).

## Notes (non-blocking)

- `--decision`/`--since` filtering for `policy decisions` is client-side over the
  fetched newest-first page (default --limit 100). The spec excerpt does not
  mandate server-side filtering and the server API only offers
  subject_id/resource_type/limit; the flag help documents the window. Acceptable.
- The decisions table's EVALUATED_AT column reuses `fmt_secret_epoch`
  ("epoch <seconds>"), the CLI's existing timestamp convention (no chrono dep).
  Cosmetic only.
- Server-side tests (api::policies endpoint contracts, cargo test -p gyre-server)
  are untouched by this diff (server code byte-identical to main) and are owned by
  the independent verifier per the 4 GiB build budget. The verifier must run
  `cargo test --all` (final acceptance criterion).
