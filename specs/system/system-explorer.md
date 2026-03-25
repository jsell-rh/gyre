# System Explorer

> **Status: Draft.** Extended by [`human-system-interface.md`](human-system-interface.md) (trust gradient, interrogation, conversation provenance) and [`ui-layout.md`](ui-layout.md) (spatial layout, view spec grammar). Key amendments from those specs:
>
> - **`Cmd+K`** is now global search (not canvas-scoped). Canvas-local search uses `/`.
> - The **Sidebar** layout (Boundaries/Interfaces/Data/Specs) is an **in-view filter panel** (200px, collapsible) inside the Explorer content area, NOT part of the application sidebar.
> - **Ghost overlays** (§3 structural prediction) are deferred — replaced by the Editor Split preview workflow in `ui-layout.md` §9.
> - The Explorer is accessed via a stable 6-item app sidebar (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin).
>
> This spec defines the primary UI for Gyre: a live, navigable model of the realized system where every element links to its spec, specs are editable inline, and changes preview non-destructively.

## Problem

The current UI mirrors the server's API surface: Agents view, Tasks view, MRs view, Repos view. This is like organizing Gmail around "SMTP sessions" and "MIME parts." The user doesn't think in entities — they think in systems, capabilities, and intent.

In a fully autonomous SDLC, the human's job is to understand the system, direct its evolution, and handle exceptions. The UI should serve these activities, not expose CRUD operations on 21 entity types.

The deeper problem: if agents write all code, how does the human maintain understanding of what exists? Code is too low-level. Specs are too high-level. The gap between them is where architectural comprehension lives — and no tool fills it today.

## Core Insight

Code is an act of learning as much as it is an application of learning. When agents do the coding, humans lose the learning that comes from writing it. But understanding can still be acquired through **exploration of the realized architecture** — seeing what exists, why it exists, how it connects, and how it evolves.

The System Explorer makes the realized system the primary object of interaction. Specs are handles on the code — you read them to understand intent, you see their implementations to understand realization, and you edit them to direct change. The spec and its realization are always visible together.

## Why This Is the Centerpiece

### The Bottleneck Is Human Judgment, Not Code Generation

Models are commoditized. Code generation is commoditized. The bottleneck in software development is no longer producing code — it's the rate at which humans can apply judgment: deciding what to build, recognizing what's working, changing course when it isn't.

Today, judgment is bottlenecked by menial context that fills human brains:
- "Did CI pass?" — the system should handle this invisibly
- "Which PR needs review?" — irrelevant when agents review
- "What's the status of TASK-047?" — you shouldn't track tasks, you should track specs
- "Is this formatted correctly?" — gates handle this, you never see it
- "What happened overnight?" — you need a briefing, not an event log

All of this is noise that consumes the finite resource — human attention — without enhancing human reasoning. Strip it away and what's left is the context that actually matters: structural patterns, domain relationships, behavioral signals, the gap between intent and realization.

**The System Explorer exists to present exactly the context that enhances human reasoning, and nothing else.** Every surface is a context filter. The system has enormous amounts of data. The human's brain is the bottleneck. The system's job is to surface the signal, not the noise.

| Bottleneck today | Gyre mechanism | What the human gets |
|---|---|---|
| Writing code to explore designs | Preview mode — churn out attempts, throwaway branches | Structural experimentation at LLM speed |
| Reading code to understand | System explorer — boundaries, interfaces, data shapes | Comprehension without reading a line |
| Tracking autonomous work | Briefing — spec-level narrative | Situational awareness in 30 seconds |
| Enforcing decisions | Meta-spec reconciliation — discover, encode, enforce | Decisions stick without vigilance |
| Understanding production behavior | Observable lens (future) | Behavior mapped to architecture |

### Structure Matters Because It Determines Velocity

Structure is not an end in itself. Obsessing over code organization while competitors ship is Company C — they lose to everyone. The right amount of structural attention is exactly enough to maintain velocity. No more.

