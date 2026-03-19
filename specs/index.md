# Gyre Specifications - Index

This is the entry point for all Gyre specs. Read this first.

## How to Navigate

- Specs are organized by topic into directories with predictable names.
- Every file name describes its contents - optimize for cache hits on search.
- If you're looking for something specific, check the table below before searching.

---

## System Specification

What Gyre does - the product.

| Spec | Path | Summary |
|---|---|---|
| Design Principles | [`system/design-principles.md`](system/design-principles.md) | Core invariants that govern all decisions |
| Source Control | [`system/source-control.md`](system/source-control.md) | Built-in git forge, MRs, merge queue, jj evaluation |
| Agent Runtime & Compute | [`system/agent-runtime.md`](system/agent-runtime.md) | CLI, TTY, WebSocket, compute targets, lifetimes, networking, MCP, protocols |
| Identity & Security | [`system/identity-security.md`](system/identity-security.md) | SPIFFE, SSO/Keycloak, SCIM, ABAC, impersonation, audit |
| Observability & Governance | [`system/observability.md`](system/observability.md) | OTel, eBPF audit, SIEM forwarding, agent auditability |
| Product Analytics | [`system/analytics.md`](system/analytics.md) | Built-in PostHog-like analytics, agent-consumable |
| Activity Dashboard | [`system/activity-dashboard.md`](system/activity-dashboard.md) | Real-time feed, "try it" ephemeral environments |
| Admin Panel | [`system/admin-panel.md`](system/admin-panel.md) | Credentials, secrets, background jobs, logs |
| SDLC Philosophy | [`system/sdlc.md`](system/sdlc.md) | Challenge every ceremony, engineer the need away |
| Business Continuity | [`system/business-continuity.md`](system/business-continuity.md) | BCP primitives, snapshot/restore, retention |

## Development of This System

How Gyre gets built - process and standards for the agent team.

| Spec | Path | Summary |
|---|---|---|
| Architecture & Standards | [`development/architecture.md`](development/architecture.md) | Rust, Svelte, DDD, hexagonal, storage, API |
| Ralph Loops | [`development/ralph-loops.md`](development/ralph-loops.md) | Core loop definition, meta loops, coordination |
| Agent Experience (Day One) | [`development/agent-experience.md`](development/agent-experience.md) | Testing, observability, repo as system of record, cache hits, entropy management |
| Speed & Backpressure | [`development/speed-backpressure.md`](development/speed-backpressure.md) | The wheel, pre-commit hooks, quality gates |
| CI, Docs & Release | [`development/ci-docs-release.md`](development/ci-docs-release.md) | GitHub Actions, Starlight docs, semver, conventional commits |
| Manager Agent Orchestration | [`development/manager-agent.md`](development/manager-agent.md) | Manager Ralph loop, sub-agent dispatch, lifecycle rules |
| Agent Development Workflow | [`development/agent-workflow.md`](development/agent-workflow.md) | Immediate feedback, worktrees, PRs, fix the environment |
| Dogfooding | [`development/dogfooding.md`](development/dogfooding.md) | Building Gyre with agent-boss |
| Development Philosophy | [`development/philosophy.md`](development/philosophy.md) | Speed, failure domains, humans steer / agents execute |

## Prior Art & Lessons Learned

| Spec | Path | Summary |
|---|---|---|
| agent-boss Lessons | [`prior-art/agent-boss.md`](prior-art/agent-boss.md) | Carry forward / don't repeat from the prototype |
| Gas Town Lessons | [`prior-art/gas-town.md`](prior-art/gas-town.md) | GUPP, NDI, molecules, patrols, convoys |

## Personas

| Persona | Path | Summary |
|---|---|---|
| CEO Agent | [`personas/ceo.md`](personas/ceo.md) | Top-level orchestrator, spawns all work |

## Milestones

| Milestone | Path | Summary |
|---|---|---|
| M0: Walking Skeleton | [`milestones/m0-walking-skeleton.md`](milestones/m0-walking-skeleton.md) | First vertical slice - server, CLI, one Ralph loop |
| M1: Domain Foundation | [`milestones/m1-domain-foundation.md`](milestones/m1-domain-foundation.md) | Core domain model, persistence, REST API, dashboard |
| M2: Source Control | [`milestones/m2-source-control.md`](milestones/m2-source-control.md) | Git forge, merge requests, merge queue, agent-commit tracking |
| M3: Agent Orchestration | [`milestones/m3-agent-orchestration.md`](milestones/m3-agent-orchestration.md) | Smart HTTP git, agent spawn, CLI client, end-to-end Ralph loop |
| M4: Identity & Observability | [`milestones/m4-identity-observability.md`](milestones/m4-identity-observability.md) | Keycloak SSO, RBAC, OpenTelemetry, admin panel |
| M5: Agent Protocols | [`milestones/m5-agent-protocols.md`](milestones/m5-agent-protocols.md) | MCP server, A2A discovery, AG-UI events, jj, agent-compose |
| M6: Infrastructure | [`milestones/m6-infrastructure.md`](milestones/m6-infrastructure.md) | Analytics, BCP, cost tracking, job framework |

## Open Questions

| Question | Status |
|---|---|
| SPIFFE integration details | **Resolved** - 3-layer stack: SPIFFE (workload attestation) + Gyre as OIDC provider (agent permissions) + Sigstore/Fulcio (keyless commit signing). Federated via standard protocols. |
| jj (Jujutsu) vs. Git | **Resolved** - jj adds value for agent workflows (atomic changes, auto-rebase, undo) |
| Agent collaboration model (hierarchy + ?) | Open |
| Coordination primitives (blackboard vs. event stream vs. persistent work chains) | Open |
| Persistent Ralph loop steps (NDI pattern) | Open |
| Decision library for learned interrupt resolutions | Open |
| Cost tracking model | Open |
| CI as separate concept vs. emergent property | Open |
| Feature flags as CI alternative | Open |
| SSO/SCIM provider targets | **Resolved** - Keycloak primary, pluggable for others |
