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

Implementation of `realized-model.md` §6 Narrative Generation: `crates/gyre-domain/src/narrative.rs`
(template generator + grounded facts, pure domain logic, no LLM-port import — the LLM call lives
in `gyre-server` because `gyre-domain` MUST NOT depend on the LLM port), wired into
`crates/gyre-server/src/api/graph.rs` (timeline/diff `narrative` field, briefing architecture
narrative with LLM synthesis and template fallback) with the `PROMPT_GRAPH_NARRATIVE` fallback
template in `llm_defaults.rs`. The original "stubbed for now" narrative comment in graph.rs is
gone. Reviewed in `specs/reviews/task-152.md`; the sections below summarize the shipped behavior,
the repair history, and this round's rebase resolution.

### Shipped behavior

- **Template generator** (`crates/gyre-domain/src/narrative.rs`): `NarrativeGrounding::from_graph`
  indexes the live graph (Contains parent, Implements traits, FieldOf fields, node `spec_path`/
  `spec_paths` + GovernedBy edges → specs; soft-deleted nodes/edges excluded, edges to missing
  nodes skipped). `generate_template_narrative` renders per-node additions with module/trait/field
  sentences, removals, modifications with old→new field changes (char-boundary-safe truncation),
  >3 additions grouped per module+type ("N types added to `module`"), the plan's per-edge template
  `"New {edge_type} relationship: \`{source}\` → \`{target}\`."` from `DeltaEdgeEntry` arrays
  (grouped by edge type above threshold; legacy count-only `delta_json` renders unchanged),
  "Governed by spec: X." (grounding specs ∪ commit `spec_ref`, `@sha` stripped), "Produced by
  agent Y under persona Z." `""` for empty/malformed deltas. Fully deterministic.
- **Timeline/diff endpoints**: `DeltaResponse.narrative` on every delta; grounding loaded once per
  request from real `graph_store.list_nodes/list_edges`; per-delta attribution via real
  `agents.find_by_id` + `agent_personas` kv binding (falls back to the stored agent id when the
  agent row is gone; `None` only when the delta records no agent).
- **LLM synthesis** (`llm_architecture_narrative`): grounded facts JSON injected into the
  `graph-narrative` prompt template, model resolved via `resolve_llm_model`, call wrapped in
  `tokio::time::timeout(10s)`. Falls back to concatenated template narratives on unconfigured LLM,
  port error, timeout, or empty completion — each branch logged.
- **Briefing** (`assemble_briefing`): collects the workspace's recent deltas (10 most recent since
  `since`, grounding cached per repo), renders template narratives, and appends
  "Architecture: {narrative}" to the summary — LLM-synthesized when configured, template as the
  quality floor. Both REST and MCP briefing handlers delegate to `assemble_briefing`.
- **Extractor** (`graph_extraction.rs`): records `DeltaEdgeEntry { edge_type, source, target }`
  arrays in agent-context deltas (endpoint qualified names resolved from the merged node state;
  unknown endpoints omit the entry rather than guess a name), bare counts in the compact no-agent
  branch; the removed-edge count fix (`compact_delta_records_true_removed_edge_count`) is covered
  by a discriminating test over a real bare git repo.
- **Docs**: `docs/api-reference.md` timeline row documents `narrative`; briefing row corrected to
  the actual HSI §9 response shape.