But structure IS one dimension of the supply chain where human judgment adds value. Good structure reduces cost-of-change: less tokens per modification, more parallelizable agent work, fewer unintended consequences from changes. Bad structure compounds: each change is harder than the last, agents step on each other, latent bugs emerge through slow SLI degradation.

Good structure is a function of the problem itself. You cannot define it up front. Generic principles (hexagonal, DDD, CQRS) are starting points, but "the right decomposition for this specific problem domain" emerges from building. You discover it by seeing what agents produce, recognizing what works, and encoding that recognition.

The discovery loop:

```
Start with generic principles (meta-specs)
  → Agents produce code
    → Human explores realized model, sees actual structure
      → Human recognizes: "this works" or "this is wrong"
        → Human encodes recognition into spec or meta-spec
          → System enforces going forward + reconciles
            → ...
```

On prototypes, structure matters for LEARNING — not for producing lasting code. You churn out several structural approaches, see which ones make the domain legible, throw away the code and keep the insight. Preview mode makes this cheap: structural experimentation at the cost of a few agent runs.

In production, structure matters for VELOCITY — cheaper to change, more agents can work in parallel, fewer surprises. The structural insights you discovered during prototyping are now encoded as meta-specs and enforced automatically.

### The Right Abstraction Level

When humans stop caring about code syntax — method bodies, variable names, formatting — what remains is the **essential design**: interfaces exposed by boundaries, data shapes that cross them, integration points, behavioral contracts.

This is analogous to the transition from assembly to higher-level languages. You don't review JIT machine code. You shouldn't review method-level syntax either. The explorer presents code at the abstraction level where human judgment adds value: interfaces, boundaries, data, and integration. Implementation detail is a drill-down, not the default view.

The human doesn't need to see the traditional code-level syntax any more than they need to see Java bytecode or JIT-compiled machine code. What they need is something **derived from the output** that enters human consciousness at the right level — high enough to reason about, concrete enough to act on.

### Three Lenses

The realized system can be viewed through three complementary lenses:

| Lens | What It Shows | Why It Matters |
|---|---|---|
| **Structural** | Interfaces, boundaries, data shapes, dependencies | The essential design decisions. What can change independently? What's coupled? |
| **Evaluative** | Test results, gate outcomes, spec assertion status | Evidence of correctness. Proof that the structure produces correct behavior. |
| **Observable** | SLIs, error rates, latency, throughput per endpoint | How structure manifests at runtime. Is the design actually working in production? |

The structural lens is the default. The evaluative lens overlays evidence on the structure. The observable lens (future work — requires production integration) closes the loop between "what we built" and "how it performs."

All three serve the same purpose: enhancing human reasoning about the system. Not showing everything — showing the right things.

## Design

### 1. The Navigable Architecture

The default Explorer view at workspace scope is the **realized architecture** — a visual, interactive representation of the system's current structure. (Note: the overall application landing page after workspace selection is Inbox, per `human-system-interface.md` §1. The Explorer shows realized architecture when the user navigates to it.)

#### Layout

```
+------------------------------------------------------------------+
| Acme Corp > Payments > payment-api          [scope breadcrumb]   |
+------------------------------------------------------------------+
|          |                                                        |
| Sidebar  |  Architecture Canvas (default: boundaries + interfaces)|
|          |                                                        |
| Boundaries| ┌─────────────────────────────────────────────┐       |
| ▸ api    |  │  API Boundary                               │       |
| ▸ domain |  │  POST /agents/spawn ─── POST /agents/complete│      |
| ▸ ports  |  │  GET /tasks ─── PUT /tasks/{id}/status       │      |
| ▸ adapters| └────────────────────┬────────────────────────┘       |
|          |                       │ uses                            |
| Interfaces|  ┌───────────────────▼──────────────────────┐         |
| ▸ TaskPort|  │  Ports (interfaces)                       │         |
| ▸ AgentPort| │  TaskPort: create, get, list, update      │         |
| ▸ RepoPort|  │  AgentPort: create, get, list, spawn     │         |
|          |  │  RepoPort: create, get, list, branches    │         |
| Data     |  └────────────────────┬──────────────────────┘         |
| ▸ Task   |                       │ implemented by                  |
| ▸ Agent  |  ┌───────────────────▼──────────────────────┐         |
|          |  │  Adapters                                  │         |
| Specs    |  │  SqliteTaskRepo ── SqliteAgentRepo        │         |
| ▸ search |  │  SqliteRepoRepo                           │         |
| ▸ auth   |  └──────────────────────────────────────────┘         |
|          |                                                        |
|          |  [Timeline scrubber: ●────────────────────○ Mar 1-23] |
+------------------------------------------------------------------+
|  Detail Panel (contextual — shows selected element)              |
+------------------------------------------------------------------+
```

