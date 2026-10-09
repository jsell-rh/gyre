---
title: "Implement graph narrative generation (template-based + LLM-synthesized)"
spec_ref: "realized-model.md §6"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "realized-model.md §6 Narrative Generation"
commits: ["95f1a147e04b1e0ff8b380aa37a9c5556c39af01", "f88e55b70b980e2f9823a51315097d3e8a8b6334", "cb4d2fbc3bdeb64d8b4964b63f2465a49cce71d6"]
---

## Spec Excerpt

From `realized-model.md` §6 — Narrative Generation:

> The briefing/delta view requires human-readable summaries, not raw graph diffs. The forge generates narratives from architectural deltas:
>
> **Template-based** (fast, deterministic):
> ```
> "New type `VectorIndex` added to module `gyre_domain::search`.
>  Implements trait `FullTextPort`. 3 fields: embedding_model, dimension, index_path.
>  Governed by spec: search.md. Produced by agent worker-12 under persona backend-dev v4."
> ```
>
> **LLM-synthesized** (richer, for the briefing view):
> ```
> "The search subsystem gained vector similarity support. A new VectorIndex type
>  was added alongside the existing FtsIndex, both implementing FullTextPort.
>  SearchQuery now supports a mode field (FullText | Semantic). This implements
>  the semantic search requirement added to search.md on March 15."
> ```
>
> Both are grounded in the knowledge graph — the LLM is summarizing structured data, not hallucinating.

## Implementation Plan

1. **Add template-based narrative generator** (`crates/gyre-domain/src/narrative.rs` — new file):
   - Function `generate_template_narrative(delta: &ArchitecturalDelta) -> String`
   - Templates for common delta patterns:
     - Node added: "New {node_type} `{name}` added to module `{parent_module}`. {field_count} fields. {spec_link}. {agent_attribution}."
     - Node removed: "{node_type} `{name}` removed from module `{parent_module}`. {spec_link}."
     - Node modified: "{node_type} `{name}` in `{parent_module}` modified: {field_changes}. {agent_attribution}."
     - Edge added/removed: "New {edge_type} relationship: `{source}` → `{target}`."
   - Each template fills in spec governance and agent provenance when available
   - Handle batching: if a delta has many nodes, group by module and summarize ("5 types added to `gyre_domain::search`")

2. **Integrate template narratives into graph timeline endpoint** (`crates/gyre-server/src/api/graph.rs`):
   - The `GET /repos/:id/graph/timeline` response already returns deltas
   - Add a `narrative` string field to each delta in the response, populated by the template generator
   - The existing "stubbed for now" comment in graph.rs (line ~254) should be replaced with the real implementation

3. **Add LLM-synthesized narrative generation** (behind feature flag or endpoint):
   - Function `generate_llm_narrative(delta: &ArchitecturalDelta, llm: &dyn LlmPort) -> String`
   - Construct a prompt from the structured delta data: node/edge additions/removals, spec references, agent attribution
   - Use the existing LLM port infrastructure (same pattern as briefing LLM synthesis)
   - Fall back to template-based narrative if LLM call fails or is not configured

4. **Wire into briefing endpoint**:
   - The briefing endpoint (`GET /workspaces/:id/briefing`) already synthesizes summaries
   - Feed template narratives into the briefing's architectural change section
   - Use LLM narrative for the briefing's `summary` string (richer prose for human consumption)

5. **Tests**:
   - Unit test: template narrative for single node addition
   - Unit test: template narrative for multiple additions (grouping)
   - Unit test: template narrative includes spec governance when present
   - Unit test: template narrative includes agent attribution when present
   - Unit test: empty delta produces empty narrative

## Acceptance Criteria

- [ ] `generate_template_narrative()` produces human-readable summaries from ArchitecturalDelta
- [ ] Template narratives include spec governance ("Governed by spec: X") when `governed_by` edges exist
- [ ] Template narratives include agent attribution ("Produced by agent Y under persona Z") when provenance exists
- [ ] Timeline endpoint includes `narrative` field in each delta response
- [ ] LLM narrative function exists and falls back to template on failure
- [ ] Briefing endpoint uses narratives for architectural change summaries
- [ ] Tests cover addition, removal, modification, and empty delta cases

