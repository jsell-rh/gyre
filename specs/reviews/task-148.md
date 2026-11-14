# Review — task-148 (gyre_analytics_decide MCP tool and Ralph Loop integration)

Spec: `specs/system/analytics.md` §MCP Tool: `gyre_analytics_decide`, §Integration with the Ralph Loop.
Commits under review (base `4b9220b` → candidate `929eee2`): `c103a38` (product), `a5ab668` (task notes), `929eee2` (checkpoint).
Verdict: **approved** — no findings.

## Probes run (review checkout at candidate `929eee2`)

- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0 (8m37s; compiles the
  new handler, the extracted core evaluators, and every new `#[cfg(test)]` test — also
  confirmed the build.rs web-asset path still works).
- `/tmp/stage/bin/cargo test -p gyre-domain decisions` → **24 passed, 0 failed** — runtime
  evidence that the pure-domain evaluators the MCP tool delegates to behave as the MCP
  tests' assertions expect (adoption-rate math, custom-rule first-match/default, DSL
  validation rejections).
- `bash scripts/check-mcp-write-tools.sh` → OK (8 write-capable tools checked, all gated);
  `gyre_analytics_decide` correctly absent from `needs_write` — its handler only queries
  the analytics/budget/merge-queue stores (verified by reading both core evaluators end
  to end: `query_filtered`, `get_usage`, `list_queue` — no writes).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-forged-scope-fields.sh`, `check-inert-enforcement.sh`,
  `check-scope-literal-defaults.sh` → all OK.
- Full diff inspection of all six touched files; assertion-by-assertion cross-check of
  each new test against the production evaluator source (details below).

Per this round's runtime constraints (hard 4 GiB budget), gyre-server test-binary
compilation was deferred to the verifier. Commands the verifier must run:

```
cargo test -p gyre-server --lib decide          # covers mcp_analytics_decide_* (5 protocol tests),
                                                 # handle_analytics_decide_delegates_by_custom_rules_presence,
                                                 # and the existing HTTP decide_* suites (refactor regression)
cargo test -p gyre-server --lib mcp_tools_list  # covers mcp_tools_list_includes_analytics_decide
```

## Verified working (no findings)

- **tools/list**: `tool_definitions()` gains a `gyre_analytics_decide` entry with the
  spec-verbatim description (all six built-in names), `inputSchema`
  `{decision: string (required), context: object, custom_rules?: array}` — placed
  alongside `gyre_analytics_query`. Inclusion is a static JSON literal, so the
  tools/list AC is structurally guaranteed; the protocol test additionally asserts it.
- **Thin wrapper, no duplicated decision logic (the core AC)**: the HTTP
  `decide`/`decide_custom` handler bodies were extracted into
  `api::analytics::evaluate_builtin_decision` / `evaluate_custom_decision`, and both the
  HTTP routes and `handle_analytics_decide` call those same cores. I read both cores in
  full: caller-scope validation (`validate_decision_scope` — cross-tenant
  context.workspace_id/repo_id rejected), event fetching, budget/merge-queue reads, and
  `DecisionResult` construction are identical for HTTP and MCP callers. The only logic
  in the MCP handler is argument parsing and `CustomDecisionSpec` assembly (`name` from
  `decision`, `context`, `custom_rules`, optional `default` — exactly the
  `POST /decide/custom` body shape; `CustomDecisionSpec::parse` field requirements all
  satisfied, `default` serde-defaults to `"default"`).
- **Dispatch**: `"gyre_analytics_decide"` arm in the `tools/call` match, reached only
  after the `needs_write` RBAC gate (correctly not applied — read-only tool, see
  check-mcp-write-tools above) and `validate_tool_scope`.
- **Token scoping**: new `validate_tool_scope` arm treats `context.workspace_id` /
  `context.repo_id` as scope-addressable args, mirroring `gyre_search`'s workspace arm
  (own-workspace pass-through; foreign workspace requires workspace scopes, else
  PERMISSION_DENIED; repo targets via `check_repo_read_target`). Defense in depth: the
  shared core re-validates the named scope against the caller's tenant
  (`validate_decision_scope`), so an MCP caller cannot leak another scope's decision
  outcome any more than an HTTP caller can.
- **Delegation contract holds**: without `custom_rules` → built-in evaluator (unknown
  names rejected listing the six built-ins); with `custom_rules` → custom evaluator
  (same decision name is legal as a custom name). The unit test
  `handle_analytics_decide_delegates_by_custom_rules_presence` is a real discriminator:
  it uses the SAME decision name (`feature.explode`) in both branches — invalid for the
  built-in evaluator, valid for the custom one — so swapping the delegation inverts both
  assertions. Not self-confirming.
- **Protocol tests are real, not mirrored stubs**: `mcp_analytics_decide_builtin_via_protocol`
  drives the actual router (`build_router(test_state())` + `oneshot` POST `/mcp` with
  bearer auth) over 200 real recorded adoption events and asserts the full
  `DecisionResult` shape. I cross-checked every asserted value against the production
  evaluator source: `promote: true` (sample 100 ≥ 30, adoption 1.0 ≥ 0.5, no
  error/latency delta — adoption is distinct current-window flag actors ÷ distinct
  current-window actors, so the previous-window `old-*` seeds correctly don't count),
  `metrics.sample_size` 100, `metrics.adoption_rate` 1.0, `confidence ≥ 0.9`
  (`1 − 1/√100 = 0.9`), `reason` contains "Adoption", ISO8601 `evaluated_at`. The
  seeding mirrors the already-proven HTTP test
  `decide_feature_promote_from_real_event_data`. The custom-rules protocol test's
  assertions (`outcome: "hold"`/`"approve"`, `matched_rule` null/0, reason containing
  "count 2") match `evaluate_custom`'s actual output format verbatim; error-path tests
  match `BuiltinDecision::parse`'s and `CustomDecisionSpec::parse`'s actual rejection
  strings (which surface verbatim — `ApiError::InvalidInput` Display writes the raw
  message, and the handler wraps it in a tool error).
- **Ralph Loop documentation**: `docs/agent-protocol.md` gains
  §"Analytics Decisions in the Ralph Loop" with the spec's six-step loop, built-in
  `feature.promote` and custom-rule MCP invocation examples, and the explicit
  steps-5–6-are-agent-decisions framing the task requires (pattern, not code).
  `docs/api-reference.md` MCP tool table row added.
- **Coverage matrix**: analytics.md rows 9 and 11 flipped to `implemented` with accurate
  notes (thin-wrapper extraction, scope validation, docs location); counts updated
  8/12 → 10/12 consistently; rows 10/12 remain task-assigned for task-149. No hollow
  `implemented` claims — both sections have real shipped behavior behind them.
- **HTTP API semantics preserved by the refactor**: the only behavioral reordering in
  `decide` is that context-JSON parsing now precedes decision-name parsing (both were
  400 `InvalidInput` before and after); all existing HTTP decide tests are untouched and
  still target the same routes.

## Notes for the verifier (non-blocking)

- `web/dist/` was not regenerated by the agent — per this round's rules that output is
  verifier-owned; no frontend source was touched by this task (no `web/src/` changes in
  the diff), so nothing frontend-specific rides on the rebuild.
- The review checkout's working tree has `specs/tasks/task-148.md` reverted to its base
  state by the stage harness; the candidate's committed version (with `commits:
  ["c103a38b..."]` and shipped notes) is what `929eee2` contains and what was reviewed.
