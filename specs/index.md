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
| Forge Advantages | [`system/forge-advantages.md`](system/forge-advantages.md) | 8 capabilities only possible with a forge-native agent platform |
| Agent Runtime & Compute | [`system/agent-runtime.md`](system/agent-runtime.md) | CLI, TTY, WebSocket, compute targets, lifetimes, networking, MCP, protocols |
| Identity & Security | [`system/identity-security.md`](system/identity-security.md) | SPIFFE, SSO/Keycloak, SCIM, ABAC, impersonation, audit |
| Observability & Governance | [`system/observability.md`](system/observability.md) | OTel, eBPF audit, SIEM forwarding, agent auditability |
| Product Analytics | [`system/analytics.md`](system/analytics.md) | Built-in PostHog-like analytics, agent-consumable |
| Activity Dashboard | [`system/activity-dashboard.md`](system/activity-dashboard.md) | Real-time feed, "try it" ephemeral environments |
| Admin Panel | [`system/admin-panel.md`](system/admin-panel.md) | Credentials, secrets, background jobs, logs |
| SDLC Philosophy | [`system/sdlc.md`](system/sdlc.md) | Challenge every ceremony, engineer the need away |
| Business Continuity | [`system/business-continuity.md`](system/business-continuity.md) | BCP primitives, snapshot/restore, retention |
| Supply Chain Security | [`system/supply-chain.md`](system/supply-chain.md) | Agent stack attestation, gyre-stack.lock, AIBOM, SLSA provenance, attestation levels |

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
| Accountability Agent | [`personas/accountability.md`](personas/accountability.md) | Spec integrity guardian, detects implementation drift |
| Security Agent | [`personas/security.md`](personas/security.md) | Continuous security review, threat detection, OWASP + agent-specific threats |

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
| M7: Production Hardening | [`milestones/m7-production-hardening.md`](milestones/m7-production-hardening.md) | eBPF audit, SIEM, NixOS, compute targets, WireGuard, hardening |
| M8: Frontend Excellence | [`milestones/m8-frontend-excellence.md`](milestones/m8-frontend-excellence.md) | Red Hat brand, design system, polished UX, component library |
| M9: Functional UI | [`milestones/m9-functional-ui.md`](milestones/m9-functional-ui.md) | Seed data, CRUD modals, auth integration, working user journeys |
| M10: Persistent Storage | [`milestones/m10-persistent-storage.md`](milestones/m10-persistent-storage.md) | SQLite persistence, real-time WebSocket events, git repo storage |
| M11: Agent Execution | [`milestones/m11-agent-execution.md`](milestones/m11-agent-execution.md) | Real agent processes, TTY attach from browser, agent logs |
| M12: Quality Gates | [`milestones/m12-quality-gates.md`](milestones/m12-quality-gates.md) | Merge queue gates (tests/lints/reviews), repo mirroring, diff viewer |
| M13: Forge Native | [`milestones/m13-forge-native.md`](milestones/m13-forge-native.md) | Pre-accept validation, commit provenance, speculative merging, cross-agent awareness |
| M14: Supply Chain Security | [`milestones/m14-supply-chain.md`](milestones/m14-supply-chain.md) | Agent stack fingerprinting, push attestation, AIBOM generation |

## Open Questions

| Question | Status |
|---|---|
| SPIFFE integration details | **Resolved** - 3-layer stack: SPIFFE (workload attestation) + Gyre as OIDC provider (agent permissions) + Sigstore/Fulcio (keyless commit signing). Federated via standard protocols. |
| jj (Jujutsu) vs. Git | **Resolved** - jj adds value for agent workflows (atomic changes, auto-rebase, undo) |
| Agent collaboration model (hierarchy + ?) | **Resolved** - hierarchy (spawn tree) + typed peer messages (A2A) + cross-agent code awareness (M13.4) |
| Coordination primitives (blackboard vs. event stream vs. persistent work chains) | **Resolved** - event stream (domain events via WebSocket) + persistent work chains (task/Ralph ref tree) |
| Persistent Ralph loop steps (NDI pattern) | **Resolved** - Custom ref namespaces (M13.6): refs/ralph/{task-id}/{step} survives agent crash |
| Decision library for learned interrupt resolutions | Open |
| Cost tracking model | **Resolved** - cost entries table, per-agent aggregation, dashboard cost view (M6.1) |
| CI as separate concept vs. emergent property | **Resolved** - emergent: pre-accept gates (M13.1) + merge queue gates (M12.1) = CI without a separate concept |
| Feature flags as CI alternative | Open |
| SSO/SCIM provider targets | **Resolved** - Keycloak primary, pluggable for others |