## Agent Instructions

- Read `crates/gyre-server/src/api/graph.rs` — find the timeline endpoint and the "stubbed for now" narrative comment (around line 254)
- Read `crates/gyre-common/src/graph.rs` for `ArchitecturalDelta`, `GraphNode`, `GraphEdge` types
- Read `crates/gyre-server/src/api/graph.rs` for the existing briefing integration pattern
- Read `crates/gyre-domain/src/extractor.rs` for the extraction pipeline that produces deltas
- The narrative module goes in `gyre-domain` (pure logic, no infrastructure deps) per hexagonal architecture
- Follow existing patterns in `gyre-domain` for new modules (add `pub mod narrative;` to lib.rs)

## Shipped

Implementation: `crates/gyre-domain/src/narrative.rs` (template generator + grounded facts, pure
domain logic, no LLM-port import — the LLM call lives in `gyre-server` because `gyre-domain` MUST NOT
depend on the LLM port), wired into `crates/gyre-server/src/api/graph.rs` (timeline/diff `narrative`
field, briefing architecture narrative with LLM synthesis and template fallback) with the
`PROMPT_GRAPH_NARRATIVE` fallback template in `llm_defaults.rs`. Reviewed complete in
`specs/reviews/task-152.md` rounds 1–2 (attempt-11); this round repairs a `contract` finding.

**Root cause of the contract finding:** the round-3 commit `afed3fe7` flipped the acceptance-criteria
checkboxes `- [ ]` → `- [x]` in this file. `dev-contract.py:requirement_parts` strips only the
`## Shipped` operational section, so checkbox state is part of the compared prose — the pipeline
correctly read it as a self-authored weakening of the assigned requirements (an implementer cannot
check off its own acceptance criteria). The checkboxes are now left in their assigned unchecked state;
progress and evidence live in frontmatter and this operational section.

- **Template generator** (`crates/gyre-domain/src/narrative.rs`): `NarrativeGrounding::from_graph`
  indexes the live graph (Contains parent, Implements traits, FieldOf fields, node `spec_path`/
  `spec_paths` + GovernedBy edges → specs; soft-deleted nodes/edges excluded, edges to missing nodes
  skipped). `generate_template_narrative` renders per-node additions with module/trait/field sentences,
  removals, modifications with old→new field changes (char-boundary-safe truncation at 60 chars),
  >3 additions grouped per module+type ("N types added to `module`"), count-only legacy `delta_json`
  formats, relationship counts, "Governed by spec: X." (grounding specs ∪ commit `spec_ref`, `@sha`
  stripped), "Produced by agent Y under persona Z." `""` for empty/malformed deltas. Fully
  deterministic.
- **Timeline/diff endpoints**: `DeltaResponse.narrative` on every delta; grounding loaded once per
  request from real `graph_store.list_nodes/list_edges`; per-delta attribution via real
  `agents.find_by_id` + `agent_personas` kv binding (falls back to the stored agent id when the agent
  row is gone; `None` only when the delta records no agent). The old "stubbed for now" comment is gone.
- **LLM synthesis** (`llm_architecture_narrative`): grounded facts JSON injected into the
  `graph-narrative` prompt template (`PROMPT_GRAPH_NARRATIVE` hardcoded fallback), model resolved via
  `resolve_llm_model`, call wrapped in `tokio::time::timeout(10s)`. Falls back to concatenated template
  narratives on unconfigured LLM, port error, timeout, or empty completion — each branch logged.
- **Briefing** (`assemble_briefing`): collects the workspace's recent deltas (10 most recent since
  `since`, grounding cached per repo), renders template narratives, and appends
  "Architecture: {narrative}" to the summary — LLM-synthesized when configured, template as the
  quality floor. Both REST and MCP briefing handlers delegate to `assemble_briefing`.
- **Tests**: 11 domain tests (addition/removal/modification/grouping/count-only/empty/malformed, spec
  governance from GovernedBy edges and `spec_ref` with `@sha` stripped, deleted-node exclusion, agent
  attribution, facts JSON grounding, char-boundary truncation with `"é".repeat(120)`) plus 3 server
  tests driving the real router (`timeline_endpoint_returns_grounded_narrative` asserts every clause
  of the spec's template example; `briefing_summary_falls_back_to_template_narrative_without_llm`;
  `briefing_summary_uses_llm_narrative_when_configured` asserts the LLM path ran AND the template
  fallback text does not appear).