The canvas is zoomable and pannable. The **default zoom level shows boundaries and interfaces** — the essential design. Types, functions, and implementation detail are visible on drill-down. This is the "higher level code" view: stop caring about method syntax, focus on what boundaries exist, what interfaces they expose, and what data crosses them.

Colors indicate spec coverage (green = governed by spec, amber = suggested link, red = no spec). Size indicates complexity or churn. Lens toggle (top-right): Structural (default) / Evaluative (overlay test status + assertion results) / Observable (future: overlay runtime SLIs).

#### Interaction Model

**Click** a node → Detail Panel shows the entity with its moldable view (see Section 2).

**Double-click** a node → Drill into it (module → types inside it, type → fields and methods).

**Right-click** a node → Context menu: "View spec", "View provenance", "View history", "Open in code."

**Drag** the timeline scrubber → The canvas updates to show the architecture at that point in time. Ghost outlines show where things were added or removed since.

**Search** (`/`) → Find any entity by name, type, or spec within the canvas. Results highlight on the canvas. (Note: `Cmd+K` is now global search per `human-system-interface.md` §1.)

### 2. Moldable Views

Every entity type gets a view tailored to what makes it understandable. These are not generic "detail panels" — they surface the information that matters for that kind of thing.

#### Type View

When you click a type (e.g., `Task`):

```
┌──────────────────────────────────────────┐
│ struct Task                    [pub]      │
│ Spec: platform-model.md ← [click]        │
│ Last modified: agent worker-12, 2d ago    │
│ Persona: backend-dev v3                   │
├──────────────────────────────────────────┤
│ Fields:                                   │
│   id: Id                                  │
│   title: String                           │
│   status: TaskStatus ← [click]            │
│   repository_id: Id → Repository          │
│   assigned_to: Option<Id> → Agent         │
│   priority: Priority ← [click]            │
│   parent_task_id: Option<Id> → Task       │
├──────────────────────────────────────────┤
│ Implements:                               │
│   (none — this is a domain entity)        │
│                                           │
│ Used by:                                  │
│   TaskPort (trait) — CRUD operations      │
│   SqliteTaskRepo (impl)                   │
│   handle_create_task (endpoint)           │
│   TaskBoard.svelte (component)            │
├──────────────────────────────────────────┤
│ Story:                                    │
│ Created in M1 (domain-foundation). Fields │
│ added over M3 (parent_task_id), M22       │
│ (workspace scoping). 14 modifications     │
│ across 8 milestones.                      │
├──────────────────────────────────────────┤
│ Risk: churn=3/30d, coupling=medium,       │
│        spec_coverage=high, tests=94%      │
└──────────────────────────────────────────┘
```

Every field type is clickable → navigates to that type. Every spec reference is clickable → opens spec inline. Every agent reference is clickable → shows provenance.

#### Trait View

When you click a trait (e.g., `TaskPort`):

