# Review — task-089 (HSI Explorer Generated Views, LLM-Powered)

Spec: `specs/system/human-system-interface.md` §3 "Generated Views (LLM-Powered, On-Demand)" (lines 756-784), task `specs/tasks/task-089.md`.
Candidate: `9980ba0c8c8baf4d93a55d9ae7099f0b7952a234` (branch `pipeline/task-089/b47c796e6a1c43ff8eaeaad85e292141-1`), diff against base `98a5299b9` = feat commit `801a872a3` + process commit `9980ba0c8` (14 files, +2391/−20).
Verdict: **approved**.

## Probes (evidence in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib api::explorer_views` → **23 passed, 0 failed** (incl. 7 new task-089 tests: flow pass-through with traced MR, flow→graph fallback for untraced MR, flow→graph fallback for cross-workspace MR, invalid `previous_spec` → 400, prompt-injection/unit assertions on `build_generate_user_prompt`, explanation fallback chain, `recent_mrs_with_traces` ordering/filtering/limit/cross-workspace isolation). (`rust-explorer-views.txt`)
- `cargo test -p gyre-server --lib api::merge_requests` → **33 passed, 0 failed** (incl. new `list_workspace_mrs_filters_by_trace_and_limits`). (`rust-merge-requests.txt`)
- Full `npm test` in `web/` → **86 files, 1883 passed, 41 skipped, 0 failed** — matches the shipped claim exactly. (`web-full.txt`)
- Focused web run of the three new test files → 25 passed. (`web-focused.txt`)
- **Mutation probe 1 (Rust):** disabled `enforce_flow_trace_backing` (made its layout guard never match) → `generate_flow_layout_without_trace_falls_back_to_graph` and `generate_flow_layout_with_foreign_mr_falls_back_to_graph` both **fail** with the raw flow payload in the assertion output. The fallback tests kill a real bug, they are not self-confirming. Source restored afterwards. (`mutation-rust-flow-backing.txt`)
- **Mutation probe 2 (web):** dropped the `explanation` from `viewSpecToViewQuery`'s query annotation → both the unit test and the full-tree `ExplorerGeneratedViews` integration test (asserting the rendered `.query-annotation` sidebar contains the LLM explanation) **fail**. Source restored afterwards. (`mutation-web-annotation.txt`)
- Standing checks all pass: `check-arch.sh`, `check-abac-route-registry.sh` (no new routes; query params only on an existing route), `check-mcp-write-tools.sh`, `check-forged-scope-fields.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-task-commit-attribution.sh`.
- Working tree verified clean (candidate state) after all probes; `web/dist` rebuilt by the probe's build.rs was restored to the committed baseline.

## Verified against the task contract

- **NL question → valid view definition**: `POST /workspaces/:id/explorer-views/generate` validates LLM output via the existing `parse_and_validate` grammar gate; invalid output yields `{view_spec: null, fallback}` (tested). `PROMPT_EXPLORER_GENERATE` now carries the full ViewSpec grammar (data incl. `trace_source`, layout, encoding, annotations, highlight, explanation) and the spec's layout-selection rules (flow = data movement requiring a listed mr_id, hierarchical = containment, graph = relationships, list = enumeration).
- **Renders on ExplorerCanvas**: `viewSpecToViewQuery` (concept/filter scopes, type+edge normalization, `$governed_by` quote-escaped computed filters, annotations→callouts) → `activeViewQuery`; the integration test renders the full ExplorerView tree and asserts the canvas state.
- **Flow for data movement / graph-hierarchical for structural**: prompt rules plus grammar; flow additionally **server-enforced** — `enforce_flow_trace_backing` rewrites flow→graph (stripping `trace_source`) unless the mr_id exists, belongs to this workspace, and currently has a captured trace (same `traces.get_by_mr` lookup as the trace endpoint). Cross-workspace mr_id treated as unbacked without confirming existence.
- **MR ID discovery**: `recent_mrs_with_traces` (newest-first, cap 10) injected into the user prompt with mr_id/title/spec_ref/status; empty list → explicit "do not use flow" instruction. `GET /workspaces/:id/merge-requests` gained the specced `?has_trace=true&limit=N` (tested).
- **Explanation sidebar**: the `complete` SSE event carries the LLM's own `explanation` (fallback chain explanation→description→generic, tested); rendered as the canvas `.query-annotation` (integration test) and in the panel.
- **Save**: per-view "Save this view" → `POST /workspaces/:id/explorer-views` with name/description/spec (component test pins the exact payload; spec includes query + layout + explanation).
- **Refine**: follow-ups send `previous_spec`; the server validates it against the same grammar + repo-ownership before the LLM sees it (400 on invalid, tested) and injects it into the user prompt (unit test pins the injection and that the question terminates the prompt).
- **No-trace fallback**: server-enforced (see mutation probe 1); the client mirrors it (`mrTrace` fetch failure → structural lens).
- **Read-only LLM**: handler calls only `predict_json` (no tools, no writes); user question never enters the system prompt ({{question}} stripped; question terminates the user prompt after server-derived context blocks); `previous_spec` and LLM output both grammar- and ownership-validated. Rate limiting and budget recording are pre-existing and intact.
- `npm test` passes in `web/` (see above).

## Minor observations (non-blocking, recorded for completeness)

- `GenerateViewRequest.repo_id` is accepted but unused by the handler — pre-existing field, not introduced by this task; repo scoping in practice comes from applying the generated view to the currently loaded canvas repo, and any `repo_id` the LLM emits is ownership-validated server-side.
- The client buffers the SSE body (`resp.text()`), so `partial` progress frames are not rendered live; the thinking indicator covers the wait. The spec does not require live partial rendering and the shipped notes do not claim it.
- Coverage row 22 (`specs/coverage/system/human-system-interface.md`) remains `task-assigned` — consistent with the pipeline convention (the row flips in the post-review process stage, as task-088's row shows); not an implementer defect.
