# Vision: Amplifying Human Judgment in Autonomous Software Development

> **Status: Draft.** This is the foundational spec. Every other spec should be traceable to the principles here.

## The Thesis

Inference cost falls. Code generation is commoditized. Every company has access to the same models. The bottleneck in software development is no longer producing code — it is the rate at which humans can apply judgment.

Gyre exists to **remove the bottlenecks that prevent humans from reasoning about their systems at the speed the system can execute.** It is an opinionated implementation of a fully autonomous SDLC — from spec to merge — where the human touchpoints are judgment, not labor.

## What Humans Do

In a fully autonomous SDLC, the human's role collapses to:

1. **Decide what to build.** Write and approve specs. This is essential complexity — the domain, the users, the business intent. No model can substitute for knowing your customers.

2. **Set direction on how to build it.** Define and iterate on meta-specs — personas, principles, coding standards. These encode organizational judgment about process, structure, and quality.

3. **Maintain understanding.** See what the system IS — not the code, but the essential design: interfaces, boundaries, data shapes, domain relationships. Understanding is how humans stay in the loop. Without it, they write specs into a void.

4. **Handle exceptions.** Gate failures, reconciliation conflicts, budget constraints, production incidents. The system runs autonomously until it can't — then it escalates to a human with the right context.

5. **Discover and encode.** Recognize patterns — structural, behavioral, domain — that improve the system. Encode them as specs or meta-specs. The system enforces them going forward. Each cycle of discovery compounds.

Everything else — writing code, reviewing PRs, tracking tasks, running CI, managing branches, formatting, linting — is agent work. The system handles it. The human never sees it unless something goes wrong.

## Core Principles

### 1. The Bottleneck Is Judgment, Not Generation

Models generate code. Agents orchestrate. The merge queue processes. Gates validate. All of this is fast, cheap, and getting cheaper.

The scarce resource is human attention. Every surface in the system must earn its place by enhancing human reasoning — not by showing data, but by showing the right data at the right level of abstraction. If a view doesn't help the human make a better decision, it shouldn't exist.

### 2. Right Context, Not More Context

Countless menial things fill human brains today: CI status, PR review queues, task tracking, deployment checklists, formatting nitpicks. These consume attention without enhancing reasoning.

The system strips menial context and surfaces judgment-worthy context:
- Not "47 events happened" but "payment retry is implemented, auth refactor is 60% done, one gate failure needs you"
- Not "here are 50,000 lines of code" but "here are the boundaries, interfaces, and data shapes"
- Not "TASK-047 transitioned to in_progress" but "spec X is being implemented, 3 of 5 sub-tasks complete"

Every surface is a context filter. The system has enormous data. The human's brain is finite. Surface the signal.

### 3. Specs Are the Primary Artifact

Specs declare intent. Everything else — tasks, agents, MRs, branches, commits — is implementation machinery. Humans think at the spec level: "is search implemented?" not "is TASK-047 done?"

Specs are the unit of:
- **Tracking**: implementation progress is reported per spec, not per task
- **Approval**: humans approve specs, not PRs
- **Binding**: code is linked to specs via provenance, not via convention
- **Accountability**: drift is detected at the spec level

The spec is the handle on the code. You read a spec to understand intent, you see its realization in the explorer to understand implementation, you edit the spec to direct change.

### 4. Structure Is Discovered, Not Prescribed

Good structure is a function of the problem itself. You cannot define it up front because you only understand the problem by building. Generic principles (hexagonal, DDD) are starting points, not answers.

The system accelerates structural discovery:
- **Preview mode** lets humans cheaply experiment with structural approaches — churn out attempts, throw away the code, keep the insight
- **The realized model** shows the actual structure so humans can recognize what works
- **Meta-spec encoding** captures structural discoveries durably
- **Reconciliation** propagates structural decisions across the codebase

On prototypes, structure matters for **learning** — you're discovering the right decomposition. In production, structure matters for **velocity** — cheaper to change, more parallelizable work, fewer surprises. In both cases, the code itself is disposable. The learning is the asset.

### 5. The Feedback Loop Is Everything

The system's value is proportional to how fast humans can cycle through:

```
Observe  →  Understand  →  Decide  →  Encode  →  Execute  →  Observe
```

- **Observe**: see the realized model, the briefing, the delta
- **Understand**: comprehend domain relationships, structural patterns, behavioral signals
- **Decide**: "this decomposition is wrong" or "this interface needs to change"
- **Encode**: write or edit a spec or meta-spec
- **Execute**: agents implement autonomously
- **Observe**: see what they produced → loop