```
┌──────────────────────────────────────────┐
│ trait TaskPort                  [pub]     │
│ Spec: architecture.md (hexagonal ports)  │
│ Crate: gyre-ports                        │
├──────────────────────────────────────────┤
│ Methods:                                  │
│   create(&self, task: Task) -> Result     │
│   get(&self, id: Id) -> Option<Task>      │
│   list(&self, filter: TaskFilter) -> Vec  │
│   update(&self, task: Task) -> Result     │
├──────────────────────────────────────────┤
│ Implementations:                          │
│   MemTaskRepo (gyre-adapters) — in-memory │
│   SqliteTaskRepo (gyre-adapters) — SQLite │
│   DieselTaskRepo (gyre-adapters) — Diesel │
├──────────────────────────────────────────┤
│ Dependents (who uses this trait):         │
│   gyre-domain (6 call sites)             │
│   gyre-server (3 call sites via DI)      │
└──────────────────────────────────────────┘
```

#### Endpoint View

When you click an API endpoint:

```
┌──────────────────────────────────────────┐
│ POST /api/v1/agents/spawn       [Admin+] │
│ Spec: platform-model.md §3              │
│ Handler: handle_spawn (api/spawn.rs:42)  │
├──────────────────────────────────────────┤
│ Request body:                             │
│   { name, repo_id, task_id, branch,      │
│     parent_id?, compute_target_id? }      │
│                                           │
│ Response 201:                             │
│   { agent, token, worktree_path,         │
│     clone_url, branch, container_id? }    │
├──────────────────────────────────────────┤
│ Flow:                                     │
│   auth → ABAC check → budget check →     │
│   create agent → mint JWT → provision    │
│   worktree → assign task → respond       │
├──────────────────────────────────────────┤
│ Gates: RequireDeveloper, ABAC policy     │
│ Tests: 4 integration tests covering this │
└──────────────────────────────────────────┘
```

#### Spec View (Inline)

When you click a spec linkage badge anywhere in the explorer, the spec opens **inline** in the detail panel — not in a separate view. You see the spec content alongside the code elements it governs:

```
┌──────────────────────────────────────────┐
│ specs/system/search.md          [edit]   │
│ Status: Approved (SHA: a1b2c3)           │
│ Owner: user:jsell                        │
│ Approval: human (jsell) + agent (acct)   │
├──────────────────────────────────────────┤
│ # Search                                 │
│                                           │
│ Full-text search across all entities...  │
│ [full spec content, editable]            │
│                                           │
├──────────────────────────────────────────┤
│ Implements (realized):                    │
│   SearchService (module) ✓               │
│   SearchQuery (type) ✓                   │
│   SearchResult (type) ✓                  │
│   FullTextPort (trait) ✓                 │
│   GET /api/v1/search (endpoint) ✓        │
│   SearchBar.svelte (component) ✓         │
│                                           │
│ Implementation completeness: 6/6 (100%)  │
├──────────────────────────────────────────┤
│ [Preview]  [Publish]  [History]          │
└──────────────────────────────────────────┘
```

### 3. Inline Spec Editing with Progressive Preview

> **Note:** The ghost overlay UX (§3.2-3.3 below — real-time canvas overlays as user types) is **deferred**. The replacement workflow is the Editor Split preview in `ui-layout.md` §9: edit a spec/meta-spec → click Preview → agent implements on throwaway branch → view architectural diff. The `POST /repos/{id}/graph/predict` endpoint remains in scope — it powers the preview's architectural impact analysis. The ghost overlay visual language and real-time canvas updates below are retained as future design direction but are NOT part of the current milestone.

This is the core interaction. The human sees the realized architecture, clicks a spec, edits it, and sees the projected impact — all in one view.

#### The Edit → Preview Flow

1. **Click [edit]** on any inline spec. The spec content becomes editable (markdown editor).

