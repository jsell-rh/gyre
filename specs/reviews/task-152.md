# Review — task-152 (realized-model §6 — Narrative Generation: template-based + LLM-synthesized)

Spec: `specs/system/realized-model.md` §6 "Narrative Generation": briefing/delta view requires human-readable summaries generated from architectural deltas — template-based (fast, deterministic, grounded in module/trait/fields/spec/agent facts) and LLM-synthesized (richer prose for the briefing view), both grounded in the knowledge graph so the LLM summarizes structured data rather than hallucinating.

Commits under review: `f88e55b7` (implementation: narrative.rs + graph.rs + llm_defaults.rs + domain lib.rs), `95f1a14` (byte-slice gate annotation follow-up), `e69fa0f` (template-substitution gate false-positive fix, round 2). Docs-only commits `c120478` (coverage row) and `32456cc` (api-reference) are task-labeled but not product-surface; correctly outside `commits:` per the attribution script's scope. Verdict: **complete**.

## Round 1 (this review)

Test runs (`CARGO_HOME=/tmp/cargo-home`, `CARGO_TARGET_DIR=/tmp/gyre-target`):

- `cargo test -p gyre-domain --lib narrative` → **11 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- narrative briefing` → **20 passed, 0 failed**, including the 3 new task tests: `timeline_endpoint_returns_grounded_narrative`, `briefing_summary_falls_back_to_template_narrative_without_llm`, `briefing_summary_uses_llm_narrative_when_configured`.
- Mechanical gates on the tree: check-arch.sh OK (gyre-domain pure, no LLM-port import); check-byte-slice-truncation OK; check-inert-enforcement OK; check-task-commit-attribution OK; check-unbounded-external-http OK (the narrative LLM call is wrapped in `tokio::time::timeout(10s)`); check-lossy-secret-conversion, check-forged-scope-fields, check-relative-path-defaults, check-abac-route-registry, check-abac-exempt-handlers (89 handlers), check-mem-port-contracts all OK. No gate failures, including the two that were pre-existing failures at the time of the task-151 review (mem-port-contracts has since been fixed).

Verified working (no material findings):

- **Template generator is real and fully grounded** (`crates/gyre-domain/src/narrative.rs`). `NarrativeGrounding::from_graph` indexes the live graph (soft-deleted nodes/edges excluded, edges to missing nodes skipped): `Contains` parent→child, `Implements` impl→trait, `FieldOf` field→owner, `GovernedBy` code→spec, plus node `spec_path`/`spec_paths` columns. I verified each assumed edge direction against the real extractor call sites, not just the tests: `rust_extractor.rs:1002-1005` (`make_edge(Contains, module_id, child_id)`), `rust_extractor.rs:1154-1158` (`Implements` from type→trait), `rust_extractor.rs:1051-1055` / `python_extractor.rs:462` / `typescript_extractor.rs:505` / `go_extractor.rs:397-401` (`FieldOf` field→owner), `rust_extractor.rs:386-390` / go/py/ts equivalents (`GovernedBy` code→spec). The grounding contract comment in narrative.rs matches reality.
- **Renderer covers the spec's example shape and the plan's templates**: per-node additions ("New type `X` added to module `M`. Implements trait `T`. N fields: a, b, c."), removals, modifications with old→new field changes (char-boundary-safe `truncate_value` at 60 chars + `…` — the byte-slice gate forced this and it's genuinely correct for multi-byte values; `long_field_change_values_truncated_at_char_boundary` proves it with `"é".repeat(120)`), grouped summaries above GROUP_THRESHOLD=3 ("N types added to module `M`", grouped by module+node_type), count-only legacy delta_json format, relationship counts, "Governed by spec: X." (grounding specs ∪ commit `spec_ref` with `@sha` stripped), "Produced by agent Y under persona Z." Empty/malformed delta → `""` (no fabricated sentences). Unknown facts are omitted, never guessed — matches the spec's grounding requirement.
- **Timeline endpoint wiring is through the real entry point**: `get_graph_timeline` loads grounding once per request via `load_narrative_grounding` (real `graph_store.list_nodes/list_edges` queries), resolves provenance per delta via `delta_attribution` — a real `agents.find_by_id` lookup plus the `agent_personas` kv binding, the same key production code reads (`constraint_check.rs:964`). Falls back to the stored agent id (real recorded provenance) when the agent row is gone; `None` only when the delta records no agent. The old "stubbed for now" comment is gone (grep: no matches in gyre-server). `DeltaResponse.narrative` is additive; the web `TimelineScrubber`/`timeline-utils` consumers parse `delta_json` only, so nothing downstream breaks.
- **LLM synthesis is genuine, bounded, and falls back correctly** (`llm_architecture_narrative`, graph.rs:753-796). Uses the real prompt-template infrastructure (`get_effective(ws, "graph-narrative")` with the new `PROMPT_GRAPH_NARRATIVE` hardcoded fallback that injects the grounded facts JSON and instructs "never invent names, modules, traits, specs..."), `resolve_llm_model`, `for_model`, and a 10s `tokio::time::timeout`. Fallback to concatenated template narratives on unconfigured LLM, port error, timeout, or empty completion — each branch logged. The plan's sketch (`generate_llm_narrative(delta, llm: &dyn LlmPort)` in gyre-domain) would have violated the hexagonal boundary; the shipped split (facts/prompt constants in domain, LLM call in server) is the correct realization and is documented in narrative.rs's module doc. Acceptance criterion "LLM narrative function exists and falls back to template on failure" is satisfied by `llm_architecture_narrative`.
- **Briefing integration**: `assemble_briefing` collects the workspace's recent deltas (repo-listing → per-repo `list_deltas` since `since`, truncated to 10 most recent, grounding cached per repo), renders template narratives, and appends "Architecture: {narrative}" to the summary with LLM synthesis when configured. Both REST (`get_workspace_briefing`) and MCP (`mcp.rs:2792` briefing resource) delegate to `assemble_briefing` — HSI §11 parity holds for the narrative content.
- **Tests kill real bugs, not mirrors.** The domain suite exercises the renderer against `NarrativeGrounding::from_graph` built from synthetic nodes/edges, including deleted-node exclusion, GovernedBy edge facts, and compact count-only formats. The server timeline test drives the REAL router (`api_router().oneshot`) with a seeded graph (module/Contains, Implements edge, spec_path, real agent row, persona kv binding) and asserts every clause of the spec's template example. The LLM-path test is discriminating: `test_state()` wires `MockLlmPortFactory::echo()` (verified: `complete` returns the user prompt), and the test asserts the echoed `LLM_NARRATIVE_USER_PROMPT` appears AND the template fallback text does NOT — it fails if the LLM path regresses to always-template. The fallback test sets `llm = None` and asserts the template narrative lands in the summary.
- **Spec-fidelity details**: the spec's example includes field enumeration ("3 fields: embedding_model, dimension, index_path") and "Governed by spec: search.md. Produced by agent worker-12 under persona backend-dev v4." — all rendered. `MAX_LISTED_FIELDS=8` with "and N more" tail keeps large types readable; `specs_mentioned` dedupes governance across nodes via BTreeSet. Deterministic ordering throughout (sorted, deduped grounding lists) — same delta always renders the same narrative.
- **Docs fixed, not drifted**: `docs/api-reference.md` timeline row documents the new `narrative` field; the briefing row was rewritten from the stale M30-era `{workspace_id, since, summary, deltas}` shape (pre-existing drift that this task's summary-semantics change would have compounded) to the actual response shape including the LLM/template narrative semantics. Coverage row 9 moved `task-assigned`→`implemented` with accurate anchors; 7/8 coverage.

Minor (non-blocking, recorded for completeness):

- `graph/diff` now also renders narratives via the same grounding path — consistent, though the endpoint itself remains the documented "approximation" stub for from/to filtering (§7 row 10 tracks that separately; out of scope here).
- `NARRATIVE_LLM_TIMEOUT_SECS=10` bounds a synchronous GET; acceptable, though a workspace with a slow LLM still pays up to 10s on briefing. If that ever matters, an async/precomputed narrative is the follow-up — not a §6 requirement.
- `delta_attribution`'s persona lookup is a `.filter(|p| !p.is_empty())` on the kv value — an empty-string persona binding is treated as no persona, which is the honest reading.

## Round 2 — gate-repair verification (this review)

Scope: follow-up commit `e69fa0f` ("wip(task-152): preserve sandbox attempt") — the only tree change since round 1 (`git diff 0f0a092..HEAD --stat`: gate script +12/−2, task file). It fixes a false-positive failure of the template-substitution gate that round 1 surfaced when the fixed gate passed at HEAD: the question this round had to settle was whether that pass masked tree state, and whether the comment-strip blinded the gate.

Full probe matrix, re-run live this round (base = `/tmp/t152-base`, verified byte-identical to `git archive main` for all files except the probe-replaced gate script itself):

| Gate | Tree | Result |
|---|---|---|
| old (main's version) | pristine main | **exit 1, 7 false positives** — `predict_graph` ×3 + `briefing_ask` ×4, all from doc-block bleed |
| old | HEAD | exit 1, 8: same 7 + 4 new against `llm_architecture_narrative` (GRAPH_NARRATIVE's span swallows SPECS_ASSIST's doc block — same mechanism) |
| fixed | pristine main | **exit 0** |
| fixed | HEAD | **exit 0** |
| fixed | Probe A/B: `{{rogue_var}}` planted in `PROMPT_BRIEFING_ASK` string value | **exit 1**, correctly attributed to `briefing_ask` |
| fixed | Probe C: `{{graph_context}}` `.replace` dropped in `specs_assist.rs:403` | **exit 1**, correctly attributed (`fn assist_spec`) — the exact task-012 origin class |

Mechanism, confirmed by reading the span code (`scripts/check-template-substitution.sh:47-58`): each const's window runs to just before the next `pub const`, which always includes the NEXT const's `/// Variables:` doc block. `PROMPT_BRIEFING_ASK` (line 24) is followed by `PROMPT_SPECS_ASSIST`'s doc block (`{{spec_path}}, {{spec_content}}, {{graph_context}}, {{instruction}}`), so `briefing_ask` was flagged for 4 doc vars it never templates; likewise `PROMPT_GRAPH_PREDICT` (line 12) swallows `PROMPT_BRIEFING_ASK`'s doc vars (3). The fix strips `//`-leading lines from the span before extracting `{{vars}}` — a doc block can never be part of a string value. Verified the strip cannot blind: no line inside any template string value begins with `//` (awk scan of string-continuation lines: empty), and the consumer-side logic (lines 80-130) is byte-unchanged from main. The `|| true` addition to `grep -oP` is required, not weakening: with comments stripped, some spans now legitimately have zero vars, and `grep` exits 1 on no-match, which would kill the script under `set -euo pipefail`.

