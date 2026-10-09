---
title: "Implement graph narrative generation (template-based + LLM-synthesized)"
spec_ref: "realized-model.md §6"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "realized-model.md §6 Narrative Generation"
commits: ["95f1a147e04b1e0ff8b380aa37a9c5556c39af01", "f88e55b70b980e2f9823a51315097d3e8a8b6334"]
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

## Shipped

Template narrative generator grounded in the live knowledge graph (`crates/gyre-domain/src/narrative.rs`): `NarrativeGrounding::from_graph` indexes Contains parent modules, Implements traits, FieldOf fields, GovernedBy spec edges and node `spec_path` columns (soft-deleted nodes/edges excluded); `generate_template_narrative` renders per-node additions with module/trait/field enumeration (spec's example shape), removals, modifications with old→new field changes (char-boundary-safe truncation), edge relationship counts, "Governed by spec: X" (grounding ∪ commit `spec_ref`, `@sha` stripped), and "Produced by agent Y under persona Z", with >3 additions grouped per module+type, legacy count-only `delta_json` support, and empty/malformed-delta → empty narrative.

`narrative` field on every timeline and diff delta response (`crates/gyre-server/src/api/graph.rs`): grounding loaded once per request via real `graph_store` queries, per-delta provenance via real `agents.find_by_id` + `agent_personas` kv lookup. The old "stubbed for now" comment is gone.

LLM-synthesized briefing architecture narrative (`llm_architecture_narrative`): real prompt-template + model-config infrastructure, grounded facts JSON injected into the prompt with anti-hallucination instructions, 10s `tokio::time::timeout`; falls back to concatenated template narratives on unconfigured LLM, port error, timeout, or empty completion. `assemble_briefing` feeds template narratives into the architectural change section and the LLM narrative into the briefing `summary`.

Tests: 11 domain unit tests (addition, removal, modification, grouping, spec governance, agent attribution, empty delta, char-boundary truncation, legacy formats) + 3 server tests through the real router (timeline narrative, briefing LLM path, briefing template fallback).

Gate repair (follow-up `e69fa0fe`): the pre-existing template-substitution gate's const-span heuristic attributed the next const's `/// Variables:` doc block to this const, producing 7 false positives on pristine main (pre-commit-only; CI never ran it) — fixed by stripping comment lines from the span, with planted-bug probes proving the fixed gate still catches both true-positive classes.

Resume (cutover-task-152, merged as `b1969768` with base `4b9d61c4`): the retained source `2d854a17` survived the merge with zero diff on every narrative-touched file (`git diff 2d854a17 HEAD -- crates/gyre-domain crates/gyre-server/src/api crates/gyre-common` = admin.rs only, unrelated). Re-verified at merged HEAD: `cargo build -p gyre-domain` clean; `cargo test -p gyre-domain --lib narrative` 11 passed / 0 failed; `cargo test -p gyre-server --lib -- narrative briefing` (see review evidence); check-template-substitution, check-arch, check-byte-slice-truncation, check-task-commit-attribution all exit 0 at the restored commits list. Review history: two prior complete verdicts (rounds 1 and 2 in `specs/reviews/task-152.md`) remain accurate for this tree.

## Agent Instructions

- Read `crates/gyre-server/src/api/graph.rs` — find the timeline endpoint and the "stubbed for now" narrative comment (around line 254)
- Read `crates/gyre-common/src/graph.rs` for `ArchitecturalDelta`, `GraphNode`, `GraphEdge` types
- Read `crates/gyre-server/src/api/graph.rs` for the existing briefing integration pattern
- Read `crates/gyre-domain/src/extractor.rs` for the extraction pipeline that produces deltas
- The narrative module goes in `gyre-domain` (pure logic, no infrastructure deps) per hexagonal architecture
- Follow existing patterns in `gyre-domain` for new modules (add `pub mod narrative;` to lib.rs)