2. **Edit the spec.** As you type, the system responds progressively:

   **Instant (milliseconds)** — Link graph impact:
   - Which other specs are connected (via spec links)
   - Which repos implement this spec
   - How many nodes in the knowledge graph are governed by this spec
   - Shown as a subtle info bar above the editor

   **Fast (2-5 seconds)** — Structural prediction:
   - LLM analyzes the spec diff against the current knowledge graph
   - Predicts: new types/traits to add, existing types to modify, dependencies to change
   - Shown as **ghost nodes** on the architecture canvas — dotted outlines where new elements would appear, yellow highlights on elements that would change, strikethrough on elements that would be removed
   - The canvas updates live as you edit

   **Thorough (minutes, on-demand)** — Full preview:
   - Click [Preview] to spawn an agent that implements the changed spec on a throwaway branch
   - Real code produced, real diff generated
   - The architecture canvas updates with actual extracted nodes from the preview branch (not predictions)
   - Diff viewer available: existing code vs. preview code

3. **Iterate.** Adjust the spec, see new ghost overlay, adjust again. The ghost overlay is the fast feedback loop. Full preview is for validation.

4. **Publish.** Click [Publish] → the spec change enters the approval flow. On approval, agents implement for real. The ghost nodes become real nodes.

#### Ghost Overlay Visual Language

| Visual | Meaning |
|---|---|
| Dotted outline, green fill | New element predicted to be added |
| Yellow highlight on existing node | Existing element predicted to change |
| Red strikethrough on existing node | Existing element predicted to be removed |
| Dotted edge, green | New relationship predicted |
| Pulsing border | Currently being previewed (agent running) |
| Solid border (after preview) | Confirmed by actual agent implementation |

#### Structural Prediction

The "fast" tier uses the knowledge graph + LLM to predict impact without running an agent:

```
POST /api/v1/repos/{id}/graph/predict
{
  "spec_path": "specs/system/search.md",
  "draft_content": "<full spec with changes>"
}

// Response (2-5 seconds)
{
  "predictions": [
    { "action": "add", "node_type": "Type", "name": "VectorIndex", "confidence": 0.85,
      "reason": "Spec mentions 'vector similarity search', likely needs an index type" },
    { "action": "modify", "node_id": "<existing-SearchQuery-id>", "confidence": 0.90,
      "changes": ["New field: mode (FullText | Semantic)"],
      "reason": "Search needs to distinguish between full-text and semantic modes" },
    { "action": "add", "node_type": "TraitMethod", "name": "search_similar", "confidence": 0.75,
      "parent_trait": "FullTextPort",
      "reason": "Semantic search likely requires a new method on the search port" }
  ],
  "affected_specs": ["dependency-graph.md"],
  "affected_repos": ["gyre-server", "search-worker"],
  "estimated_agent_cost": "$0.45"
}
```

Predictions are probabilistic. Confidence scores are shown in the ghost overlay (higher confidence = more opaque ghost). The human understands these are predictions, not certainties — the full preview provides certainty.

### 4. Concept Views

Cross-cutting views that show a concept (e.g., "Authentication") by pulling related elements from across the codebase. Defined in the spec manifest or auto-suggested by the knowledge graph.

The canvas reorganizes to show only elements related to the selected concept. Everything else fades to background. Relationships between concept elements are highlighted.

The user can:
- Browse predefined concepts (from manifest)
- Create ad-hoc concepts by selecting nodes and grouping them
- Ask "show me everything related to X" (conversational, LLM-powered)

### 5. Flow Traces

Behavioral views that show how data flows through the system for a specific operation:

**Example: "What happens when an agent pushes code?"**

```
git push (agent)
  │
  ▼
git-receive-pack (endpoint) ──spec: source-control.md
  │
  ├──▶ JWT auth validation ──spec: identity-security.md
  │
  ├──▶ Pre-accept gates ──spec: agent-gates.md
  │     ├── ConventionalCommit
  │     ├── TaskRef
  │     └── NoEmDash
  │
  ├──▶ Commit provenance recording ──spec: forge-advantages.md
  │
  ├──▶ Spec lifecycle check ──spec: spec-lifecycle.md
  │     └── (auto-task if specs/ changed)
  │
  ├──▶ Domain event broadcast ──spec: source-control.md
  │
  └──▶ Speculative merge attempt ──spec: forge-advantages.md
```