Notable context: the old gate's 7 false positives exist on **pristine main** — this bug predates task-152. It was pre-commit-only (`.pre-commit-config.yaml:233`), never wired into `.github/workflows/ci.yml` (grep: no match among its 31 run steps), which is why main's PRs passed. The fix is therefore a true false-positive repair with no coverage regression.

Round-2 verdict: the fix is strictly better than the old gate — removes all false positives (base and HEAD), retains both true-positive classes (probes A/B and C). No exemptions added (zero `template-sub:ok` in tree), no meaningful tests deleted, no gate weakening. `e69fa0f` is scripts-only, outside the attribution gate's product-surface regex (`^(crates/|web/src|web/tests)`), but is recorded in `commits:` so this review round's scope is explicit. Task-152 stands **complete**: round 1 verified the §6 narrative implementation end-to-end; round 2 resolves the only open doubt (gate repair) with a decisive probe matrix.

## Shipped (mirrors task file)

- Template narrative generator grounded in the live knowledge graph (module, traits, fields, spec governance, agent/persona attribution), with grouping, count-only, and empty-delta handling.
- `narrative` field on every timeline and diff delta response, grounded per repo and attributed via real agent/persona lookups.
- LLM-synthesized briefing architecture narrative over grounded delta facts (prompt-template + model-config aware, 10s-bounded) with template narratives as the quality floor on every failure mode.
- Docs: api-reference timeline/briefing rows corrected to the shipped response shapes.
- Gate repair: template-substitution gate's const-span swallowed the next const's `/// Variables:` doc block (7 false positives on pristine main, pre-commit-only so CI never saw it); fixed by comment-stripping the span, proven non-blinding by planted-bug probes for both true-positive classes.