- **Tests**: domain narrative suite (addition/removal/modification/grouping/edge-template/count-only/
  empty/malformed, spec governance from GovernedBy edges and `spec_ref` with `@sha` stripped,
  deleted-node exclusion, agent attribution, facts JSON grounding, char-boundary truncation) plus
  server tests driving the real router (`timeline_endpoint_returns_grounded_narrative` asserts
  every clause of the spec's template example; `briefing_summary_falls_back_to_template_narrative_
without_llm`; `briefing_summary_uses_llm_narrative_when_configured` asserts the LLM path ran AND
  the template fallback text does not appear; extraction `edge_entry` resolution and unknown-
  endpoint skipping; `compact_delta_records_true_removed_edge_count`).

### Repair history (condensed)

- Rounds 1–2 (review): edge-detail template renderable from real `delta_json` (above);
  `check-template-substitution.sh` const-span doc-block-bleed false positives fixed with
  planted-bug non-blinding proof (pre-commit-only gate).
- Round 4: `check-template-substitution.sh` SIGPIPE race repaired (single `awk` process, no
  upstream writer); real bug fixed — dropped `edges_removed_count` increment in the edge-entry
  refactor; inherited attribution drift recorded per task-210 precedent.
- Round 5: verification-merge gate failures repaired — rustfmt on the three narrative files and
  the `clippy::map_entry` site rewritten to the async-safe `Entry::Vacant` slot pattern (exactly
  one async grounding load per repo, warn-log fallback preserved).
- Round 6: `cargo test --all` WS integration failure root-caused (process-global
  `ACTIVE_SESSIONS` map + `max_sessions_per_user()=3` evicting a live session across concurrently
  running test servers authenticating as the same dev user); fixed with `ExplorerSessionRegistry`
  scoped to `AppState` plus four `session_registry_*` regression tests, including
  `session_registry_is_per_instance_not_process_global`. Gate repairs on that merge: the
  line-pinned `&raw_preview[..500]` byte-slice exemption converted to `chars().take(500)` (strict
  fix; exemption entry deleted) and `clippy::assertions_on_constants` ×2 read through local
  bindings. This sandbox cannot `accept()` TCP listeners (errno 95,
  `/tmp/stage/capabilities.json`), so `cargo test -p gyre-server --test explorer_ws_integration`
  must run on the exact head on the host / GitHub CI; the `session_registry_*` unit tests are the
  executable local proof and the failure mechanism is structurally impossible with a
  per-`AppState` registry.

### Round 7 — rebase onto main `c5deca80` (this round)

Resolved the assigned rebase merge (candidate `bcd2dd6e` onto base `c5deca80`):

- `crates/gyre-server/src/api/graph.rs` (tests, content conflict): kept BOTH test sets — the
  narrative generation tests (candidate) and the repo-scoped briefing filter tests (main, HSI §1.5
  / task-083). The candidate's tests were updated to main's new 4-arg `assemble_briefing`
  signature (`repo_filter`, passing `None`).
- `crates/gyre-server/src/graph_extraction.rs` (imports, content conflict): union of both sides'
  `gyre-common` imports (candidate `DeltaEdgeEntry` + main `call_graph::{CallGraphRunRecord,
  Language}`).
- Semantic integration beyond marker resolution: (1) `collect_architecture_narratives` now takes
  `repo_filter` and narrows the briefing architecture narrative to the filtered repo, per main's
  contract that the repo filter applies "uniformly to every repo-derived section" (the narrative
  is built from workspace repos' deltas); (2) `compact_delta_records_true_removed_edge_count`
  passes main's new `pass2_telemetry` argument (`CallGraphTelemetry` with real `MemKvStore` +
  `Metrics` sinks, task-076) — follow-up commit `173fa028a` (the merge commit itself
  contains only the marker resolutions and the two semantic integrations above; the
  test-signature adaptation is committed separately as `fix(task-152)` and recorded in
  `commits:` via `dev-attribution.py`).

### Test evidence (round 7, final tree `f9947f11`)

All on the resolved merge, `CARGO_HOME=/tmp/cargo-home`, evidence under
`/tmp/stage/review-evidence/` (`r7-*.log`):

- `cargo build -p gyre-server --lib` → success (cold).
- `cargo test -p gyre-domain --lib narrative` → **18 passed, 0 failed** (includes main's
  view-query narrative tests).
- `cargo test -p gyre-server --lib -- narrative briefing` → **27 passed, 0 failed**.
- `cargo test -p gyre-server --lib graph_extraction` → **31 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::graph` → **31 passed, 0 failed** (both merged test sets).
- `cargo test -p gyre-server --lib session_registry` → **4 passed, 0 failed**.
- `cargo test -p gyre-domain --lib` → **447 passed, 0 failed**;
  `cargo test -p gyre-common --lib` → **116 passed, 0 failed**.
- Gates on the resolved tree vs base `c5deca80`: `check-rustfmt-diff.py` → exit 0;
  `check-clippy-diff.py` → exit 0 ("changed lines clean, 7 Rust files, 1020 existing warnings
  outside changes"); `check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-migration-versions.sh`, `check-dead-message-kinds.sh`, `check-abac-route-registry.sh`,
  `check-mcp-write-tools.sh`, `check-task-commit-attribution.sh` → all exit 0
  (`r7-*.log`); `dev-attribution.py task-152` reports no attribution drift.

Transport restriction (unchanged): this sandbox cannot `accept()` TCP listeners, so the
`explorer_ws_integration` binary and exact-head GitHub CI checks remain mandatory on the host.
`web/dist` was restored from the assignment base after each local build (build.rs regenerates
it); committed `web/dist` in the merge is main's staged state, untouched by this round.