Every mechanism in Gyre — the explorer, the preview loop, the briefing, the reconciliation controller, the inbox — exists to make one step of this loop faster. Remove a bottleneck at any step and the whole cycle accelerates.

### 6. Challenge Every Ceremony

Every traditional SDLC ceremony (code review, PRs, approvals, standups, sprint planning) exists because of a real need. But the ceremony is not the need.

For each ceremony:
1. Identify the underlying need (what risk or failure mode does it guard against?)
2. If removing it feels wrong, that feeling is signal — the need is real
3. Engineer the need away. Don't preserve the ceremony; build systems that eliminate the reason we needed it.

Code review exists because humans write bugs. If agents produce tested, gated, spec-bound, provenance-tracked code — the need is addressed without the ceremony. PR stacking exists because human review is slow. If agents review in minutes, stacking is unnecessary. Sprint planning exists because work is unpredictable. If specs decompose into tasks automatically and agents execute autonomously, planning is continuous, not periodic.

### 7. Human Differentiation Compounds

Models are the same for everyone. The differentiator is what humans bring: situational awareness, domain expertise, customer understanding, judgment about tradeoffs. These are applied throughout the SDLC supply chain — not just at the spec level, but at every point where direction is set.

The system amplifies these human capabilities:
- **Situational awareness** → the briefing and realized model give humans a current, accurate picture
- **Domain expertise** → the spec corpus encodes domain-specific decisions that no model contains
- **Customer understanding** → specs capture user intent; the system traces from intent to realization
- **Judgment about tradeoffs** → the preview loop and three lenses (structural, evaluative, observable) provide the context for informed tradeoffs

Each cycle of applying judgment and encoding the result produces a compounding asset: the spec and meta-spec corpus. This corpus is the organization's accumulated understanding of its problem domain, expressed as actionable directives that agents follow. It is not in any model. It cannot be reproduced by a competitor with the same models. It is the moat.

## Baseline Assumptions

These assumptions underpin the design. If any proves false, the system must adapt.

1. **Inference cost and speed continue to fall and rise, respectively.** The economics of autonomous agents improve over time. Re-implementation (reconciliation, preview) becomes cheaper.

2. **Fully auditable software development is required for enterprise trust.** Provenance, attestation, and traceability are not optional features — they are table stakes.

3. **Some form of version control is necessary and useful.** Git (and jj) remain foundational. The forge owns the source of truth.

4. **External integrations slow development and may prevent optimal workflows.** Vertical integration (forge + agent runtime + identity + spec registry) eliminates integration tax.

5. **The current SDLC is built on the assumption that slow humans write code.** An agent-first SDLC does not share this assumption and should look fundamentally different.

6. **Humans must remain in the learning loop.** If humans lose understanding of their systems, they cannot direct them. The system must enhance comprehension, not just productivity.

## Goals

1. **Reduce software bugs and vulnerabilities.** Through gates, attestation, spec binding, and structural enforcement.
2. **Increase development velocity.** Natural consequence of removing judgment bottlenecks and automating everything else.
3. **Reduce human toil during the SDLC.** Every ceremony that can be engineered away, is.
4. **Retain the ability to deliver human vision.** The system amplifies human judgment; it does not replace it. Humans set direction; agents execute.
5. **Accelerate human understanding.** The system shows humans what they need to see to reason well — not more, not less.

## Non-Goals

1. **Reduce time to detect and remediate production failures.** An integrated SRE system should feed production signals back into the SDLC (failure → spec update → human approval → task creation), but production operations are out of scope for this spec.

## Relationship to Other Specs

This spec is the **root**. All other specs should be traceable to a principle here:

| Principle | Specs It Governs |
|---|---|
| Judgment, not generation | system-explorer.md, ui-journeys.md (Inbox, Briefing) |
| Right context, not more | ui-journeys.md (Briefing narrative, action queue), system-explorer.md (moldable views, three lenses) |
| Specs as primary artifact | spec-registry.md, spec-lifecycle.md, spec-links.md, agent-gates.md |
| Structure is discovered | meta-spec-reconciliation.md (preview loop, reconciliation), realized-model.md (knowledge graph), system-explorer.md (ghost overlays) |
| Feedback loop is everything | meta-spec-reconciliation.md (discover → encode → enforce), system-explorer.md (inline editing + preview) |
| Challenge every ceremony | sdlc.md (expanded here), forge-advantages.md |
| Human differentiation compounds | meta-spec-reconciliation.md (meta-spec corpus as moat), platform-model.md (persona model) |