## Round 8 — independent review of the rebase resolution (candidate `08f705782`)

Scope: the exact assigned candidate `08f7057826c94a77d8fe9ae4a561b690f9931bcb` against base
`c5deca801` (full diff: narrative.rs +1062, api/graph.rs +442, graph_extraction.rs +275,
explorer_ws.rs +118, llm_defaults.rs +12, gyre-common DeltaEdgeEntry, gate script repair,
coverage/docs/task/review records). Working tree clean at HEAD = candidate throughout;
`dev-review-guard.py check` exit 0 (no source edits; all mutation probes restored, verified
by `git status --porcelain` empty after each).

Focused probes (all on the exact head, `CARGO_HOME=/tmp/cargo-home`, evidence under
`/tmp/stage/review-evidence/r8-*.log`):

- `cargo test -p gyre-domain --lib narrative` → **18 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- narrative briefing` → **27 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- api::graph graph_extraction session_registry` →
  **66 passed, 0 failed** (both merged test sets: narrative + main's repo-scoped briefing
  filter tests, updated to the 4-arg `assemble_briefing`).
- `cargo test -p gyre-server --test explorer_ws_integration` → **15 passed, 0 failed** on the
  exact head — including `explorer_ws_session_limit_is_per_server_instance`, the regression
  the round-6 fix targeted. The earlier rounds' transport restriction (no `accept()`) is moot
  in the current sandbox: `/tmp/stage/capabilities.json` reports the listener probe supported
  and the real-TCP run confirms it. No host-only deferral remains for this binary.
