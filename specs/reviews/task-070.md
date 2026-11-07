# Review — task-070 (LLM Explorer Agent — Configuration, System Prompt & Self-Check Loop)

Spec: `specs/system/explorer-implementation.md` §7 (LLM Agent, Claude Agent SDK), §8 (Agent Configuration), §10 (Agent System Prompt), §11 (Self-Check Loop).
Candidate: `d98a53cd` (base `ca293ef`; attributed product commits `fb1a668`, `7ea40e7`, `a58467b`, `a885c918` — all verified ancestors of the base). Requirements-changed round: merge of the retained checkpoint `3cff085f` onto task-069's landed main.
Verdict: **approved**.

## Probes run (evidence in `/tmp/stage/review-evidence/task-070-r6-*`)

This sandbox permitted compilation, so the checks deferred by the prior round's transport restriction were run here:

- `cargo test -p gyre-server --lib explorer` → **59 passed, 0 failed** (`task-070-r6-lib-explorer.log`).
- `cargo test -p gyre-server --test explorer_ws_integration` → **11 passed, 0 failed** (`task-070-r6-integration.log`).
- **Mutation probe**: temporarily set `MAX_REFINEMENT_TURNS = 2`, re-ran the §11 tests → `explorer_ws_self_check_sends_best_version_after_cap` **FAILED** ("exactly MAX_REFINEMENT_TURNS (3) refinement turns may run, got 2"); source restored, tree verified clean (`task-070-r6-mutation-cap2.log`). The cap test kills a real regression — not weak coverage.
- `bash scripts/check-relative-path-defaults.sh` → OK.
- `bash scripts/check-task-commit-attribution.sh` → OK (all four product commits in frontmatter; process commits exempt).
- `node --check scripts/explorer-agent.mjs` → OK; `@anthropic-ai/claude-agent-sdk@^0.2.81` declared in `scripts/package.json`.
- Lineage/diff evidence: `task-070-r6-ancestry-and-diff.txt` (base→candidate product diff empty; all attributed commits ancestors of base).

## Requirements-changed round audit (why "no code changes" is sound)

- `git diff ca293ef d98a53cd -- . ':!specs/tasks'` is **empty**: every product file at HEAD is byte-identical to the base. The candidate's own changes are the task-file progress/notes only.
- The base descends from this task's lineage (all four attributed commits are ancestors), and the reverse-diff against the retained checkpoint `3cff085f` contains only base-side improvements: best-version tracking in both §11 loops, call-graph completeness integration (task-076/lsp-call-graph §8), and a reworded `graph_summary` tool description. Nothing task-070-specific is missing at HEAD — the merge-superset claim was verified independently, not taken from the shipped notes.

## Acceptance criteria (all verified at HEAD)

- **§7–8 SDK path**: `scripts/explorer-agent.mjs` imports `query()` from `@anthropic-ai/claude-agent-sdk`; model resolves request → `GYRE_LLM_MODEL` → `'claude-sonnet-4-6'` (server side via `resolve_llm_model_with_default(..., EXPLORER_SDK_DEFAULT_MODEL)` in `llm_helpers.rs`; the native Vertex fallback keeps the gemini default — correct, claude model names are invalid there); `mcpServers.gyre` is `type:'http'` at `${server_url}/mcp` with `Authorization: Bearer ${token}`; `allowedTools` covers all five §8 tools plus the read-only `node_provenance` (criterion is "includes all 5"); built-in tools disabled (`tools: []`) so the agent is read-only analysis; `permissionMode: 'bypassPermissions'` is safe given `tools: []` + read-only MCP allowlist. Subprocess spawn uses the dispatch site's canonicalized, validation-passing path (round-2 fix intact), clears the environment, passes the model via `GYRE_LLM_MODEL`, and is latency-bounded (60 s deadline + process-group kill). The master-token SDK path is admin-gated.
- **§10 system prompt**: `build_system_prompt_body()` carries the full workflow; SDK variant's step 1 is spec-verbatim ("Call graph_summary to understand the codebase structure"); dry-run-before-finalize, refine-on-warnings, `<view_query>` output block, and text explanation present; content covered by lib tests.
- **§11 self-check loop, both paths**: extract LAST `<view_query>` block (`parse_view_query_from_text` / script regex), JSON-parse, `self_check_dry_run()` (deserialize → validate → dry-run against the real graph store), actionable-warning classification shared by both paths, feed back with `status: "refining"` (native: synthetic tool_use/tool_result pair; SDK: refinement-round question), cap at `MAX_REFINEMENT_TURNS = 3`, best version (fewest actionable warnings) sent at cap, structurally unparseable queries never forwarded when the graph store is up. Proven live by the two §11 integration tests plus the mutation probe above.
- **Conversation history**: per-session `Vec<ConversationMessage>` threaded into both paths; SDK path serializes it into a `<conversation_history>` transcript prefix (`query()` has no multi-turn messages parameter); native path carries full tool-use blocks; capped at 20 entries.
- **`cargo test --all`**: full workspace suite belongs to the verification stage; focused explorer suites (59 + 11) pass here, and the candidate's product diff is empty against a base that already passed task-069 verification.

## Non-blocking observations (pre-existing at the base, untouched by this candidate — recorded, not findings)

- `explorer_ws.rs:2178`: `&content[..remaining]` truncation at the 1 MiB accumulation cap can panic if the cut lands mid-multibyte-character (AGENTS.md byte-slice flaw class; not caught by the exemption-frozen check script). SDK-path only, requires >1 MiB assistant output with an unlucky boundary.
- `explorer_ws.rs:~3106`: stale comment "MAX_TOOL_TURNS (5) … = 8" vs actual consts (8 + 3 = 11).
- SDK path with graph store unavailable forwards the extracted query without self-check (warn-logged) while the native path dead-ends — divergent degradation on an unspecced error path; defensible (frontend canvas holds graph data).
