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

## Round 5 — independent re-review of candidate 494128e0 (base f4acb4eb)

Full history re-verified at the exact assigned candidate (HEAD == 494128e0). Scope: the assigned
diff `f4acb4eb..494128e0` (narrative.rs new, graph.rs, graph_extraction.rs, graph.rs API wiring,
llm_defaults.rs, common graph.rs, gate hardening, docs, coverage row 9, task file) plus the merge
topology — candidate is base `f4acb4eb` (task-189) merged into the recovered round-3 checkpoint
`03b928c5` at `f29a4fdc`, with round-4 repairs `e5d4452e`; narrative/product files byte-identical to
reviewed `c14e5769`; no unrelated-file drift (personas.rs, task-189/210 files identical to base).

Test runs on the candidate tree (`CARGO_HOME=/tmp/cargo`, `CARGO_TARGET_DIR=/tmp/gyre-target`;
logs under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain --lib narrative` → 15 passed, 0 failed.
- `cargo test -p gyre-domain --lib` → 378 passed, 0 failed.
- `cargo test -p gyre-common --lib` → 94 passed, 0 failed.
- `cargo test -p gyre-server --lib -- narrative briefing` → 20 passed, 0 failed (incl. the three
  task tests: timeline narrative, template fallback without LLM, LLM path used when configured).
- `cargo test -p gyre-server --lib graph_extraction` → 20 passed, 0 failed (incl.
  `compact_delta_records_true_removed_edge_count` driving the real pipeline over a real bare git
  repo).
- Mechanical gates: 20/20 PASS (arch, template-substitution, byte-slice-truncation,
  task-commit-attribution, migration-versions, dead-message-kinds, unbounded-external-http,
  inert-enforcement, lossy-secret-conversion, forged-scope-fields, relative-path-defaults,
  mem-port-contracts, mcp-write-tools, abac-route-registry, abac-exempt-handlers,
  scope-literal-defaults, fabricated-scope-defaults, migration-sql-portability,
  forwarded-header-trust, in-memory-state-stores).

Mutation probes (source mutated, test run, source restored byte-exact; tree verified clean after):

1. Dropped `edges_removed_count += 1` from the removed-edges loop (the round-4 fix) →
   `compact_delta_records_true_removed_edge_count` FAILS (`left: 0, right: 1`). Discriminating.
2. Disabled grounding in `load_narrative_grounding` (returned `NarrativeGrounding::default()`) →
   `timeline_endpoint_returns_grounded_narrative` FAILS (narrative degrades to ungrounded
   qualified-name fallback; "Governed by spec"/"Implements trait" clauses vanish). Discriminating.
3. Replaced the LLM success arm with the template fallback (`Ok(Ok(text)) => fallback`) →
   `briefing_summary_uses_llm_narrative_when_configured` FAILS. Discriminating — the LLM path is
   genuinely exercised via `MockLlmPortFactory::echo()` (verified: `complete` returns the user
   prompt, and the test asserts the echoed prompt appears AND template text does not).
4. Planted `{{rogue_review_var}}` in `PROMPT_GRAPH_NARRATIVE`'s string value →
   check-template-substitution.sh exits 1 naming the narrative consumer. The hardened gate
   (single-awk spans, here-string grep) is non-blinding for the new template.

Findings assessed and judged non-blocking:

- **`graph-narrative` missing from `LLM_FUNCTION_KEYS`/`VALID_FUNCTION_KEYS`.** The narrative LLM
  path reads `prompt_templates.get_effective(ws, "graph-narrative")` and
  `resolve_llm_model(ws, "graph-narrative")`, but neither registry lists the key, so the
  REST prompt-template / LLM-config CRUD rejects `graph-narrative` (404/400 "unknown function
  key"). In practice the hardcoded `PROMPT_GRAPH_NARRATIVE` fallback and
  `DEFAULT_LLM_MODEL`/`GYRE_LLM_MODEL` serve both resolutions, so the §6 acceptance criteria
  (narrative + fallback) are met; the key gap only blocks workspace-level override of the
  narrative prompt/model. `explorer-generate` shows the registration pattern for a follow-up.
  Recorded here as tracked follow-up debt, not a §6 contract breach.
- **`agent_personas` kv writes exist only in tests** (constraint_check.rs test seed, this task's
  test seed). The persona in "Produced by agent Y under persona Z." renders only when some
  external actor seeds that binding. The lookup mirrors the pre-existing production reader
  (constraint_check.rs:964, introduced d7940e85), so this task did not widen the gap, and the
  attribution degrades honestly to "agent Y" without it. Non-blocking.
- `delta_attribution` falls back to the stored agent id when the agent row is gone — real recorded
  provenance, not fabricated identity; consistent with the no-fabrication rules.
- Timeline narrative ordering follows SQLite's `timestamp ASC`; Mem store is insertion-order.
  Pre-existing adapter divergence, out of this task's scope.

Verdict: **complete** — every §6 acceptance criterion is satisfied by production code at the
reviewed commits, tests discriminate (proven by mutation), gates pass, hexagonal boundary holds
(narrative.rs is pure domain; LLM call lives in gyre-server), and the review scope matches the
`commits:` frontmatter (all three SHAs verified ancestors of the candidate).