- Gates: check-arch, check-byte-slice-truncation, check-fail-open-ref-resolution,
  check-inert-enforcement, check-unbounded-external-http (narrative LLM call is
  `tokio::time::timeout(10s)`-bounded), check-mem-port-contracts, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-relative-path-defaults, check-task-commit-attribution →
  all exit 0. `check-rustfmt-diff.py c5deca801` → changed lines clean (7 files). Focused
  `cargo clippy -p gyre-domain --lib` / `-p gyre-server --lib`: zero warnings on
  candidate-changed lines (remaining warnings are pre-existing base lines; the workspace-wide
  clippy-diff invocation is blocked by the executor wrapper — infrastructure, not a defect).

Non-blinding proofs (each mutation reverted immediately, tree verified clean):

1. **Timeline narrative**: `DeltaResponse::from_delta` forced to emit `String::new()` →
   `timeline_endpoint_returns_grounded_narrative` **FAILED** (exit 101). The test proves real
   production narrative generation, not mirroring.
2. **LLM path**: `llm_architecture_narrative` forced to return the fallback (`if true { return
   fallback; }`) → `briefing_summary_uses_llm_narrative_when_configured` **FAILED** while
   `briefing_summary_falls_back_to_template_narrative_without_llm` still passed — exactly the
   discriminating split the AC requires. Echo mock verified genuine
   (`gyre-adapters/src/llm/mock.rs:85-92`: `complete` returns the user prompt, so the asserted
   `LLM_NARRATIVE_USER_PROMPT` in the summary can only appear via a real port round-trip).
3. **Spec governance grounding**: node `spec_path`/`spec_paths` collection disabled in
   `NarrativeGrounding::from_graph` → 2 domain tests **FAILED**
   (`single_addition_renders_grounded_spec_example_shape`,
   `facts_json_carries_grounding`).
4. **Template-substitution gate not blinded by the candidate's script repairs**: baseline
   exit 0; planted `{{narr_mode}}` in `PROMPT_GRAPH_NARRATIVE`'s string value → exit 1,
   correctly attributed to `llm_architecture_narrative` in api/graph.rs. The comment-strip
   (doc-block bleed fix), single-awk span bounds, and here-string grep all retain the
   true-positive class.

Rebase-resolution semantics verified: `collect_architecture_narratives` takes `repo_filter`
and narrows the briefing architecture narrative to the filtered repo (code-verified; main's
`briefing_repo_filter_narrows_sections_to_that_repo` passes on the merged tree);
`compact_delta_records_true_removed_edge_count` drives the real extraction pipeline over a
real bare git repo with main's `pass2_telemetry` argument and passes.

Minor observations (non-blocking, recorded for completeness):

- Grouped additions (>3) omit per-node grounding specs from the "Governed by spec" sentence
  (delta-level `spec_ref` still renders) — defensible resolution of the plan's own tension
  between per-node governance and grouped batching.
- `collect_architecture_narratives` silently skips a repo whose `list_deltas` fails (the
  repo-listing and grounding-load failures are warn-logged; the per-repo delta read is not).
  Advisory summary content degrades gracefully; no wrong data.
- Coverage row 9 cites pre-rebase commit `f88e55b7` — historically accurate (commit exists in
  the repo) and the described code matches the current tree exactly.

Round-8 verdict: **approved** — every §6 acceptance criterion is met by real production code
with discriminating tests; no fake implementations, no gate weakening, no scope-unsafe
handling found in the diff.
