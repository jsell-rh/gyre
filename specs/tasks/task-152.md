---
title: "Implement graph narrative generation (template-based + LLM-synthesized)"
spec_ref: "realized-model.md §6"
depends_on: []
progress: complete
coverage_sections:
  - "realized-model.md §6 Narrative Generation"
commits: ["173fa028a84c9637d4101f916f9fa20f5fb483a0", "595c39034a3f2a675218e88d60b5861572378477"]
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

Implementation of `realized-model.md` §6 Narrative Generation. Reviewed in
`specs/reviews/task-152.md`; `crates/gyre-domain/src/narrative.rs` holds the template generator
and grounded-facts builder (pure domain logic — the LLM call lives in `gyre-server` because
`gyre-domain` must not depend on the LLM port), wired into the timeline/diff/briefing endpoints.
The original "stubbed for now" narrative comment in `graph.rs` is gone.

### Behavior

- **Template generator**: `NarrativeGrounding::from_graph` indexes the live graph (Contains
  parent, Implements traits, FieldOf fields, node spec paths + GovernedBy edges; soft-deleted
  nodes/edges excluded). `generate_template_narrative` renders per-node addition/removal/
  modification sentences (old→new field changes, char-boundary-safe truncation), groups >3
  additions per module+type ("N types added to `module`"), renders the per-edge template
  `"New {edge_type} relationship: `{source}` → `{target}`."` from `DeltaEdgeEntry` arrays, and
  appends "Governed by spec: X." (grounding specs ∪ commit `spec_ref`, `@sha` stripped) and
  "Produced by agent Y under persona Z." when present. Empty/malformed deltas → `""`.
  Fully deterministic.
- **Timeline/diff**: `DeltaResponse.narrative` on every delta; grounding loaded once per request
  from real `graph_store` queries; per-delta agent attribution via real `agents.find_by_id` +
  `agent_personas` binding.
- **LLM synthesis** (`llm_architecture_narrative`): grounded facts JSON injected into the
  `graph-narrative` prompt (`llm_defaults.rs`), call latency-bounded (`tokio::time::timeout`
  10s); falls back to concatenated template narratives on unconfigured LLM, port error, timeout,
  or empty completion.
- **Briefing** (`assemble_briefing`, repo-filter aware): collects the workspace's recent deltas,
  renders template narratives (LLM-synthesized when configured), and appends
  "Architecture: {narrative}" to the summary. Both REST and MCP briefing handlers delegate to
  `assemble_briefing`.
- **Extractor**: `DeltaEdgeEntry { edge_type, source, target }` arrays recorded in agent-context
  deltas (qualified names resolved from merged node state; unknown endpoints omitted), bare
  counts in the compact no-agent branch.
- **Docs**: `docs/api-reference.md` timeline row documents `narrative`; briefing row matches the
  HSI §9 response shape.

### Test evidence

At the current merged head (candidate rebased onto base `b5a872087`, merge `6184bb3d0`),
`CARGO_HOME=/tmp/cargo-home`, `SKIP_WEB_BUILD=1`, logs under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-server --lib api::graph` → **35 passed, 0 failed** (`r10-server-api-graph.log`)
  — both test sets kept through the rebase: the narrative tests
  (`timeline_endpoint_returns_grounded_narrative`, briefing template-fallback and
  LLM-path tests) and main's concept-view + repo-scoped briefing filter tests.
- `cargo test -p gyre-domain --lib narrative` → **18 passed, 0 failed**
  (`r10-domain-narrative.log`).
- `cargo test -p gyre-server --lib -- narrative briefing` → **27 passed, 0 failed**
  (`r10-server-narrative.log`).
- `cargo test -p gyre-server --test graph_integration` → **38 passed, 0 failed**
  (`r10-graph-integration.log`), `cargo check --all` clean (`r10-check-all.log`),
  `cargo fmt --all --check` clean, and the arch/ABAC-route/migration/attribution/
  dead-message-kind gate scripts all pass at the merge head.
- Earlier rounds on prior heads additionally passed: full `gyre-domain --lib` (447),
  `gyre-common --lib` (116), `graph_extraction` (31), the repo gate suite, and the
  full Playwright e2e suite (65 passed, including `briefing_section_renders`).

### Rebase/merge repairs

- **Onto `fefde294`** (round 8): kept both `api/graph.rs` test sets, union imports in
  `graph_extraction.rs`; `collect_architecture_narratives` honors main's briefing
  `repo_filter` contract.
- **e2e CI** (round 9): GitHub E2E run 42623773713 failed at browser install because
  `ubuntu-latest` resolves to ubuntu 26.04, unsupported by the locked Playwright 1.58.2.
  The e2e job is pinned to `ubuntu-24.04` (commit `75fc67f23`); a Playwright bump was
  rejected because it would invalidate the committed visual baselines. The suite was
  validated locally at that head: 65 passed, 0 failed (`r9-e2e-full.log`).
- **Onto `b5a872087`** (this round, merge `6184bb3d0`): main had landed task-205
  (manifest concept views) and task-104 (briefing-ask ABAC audit event) after the
  candidate branched. Resolutions: kept main's concept-view handlers and ABAC audit
  in `api/graph.rs` and dropped the candidate's stale pre-task-205 substring concept
  search; kept the task-152 narrative section and the narrative-populating timeline
  body; unioned both `api/graph.rs` test sets; `e2e.yml` kept main's identical
  ubuntu-24.04 pin; `realized-model.md` coverage keeps main's task-205 addendum and
  task-152 row 9 (`implemented`) with main's row 10 concept-views note.

### Verification scope

Exact-head GitHub CI — including the E2E Tests job on the pinned ubuntu-24.04 image —
remains mandatory on the host. Committed `web/dist` is the assignment base's staged
state; local builds ran with `SKIP_WEB_BUILD=1` so it is untouched by these rounds.