Each step is clickable → navigates to the implementing code, the governing spec, or the agent that last modified it. Flow traces are extracted from the call graph in the knowledge graph, annotated with spec linkage.

Predefined flows for common operations (agent spawn, MR merge, spec approval). Users can trace custom flows by selecting a starting function.

### 6. Architectural Timeline

A time-scrubber at the bottom of the canvas. Drag it to see the system at any point in history.

**What changes as you scrub:**
- The canvas shows the knowledge graph at that point in time
- Ghost outlines show elements that have been added since (forward ghosts) or will be removed (backward ghosts)
- The sidebar shows the delta: "Between then and now: +12 types, -3 types, +2 traits, 8 types modified"

**Key moments** are marked on the timeline:
- Spec approvals (spec X approved → implementation began)
- Milestone completions
- Reconciliation events (persona v3 → v4)
- Major structural changes (new crate added, trait split)

Click a key moment → the delta panel shows what changed and why (narrative from the knowledge graph).

### 7. Risk Map

An overlay on the architecture canvas showing risk metrics from the knowledge graph:

**Heat map mode:**
- Color nodes by churn rate (cool blue = stable, hot red = high churn)
- Or by coupling score, spec coverage, complexity, test coverage

**Anomaly callouts:**
- "Module X has high complexity (42) but low test coverage (23%)"
- "These 3 modules always change together but have no shared spec"
- "Type Y was created by an agent but has no governing spec"
- "This trait has 5 implementations but only 2 are tested"

The risk map helps the human identify where to direct attention — where understanding is most needed and where problems are likely to emerge.

### 8. Conversational Exploration

An AI-assisted exploration mode where the human asks questions and the system answers from the knowledge graph:

> **Q:** "Why does SearchService implement both FullTextPort and CachePort?"
>
> **A:** "SearchService was initially created to implement search.md (commit abc, agent worker-7). CachePort was added during the performance-optimization.md reconciliation (commit def, agent worker-12) when persona v3 introduced the principle 'all read-heavy services should cache results.' The two traits were not planned together — CachePort was retrofitted. Consider whether a dedicated CacheService would be cleaner."

The answer is grounded in the knowledge graph: provenance records, spec linkage, architectural timeline. The LLM synthesizes, but every claim is traceable to an artifact.

**The human can follow up:**
> "Show me other services that also implement CachePort."
> → Canvas highlights all types implementing CachePort.

> "What would happen if I removed CachePort from SearchService?"
> → Structural prediction shows the impact.

### 9. Executable Spec Assertions

Specs can contain structural assertions that are checked against the knowledge graph on every push:

```markdown
## Invariants

<!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->
- `gyre-domain` MUST NOT depend on `gyre-adapters`

<!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->
- `SearchService` MUST implement `FullTextPort`

<!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->
- All API endpoints MUST have auth middleware
```

Assertion results appear in the spec's inline view: green checkmark for passing, red X for failing. Failures appear in the Inbox as action items.

This makes specs **live documentation that validates itself**. Architecture decisions don't drift because they're continuously verified against the realized model.

## Relationship to Existing Specs

**Depends on:**
- **realized-model.md** — the knowledge graph is the data layer for all views
- **meta-spec-reconciliation.md** — progressive preview extends the meta-spec preview endpoint to spec changes
- **platform-model.md** — workspace/repo scoping determines what's visible

**Supersedes (UI portions of):**
- **activity-dashboard.md** — replaced by the briefing narrative and timeline
- **admin-panel.md** — admin functions move to the Admin journey
- Current dashboard entity views (AgentList, TaskBoard, etc.) become drill-downs within the explorer, not primary navigation

**Extends:**
- **forge-advantages.md** — the explorer is a new forge advantage: no external tool can build this because no external tool owns code + specs + provenance + meta-specs in one process
- **spec-registry.md** — inline spec editing uses the spec approval flow; spec assertions are a new manifest feature
