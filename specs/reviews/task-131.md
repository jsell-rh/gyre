# Review — task-131 (Implement ABAC policy CLI commands)

Spec: `abac-policy-engine.md` §CLI. Candidate `30d32319` (implementation
commit `05549afd`, plus merge/process commits resolving the branch against
main through task-133), base `9afd66c7`. Verdict: **approved**.

## What was reviewed

`git diff 9afd66c7..30d32319` touches exactly six files: the three
`gyre-cli` sources (`Cargo.toml` moves `serde_yaml` from dev-deps to deps,
`client.rs` adds five policy API methods + wire types, `main.rs` adds the
`Policy` command group, `run_policy_command`, YAML loading, filters, and
table printers), `docs/cli.md`, and the two spec/process records. **No
server, adapter, or domain files change relative to base** — the server-side
content visible in the branch's history arrives via merge commits from main
and is byte-identical to base's, so the endpoint contracts the CLI calls are
the already-verified task-129/132/133 surface.

## Contract fidelity (verified against server source in this tree)

- `policy list` sends `?scope=&scope_id=` — matches server
  `ListPoliciesQuery`; `--scope` is a clap `ValueEnum` (tenant/workspace/repo,
  the exact snake_case strings `PolicyScope` serializes), and
  `--workspace-id` is gated twice: clap `requires = "scope"` plus a runtime
  check pinning the scope to `workspace`.
- `policy create` POSTs a body field-for-field identical to the server's
  `CreatePolicyRequest` (conditions carry raw JSON values, matching the
  server's `ConditionRequest`); scope/effect validated client-side before
  any network I/O; reserved `trust:`/`builtin:` prefixes and Admin-only
  enforcement remain server-side and surface as HTTP errors (tested for
  400).
- `policy evaluate` POSTs `subject.type/id` + `resource.type/id` — matches
  the server's `EvaluateRequest`; prints decision, matched policy,
  evaluated count, evaluation time (the spec's response shape).
- `policy decisions` GETs `?limit=&subject_id=` — matches the server's
  `ListDecisionsQuery`; `--decision`/`--since` are applied client-side by
  the tested `filter_decisions` helper (the server endpoint supports only
  subject_id/resource_type/limit — documented in `docs/cli.md` with
  `--limit` to scan deeper; see Observations).
- `policy effective` GETs `?subject=&resource_type=` — matches the server's
  `EffectiveQuery`, which binds the documented `subject` param name.
- Deserialization forms check out: `PolicyScope`/`PolicyEffect` are
  snake_case serde enums (plain strings), `Policy.priority/enabled/built_in`
  and `PolicyDecision.evaluated_at/resource_id/matched_policy` match the
  CLI's `PolicyResponse`/`PolicyDecisionResponse` field types.

## Probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-cli policy` → **23 passed, 0 failed**.
- `cargo test -p gyre-cli` → **178 passed + 1 ws-integration, 0 failed**
  (matches the shipped claim).
- **Mutation A**: removed the runtime `--workspace-id requires --scope
  workspace` guard → `policy_list_flow_rejects_workspace_id_without_workspace_scope`
  **FAILED** (killed). Source restored; tree clean.
- **Mutation B**: disabled the `--since` age filter in `filter_decisions` →
  `filter_decisions_applies_outcome_and_age_filters` **FAILED** (killed).
  Source restored; tree clean.
- Mock-server flow tests assert exact request lines and bodies
  (`GET /api/v1/policies?scope=workspace&scope_id=ws-1`, the create POST
  body, the evaluate JSON shape,
  `GET /api/v1/policies/decisions?limit=250&subject_id=worker-42`,
  `GET /api/v1/policies/effective?subject=jsell&resource_type=spec`), so any
  param-mapping regression fails a test; 403/400 propagation and
  pre-request rejection of malformed `--subject`/`--since` are asserted.
- `cargo run -p gyre-cli -- policy --help`, `policy list --help`,
  `policy decisions --help` → complete help for the group, all subcommands,
  and all flags (acceptance criterion verified live).
- `cargo fmt -p gyre-cli --check` → clean.

## Spec acceptance criteria

All §CLI commands ship and behave as specced (see contract fidelity above);
error handling names the HTTP status and server message; help text verified;
the `cargo test --all` criterion is assigned to the independent verifier per
the 4 GiB budget rule — the CLI package passes in full here, and this branch
changes no server code relative to base, so the server suite's status is
unchanged from main's verified state. Coverage row 16 (CLI) is flipped to
`implemented` with notes that accurately describe the shipped code; the
coverage header correction (17/19) follows.

## Observations (non-blocking)

- `--decision`/`--since` filtering on `policy decisions` is client-side over
  the fetched newest-first page because the server endpoint (task-129's
  frozen surface) supports only subject_id/resource_type/limit query
  params. §CLI specifies the flags, not the filter locus; the behavior and
  the `--limit` escape hatch are documented in `docs/cli.md`. If server-side
  outcome/since filters are ever wanted, that is an `abac-policy-engine.md`
  §API change, not a defect of this CLI task.

## Verifier must run

- `cargo test --all` (final acceptance criterion), in particular
  `cargo test -p gyre-cli` and the server-side `api::policies` tests owning
  the endpoint contracts the CLI calls.