- **Docs**: `docs/api-reference.md` timeline row documents `narrative`; briefing row corrected to the
  actual HSI §9 response shape.
- **Gate repair** (same tree state, reviewed round 2): `scripts/check-template-substitution.sh`
  const-span swallowed the next const's `/// Variables:` doc block (7 false positives on pristine
  main, pre-commit-only so CI never saw it); fixed by comment-stripping the span, proven non-blinding
  by planted-bug probes. `check-task-commit-attribution.sh` drift inherited from the base (main's
  `a781ede2`, feat(task-210), unattributed at base time) is recorded in `specs/tasks/task-210.md`
  `commits:` by commit `04ce9194` — task-063/task-160 precedent; no exemption entries added.
- **Edge-detail template** (this round's substantive repair): the assigned plan's normative edge
  template `"New {edge_type} relationship: \`{source}\` → \`{target}\`."` was unrenderable from the
  shipped `delta_json`, which recorded `edges_added`/`edges_removed` as bare counts. The extractor
  now records `DeltaEdgeEntry { edge_type, source, target }` arrays (endpoint qualified names
  resolved from the merged node state — new edges against final nodes, removed edges against the
  pre-extraction state since their endpoints are soft-deleted by the pass; unknown endpoints omit
  the entry rather than guess a name) when agent context is present, keeping bare counts in the
  compact no-agent branch. `generate_template_narrative` renders the per-edge template at/below
  `GROUP_THRESHOLD` (3), groups by edge type above it ("4 new relationships established (2
  contains, 2 implements)."), and keeps the legacy count sentence for count-only `delta_json`
  (old records render unchanged). `build_narrative_facts` carries the edge arrays to the LLM
  prompt. Backward compatible: `parse_delta_facts` accepts both array and count forms.
  Tests: 4 domain tests (plan-template rendering for added+removed edges, grouping above
  threshold, malformed-entry skipping, facts-JSON edge details) + 2 extraction tests
  (`edge_entry` qualified-name resolution and unknown-endpoint skipping) + 1 compact-delta test.

Test evidence this round (exact commands and output under `/tmp/stage/review-evidence/`,
`CARGO_HOME=/tmp/cargo-home`, `CARGO_TARGET_DIR=/tmp/gyre-target`):

- `cargo test -p gyre-domain --lib narrative` → **15 passed, 0 failed** (11 prior + 4 new
  edge-detail tests) (`domain-narrative-r3b.log`).
- `cargo test -p gyre-domain --lib` → **378 passed, 0 failed** (full lib).
- `cargo test -p gyre-common --lib` → **94 passed, 0 failed** (full lib, `DeltaEdgeEntry`).
- `cargo test -p gyre-server --lib -- narrative briefing` → **20 passed, 0 failed**
  (`server-narrative-briefing-r3.log`; includes the 3 task tests driving the real router and the
  LLM echo/fallback paths).
- `cargo test -p gyre-server --lib graph_extraction` → **19 passed, 0 failed** (17 prior + 2 new
  `edge_entry` tests) (`server-extraction-r3b.log`).
- Mechanical gates: 21/21 PASS including arch, template-substitution, byte-slice-truncation,
  task-commit-attribution, migration-versions, dead-message-kinds, unbounded-external-http,
  inert-enforcement, lossy-secret-conversion (`gates-r3-edge-template.log`).

Transport restriction recorded: this sandbox cannot accept TCP listeners (errno 95,
`/tmp/stage/capabilities.json`), so no live HTTP probe of the timeline/briefing endpoints was run;
the router-driven `oneshot` tests above are the executable proof. Exact-head GitHub CI checks remain
mandatory for the host.

Out of scope, noted for main: `web/dist` committed on main is stale relative to `web/src` (missing
`briefing-since` markup, still shipping `sidebar-badge` markup deleted 2026-03-28) — a main-side
regeneration is a separate task.
