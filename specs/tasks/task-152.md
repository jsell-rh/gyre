---
title: "Implement graph narrative generation (template-based + LLM-synthesized)"
spec_ref: "realized-model.md §6"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "realized-model.md §6 Narrative Generation"
commits: ["f4e08ad08f2a81e1e96cda6b1382672c80afb16b"]
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

- [x] `generate_template_narrative()` produces human-readable summaries from ArchitecturalDelta
- [x] Template narratives include spec governance ("Governed by spec: X") when `governed_by` edges exist
- [x] Template narratives include agent attribution ("Produced by agent Y under persona Z") when provenance exists
- [x] Timeline endpoint includes `narrative` field in each delta response
- [x] LLM narrative function exists and falls back to template on failure
- [x] Briefing endpoint uses narratives for architectural change summaries
- [x] Tests cover addition, removal, modification, and empty delta cases

## Agent Instructions

- Read `crates/gyre-server/src/api/graph.rs` — find the timeline endpoint and the "stubbed for now" narrative comment (around line 254)
- Read `crates/gyre-common/src/graph.rs` for `ArchitecturalDelta`, `GraphNode`, `GraphEdge` types
- Read `crates/gyre-server/src/api/graph.rs` for the existing briefing integration pattern
- Read `crates/gyre-domain/src/extractor.rs` for the extraction pipeline that produces deltas
- The narrative module goes in `gyre-domain` (pure logic, no infrastructure deps) per hexagonal architecture
- Follow existing patterns in `gyre-domain` for new modules (add `pub mod narrative;` to lib.rs)

## Shipped

Implemented in `f88e55b7` (recovered into this branch by checkpoint `f4e08ad0`), with gate follow-up
`95f1a14` (byte-slice annotation on an index-typed `Vec::truncate`) and this round's `1c4a7bb4`
(restore main's committed `web/dist` — the recovered checkpoint carried an unreviewed shared-build-lane
dist rebuild; `web/src` is byte-identical to base, so the rebuild was pollution, removed per the task-210
round-12 convention that task branches never ship dist rebuilds). The attribution-repair commit at
branch head (`process(task-152): record task-210 commit a781ede2 ...`) records task-210's
`a781ede2` in its own frontmatter to clear attribution drift inherited from the base (same repair the
task-063/task-160 continuation rounds made when the identical base drift hit their gates).

- **Template generator** (`crates/gyre-domain/src/narrative.rs`): `NarrativeGrounding::from_graph` indexes
  the live graph (Contains parent, Implements traits, FieldOf fields, node `spec_path`/`spec_paths` +
  GovernedBy edges → specs; soft-deleted nodes/edges excluded, edges to missing nodes skipped).
  `generate_template_narrative` renders per-node additions with module/trait/field sentences, removals,
  modifications with old→new field changes (char-boundary-safe truncation at 60 chars), >3 additions
  grouped per module+type ("N types added to `module`"), count-only legacy `delta_json` formats,
  relationship counts, "Governed by spec: X." (grounding specs ∪ commit `spec_ref`, `@sha` stripped),
  "Produced by agent Y under persona Z." `""` for empty/malformed deltas. Fully deterministic.
- **Timeline/diff endpoints** (`crates/gyre-server/src/api/graph.rs`): `DeltaResponse.narrative` on every
  delta; grounding loaded once per request from real `graph_store.list_nodes/list_edges`; per-delta
  attribution via real `agents.find_by_id` + `agent_personas` kv binding (falls back to the stored agent
  id when the agent row is gone; `None` only when the delta records no agent). The old "stubbed for now"
  comment is gone.
- **LLM synthesis** (`llm_architecture_narrative`, graph.rs): grounded facts JSON (from
  `build_narrative_facts`) injected into the `graph-narrative` prompt template
  (`PROMPT_GRAPH_NARRATIVE` hardcoded fallback), model resolved via `resolve_llm_model`, call wrapped in
  `tokio::time::timeout(10s)`. Falls back to concatenated template narratives on unconfigured LLM, port
  error, timeout, or empty completion — each branch logged. The LLM call lives in `gyre-server` because
  `gyre-domain` must not depend on the LLM port (hexagonal boundary; the plan's `generate_llm_narrative`
  in gyre-domain would have violated `check-arch.sh`).
- **Briefing** (`assemble_briefing`): collects the workspace's recent deltas (repo listing → per-repo
  `list_deltas` since `since`, 10 most recent, grounding cached per repo), renders template narratives,
  and appends "Architecture: {narrative}" to the summary — LLM-synthesized when configured, template as
  the quality floor. Both REST and MCP briefing handlers delegate to `assemble_briefing`.
- **Tests** (11 domain + 3 server, all passing this round):
  - Domain: single addition renders the spec's example shape; grouping at >3; removals/modifications;
    count-only; empty/malformed → ""; spec governance from GovernedBy edges and `spec_ref` (`@sha`
    stripped); deleted-node exclusion; agent attribution; facts JSON carries grounding; long field-change
    values truncated at char boundary (`"é".repeat(120)`).
  - Server: `timeline_endpoint_returns_grounded_narrative` drives the real router with a seeded graph and
    asserts every clause of the spec's template example; `briefing_summary_falls_back_to_template_narrative_without_llm`;
    `briefing_summary_uses_llm_narrative_when_configured` (echo-mock asserts the LLM path ran AND the
    template fallback text does not appear).
- **Docs**: `docs/api-reference.md` timeline row documents `narrative`; briefing row corrected to the
  actual HSI §9 response shape.
- **Gate repair** (e69fa0f lineage, already on this branch's script state):
  `scripts/check-template-substitution.sh` const-span swallowed the next const's `/// Variables:` doc
  block (7 false positives on pristine main, pre-commit-only so CI never saw it); fixed by
  comment-stripping the span, proven non-blinding by planted-bug probes.

Test evidence this round (`CARGO_HOME=/tmp/cargo`, `CARGO_TARGET_DIR=/tmp/gyre-target`, exact commands
and output under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain --lib narrative` → **11 passed, 0 failed**.
- `cargo test -p gyre-server --lib -- narrative briefing` → **20 passed, 0 failed** (includes the 3
  task tests).

Known pre-existing, repaired this round: `check-task-commit-attribution.sh` failed on the base itself
(commit `a781ede2` "feat(task-210)" landed on main 2026-10-09 without task-210 frontmatter attribution —
drift inherited from base `8c2d1775`, not introduced by this branch; the fix that records it, `63d66b46`,
exists only on an unrelated task-116 branch, not on main). Repaired in the attribution-repair commit at branch head by recording the full
SHA in `specs/tasks/task-210.md` `commits:` — the task-063/task-160 precedent; no exemption entries
added, gate now passes. Still out of scope: `web/dist` committed on main
is stale relative to `web/src` (missing `briefing-since` markup, still containing `sidebar-badge` markup
deleted 2026-03-28) — a main-side dist regeneration is a separate task, not this branch's to ship.
