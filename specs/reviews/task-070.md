# Review — task-070 (LLM Explorer Agent — Configuration, System Prompt & Self-Check Loop)

Spec: `explorer-implementation.md` §7–8, §10–11.
Comparison base: `ba5ebe101be72c54ece49203ceee1d5f41d2161f` → candidate `dc89e68a`.
Commits under review: the 2 attributed in the task frontmatter (a885c918, a58467b4) — verified ancestors of the candidate; the remaining commits in the range are `process:` attribution/record commits and the rebase merge 4df679bc (web/dist taken wholesale from base — zero delta vs base in the final diff; only 4 files change: explorer_ws.rs, llm_helpers.rs, explorer-agent.mjs, task-070.md).

## What was verified

### §7–8 — Claude Agent SDK integration & configuration

- `scripts/explorer-agent.mjs` imports `query()` from `@anthropic-ai/claude-agent-sdk` and runs headless (JSON on stdin, JSON-lines on stdout). Live stub probe (SDK import replaced with an async-generator stub in an isolated /tmp copy, real node v22, repo untouched) confirmed the exact options reaching `query()`: model passes the server-resolved value through with script-level fallback `model || GYRE_LLM_MODEL || 'claude-sonnet-4-6'`; `mcpServers.gyre = {type:'http', url: server_url + '/mcp', headers: {Authorization: Bearer <token>}}`; `allowedTools` covers all five §8 tools (`mcp__gyre__graph_summary`, `graph_query_dryrun`, `graph_nodes`, `graph_edges`, `search`) plus read-only `node_provenance` (documented in the system prompt; served by the real MCP endpoint — names verified against mcp.rs tool_definitions/dispatch); built-in tools disabled (`tools: []`), so the agent is read-only analysis only per §7.
- Server-side model resolution: workspace/tenant LLM config → `GYRE_LLM_MODEL` → `EXPLORER_SDK_DEFAULT_MODEL = "claude-sonnet-4-6"` via new `resolve_llm_model_with_default` (llm_helpers.rs); all other callers keep the platform default unchanged. The native Vertex fallback correctly keeps the gemini default (claude model names are invalid there).
- SDK path is admin-gated at dispatch because the subprocess carries the server master token; non-admins fall through to the native path with a warning. Opt-out via `GYRE_EXPLORER_SDK=0`.
- Round-2 defect (relative script-path re-resolution diverging from the validated canonical path) verified fixed: `run_sdk_subprocess_once` spawns only the canonicalized, validation-passing path threaded from the dispatch site; `check-relative-path-defaults.sh` passes.

### §10 — System prompt

`build_system_prompt_body()` carries the full §10 workflow with `__WORKFLOW_STEP_1__` substituted per path: SDK variant is spec-verbatim ("Call graph_summary to understand the codebase structure"); native variant notes pre-loaded repo context. Grounding rules, View Query Grammar, ALWAYS-dry-run rule, `<view_query>` output format, and text explanation are all present; unit tests (`test_system_prompt_*`, 15 of them) pin the content.

### §11 — Self-check loop

- Both paths share `parse_view_query_from_text` / the script regex (LAST block wins, JSON-parsed — unit-tested incl. malformed/unclosed/multiple-blocks), `self_check_dry_run` (deserialize → validate → dry-run against the real graph store via the domain resolver, a real 170-function implementation), and `actionable_dry_run_warnings` (info/data-source-inherent warnings never burn a refinement turn).
- SDK path: the subprocess's `view_query` message is captured, NOT forwarded; the loop dry-runs server-side, sends `status:"refining"` and re-invokes the subprocess with the dry-run feedback while budget remains; `MAX_REFINEMENT_TURNS = 3`; at the cap the best version is forwarded anyway (logged). Structurally unparseable queries are never forwarded — budget is spent requesting a corrected block, then a visible rejection.
- Native LlmPort path: same shared machinery; dry-run result injected as a synthetic tool_use/tool_result pair; same cap and best-version-at-cap forwarding.
- Conversation history is per WebSocket session (explorer_ws.rs:505) and threaded through both paths (SDK path embeds it as a `<conversation_history>` transcript prefix — verified live in the stub probe — because `query()` has no multi-turn messages parameter; the round-1 `messages:` key was silently ignored by the SDK, so this change is a real fix, not a regression).

## Probes (this sandbox)

- `cargo test -p gyre-server --lib explorer` → 58/58 pass.
- `cargo test -p gyre-server --test explorer_ws_integration` → 9/9 pass (incl. status-progression test asserting on-wire statuses are exactly thinking|refining|ready and that a view_query arrives).
- `check-relative-path-defaults.sh`, `check-byte-slice-truncation.sh`, `check-task-commit-attribution.sh`, `check-arch.sh` → all exit 0.
- Live stub probe of explorer-agent.mjs (details in /tmp/stage/review-evidence/README.md): option passthrough, history embedding, last-block-wins extraction, malformed-JSON passthrough without a view_query message.
- `@anthropic-ai/claude-agent-sdk` is not installed in this sandbox, so the real SDK transport could not run — environment restriction, not a code defect. Workspace-wide `cargo test --all` is rejected by the executor wrapper; focused per-crate probes used instead.

## Findings

None.

## Non-findings (checked, pre-existing at base or deliberate)

- `allowedTools` includes a sixth read-only tool (`node_provenance`) beyond §8's five — documented extension consistent with the system prompt and the served MCP tools; does not contradict the spec's contract.
- `&content[..remaining]` byte-index truncation in the SDK accumulated-text cap (explorer_ws.rs ~2158) can panic on a non-char boundary above 1 MiB — present verbatim at base; the diff moved the surrounding code into the extracted `run_sdk_subprocess_once` without changing these lines. Mechanical gate passes (flags fixed literals only). Recommend a follow-up task to use char-boundary-safe truncation.
- The script streams raw text tokens (including `<view_query>` content) live before the server self-check completes, and accumulated text can duplicate when stream events and complete assistant messages carry the same text — pre-existing streaming behavior, unchanged by this diff; the authoritative `view_query` message only reaches the frontend after the server-side self-check.
- Graph-store-unavailable degraded mode forwards the extracted query without a self-check (logged, commented) — deliberate tradeoff; the agent's own dry-run MCP tool hits the same store.

## Verdict

**complete** — approved. Every acceptance criterion is backed by real production code (verified by reading both agent paths end-to-end and by live probing of everything the script controls), the shared self-check machinery operates on the real graph store and the real domain resolver, and the focused test suites and all relevant mechanical gates pass at the candidate SHA.
