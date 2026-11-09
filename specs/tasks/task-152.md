---
title: "Implement graph narrative generation (template-based + LLM-synthesized)"
spec_ref: "realized-model.md §6"
depends_on: []
progress: ready-for-review
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

At the current merged head (candidate rebased onto base `fefde294`), `CARGO_HOME=/tmp/cargo-home`,
`SKIP_WEB_BUILD=1`, logs under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-domain --lib narrative` → **18 passed, 0 failed** (`r8-domain-narrative.log`).
- `cargo test -p gyre-server --lib -- narrative briefing` → **27 passed, 0 failed**
  (`r8-server-narrative.log`) — includes `timeline_endpoint_returns_grounded_narrative` (asserts
  every clause of the spec's template example), `briefing_summary_falls_back_to_template_
narrative_without_llm`, `briefing_summary_uses_llm_narrative_when_configured`.
- `cargo test -p gyre-server --lib api::graph` → **31 passed, 0 failed** (`r8-server-api-graph.log`)
  — narrative generation tests and main's repo-scoped briefing filter tests, both kept through
  the rebase.
- Earlier rounds on the pre-merge tree additionally passed: full `gyre-domain --lib` (447),
  `gyre-common --lib` (116), `graph_extraction` (31), `session_registry` (4), and the repo gate
  suite (rustfmt/clippy diff-clean vs base, arch/ABAC/migration/attribution checks, exit 0).

### Final round repairs

- Resolved the rebase of the candidate onto base `fefde294`: kept both test sets in
  `api/graph.rs`, union imports in `graph_extraction.rs`; `collect_architecture_narratives`
  honors main's briefing `repo_filter` contract (repo filter applied uniformly to every
  repo-derived section).
- Verification finding repaired: `git diff --check` vs base flagged a new blank line at EOF in
  this file (`specs/tasks/task-152.md:209`); the Shipped notes were rewritten concisely with a
  single trailing newline, and `git diff --check fefde294 HEAD` is now clean.

### CI repair (e2e workflow)

- GitHub E2E run 42623773713 on head `47e58b11` failed at "Install Playwright browsers"
  before any test ran: `ubuntu-latest` now resolves to ubuntu 26.04 and the locked
  Playwright 1.58.2 refuses to install browsers there ("Playwright does not support
  chromium on ubuntu26.04-x64"). Fixed by pinning the e2e job to `ubuntu-24.04`
  (`.github/workflows/e2e.yml`, commit `75fc67f23`), the image the locked Playwright
  supports. A Playwright version bump was rejected: it changes the chromium revision
  and would invalidate the committed visual baselines.
- The suite the failed run never reached was validated locally at head `47e58b11`:
  server built with `SKIP_WEB_BUILD=1` (serves the assignment base's committed
  `web/dist`), run against the staged chromium-1208 that Playwright 1.58.2 expects:
  `npx playwright test` → **65 passed, 0 failed** (1.9m, `r9-e2e-full.log`), including
  `briefing_section_renders` (exercises the narrative-bearing briefing against the
  real server) and every explorer visual regression baseline.

### Verification scope

Exact-head GitHub CI — now the E2E Tests job on the pinned ubuntu-24.04 image — remains
mandatory on the host. Committed `web/dist` is the assignment base's staged state; local
builds ran with `SKIP_WEB_BUILD=1` so it is untouched by this round.
