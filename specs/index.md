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
| **Vision** | [`system/vision.md`](system/vision.md) | **Root spec:** Gyre amplifies human judgment across the SDLC. Seven principles: judgment not generation, right context, specs as primary artifact, structure is discovered, feedback loop, challenge every ceremony, human differentiation compounds |
| **Platform Model** | [`system/platform-model.md`](system/platform-model.md) | **Foundational spec:** tenant/workspace/repo hierarchy, persona model, orchestration, MCP coordination, budgets, rollback, secrets, bootstrap |
| **Ralph Loop** | [`system/agent-runtime.md`](system/agent-runtime.md) §1 | **Core implementation primitive:** agent/session model, message inbox, fresh context per session, gates + agent review as terminal conditions, provenance integration |
| User Management & Notifications | [`system/user-management.md`](system/user-management.md) | User profiles, workspace membership, teams, invitations, sessions, notification channels, "my stuff" views |
| Search | [`system/search.md`](system/search.md) | Full-text search across tenant, access-scoped, faceted, MCP-queryable, Cmd+K |
| ABAC Policy Engine | [`system/abac-policy-engine.md`](system/abac-policy-engine.md) | Attribute-based access control with declarative policies, scope cascade, dry-run evaluation, audit logging |
| Hierarchy Enforcement | [`system/hierarchy-enforcement.md`](system/hierarchy-enforcement.md) | Making the ownership hierarchy load-bearing: Tenant entity, non-optional workspace_id, ABAC middleware integration, tenant filtering, legacy cleanup |
| Unified Message Bus | [`system/message-bus.md`](system/message-bus.md) | One signed envelope for all inter-component communication: three-tier model (directed/event/telemetry), server-attested signing, workspace-scoped routing, ack-based delivery |
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
| **Authorization Provenance** | [`system/authorization-provenance.md`](system/authorization-provenance.md) | **Cryptographic work authorization:** signed inputs, CEL output constraints, derived input delegation chains, key binding, constraint propagation, offline verification. Amends agent-gates, supply-chain, identity-security, ABAC, agent-runtime, message-bus, HSI, observability |
| Agent Gates & Spec Binding | [`system/agent-gates.md`](system/agent-gates.md) | Agents as merge gates, composable gate chains, spec-to-code cryptographic binding, spec approval ledger |
| Spec Lifecycle | [`system/spec-lifecycle.md`](system/spec-lifecycle.md) | Auto-task on spec change, approval invalidation, drift detection, forge-native hooks |
| Merge Dependencies | [`system/merge-dependencies.md`](system/merge-dependencies.md) | MR dependency DAG, atomic groups, auto-detection from branch lineage, dependency-aware merge queue |
| Spec Registry | [`system/spec-registry.md`](system/spec-registry.md) | Git manifest + forge ledger for explicit spec registration, per-spec policies, approval tracking, auto-generated index |
| Spec Links | [`system/spec-links.md`](system/spec-links.md) | SHA-pinned links between specs (implements, supersedes, depends_on, conflicts_with, extends), mechanical gates, tenant-wide spec graph |
| Cross-Repo Dependency Graph | [`system/dependency-graph.md`](system/dependency-graph.md) | Auto-detected code/spec/API/schema dependencies, breaking change enforcement, cascade testing, version drift, impact analysis |
| Meta-Spec Reconciliation | [`system/meta-spec-reconciliation.md`](system/meta-spec-reconciliation.md) | Safe iteration on personas, principles, and standards: versioned meta-specs, workspace-scoped binding, provenance tracking, Kubernetes-style reconciliation loop, rollout policies, preview mode (fast iteration loop) |
| Realized System Model | [`system/realized-model.md`](system/realized-model.md) | Language-agnostic code knowledge graph: structural extraction on push, spec linkage, architectural timeline, concept views, risk metrics, narrative generation |
| System Explorer | [`system/system-explorer.md`](system/system-explorer.md) | GT-inspired live architecture viewer: moldable views per entity, inline spec editing, progressive preview (instant/fast/thorough), ghost overlays, flow traces, conversational exploration |
| UI Journeys & Navigation | [`system/ui-journeys.md`](system/ui-journeys.md) | (Superseded by Human-System Interface) Journey-oriented navigation |
| **Human-System Interface** | [`system/human-system-interface.md`](system/human-system-interface.md) | **How humans interact with autonomous development:** trust gradient, progressive Explorer (C4 + LLM views), agent interrogation, conversation-to-code provenance, scoped chat, cross-workspace spec links, completion summaries |
| UI Layout & Interactions | [`system/ui-layout.md`](system/ui-layout.md) | Spatial structure, view specification grammar (data/layout/encoding), interaction patterns (scope transitions, drill-down, inline chat), rendering technology (SVG + ELK/dagre) |
| Trusted Foundry (Future) | [`system/trusted-foundry-integration.md`](system/trusted-foundry-integration.md) | Reference pattern: IR-first development with audited components, deterministic compilation, reduced review surface. Not scheduled for implementation. |

## Development of This System

How Gyre gets built - process and standards for the agent team.

| Spec | Path | Summary |
|---|---|---|
| Architecture & Standards | [`development/architecture.md`](development/architecture.md) | Rust, Svelte, DDD, hexagonal, storage, API |
| API Design Conventions | [`development/api-conventions.md`](development/api-conventions.md) | URL structure, naming rules, parameter conventions, auth contract, deprecation protocol, mechanical enforcement |
| Database & Migrations | [`development/database-migrations.md`](development/database-migrations.md) | Diesel ORM, paired up/down migrations, multi-tenant row isolation, startup behavior |
| Ralph Loops (superseded) | [`development/ralph-loops.md`](development/ralph-loops.md) | Superseded by [`system/agent-runtime.md`](system/agent-runtime.md) §1 |
| Agent Experience (Day One) | [`development/agent-experience.md`](development/agent-experience.md) | Testing, observability, repo as system of record, cache hits, entropy management |
| Speed & Backpressure | [`development/speed-backpressure.md`](development/speed-backpressure.md) | The wheel, pre-commit hooks, quality gates |
| CI, Docs & Release | [`development/ci-docs-release.md`](development/ci-docs-release.md) | GitHub Actions, Starlight docs, semver, conventional commits |
| Manager Agent (superseded) | [`development/manager-agent.md`](development/manager-agent.md) | Superseded -- split into [`system/platform-model.md`](system/platform-model.md) + [`system/agent-runtime.md`](system/agent-runtime.md) §1 |
| Agent Development Workflow | [`development/agent-workflow.md`](development/agent-workflow.md) | Immediate feedback, worktrees, PRs, fix the environment |
| Dogfooding | [`development/dogfooding.md`](development/dogfooding.md) | Building Gyre with agent-boss |
| Development Philosophy | [`development/philosophy.md`](development/philosophy.md) | Speed, failure domains, humans steer / agents execute |
| Frontend Testing | [`development/frontend-testing.md`](development/frontend-testing.md) | Vitest + Testing Library component tests, jsdom setup, build gate |

## Prior Art & Lessons Learned

| Spec | Path | Summary |
|---|---|---|
| agent-boss Lessons | [`prior-art/agent-boss.md`](prior-art/agent-boss.md) | Carry forward / don't repeat from the prototype |
| Gas Town Lessons | [`prior-art/gas-town.md`](prior-art/gas-town.md) | GUPP, NDI, molecules, patrols, convoys |

## Personas

| Persona | Path | Summary |
|---|---|---|
| Workspace Orchestrator | [`personas/workspace-orchestrator.md`](personas/workspace-orchestrator.md) | Top-level orchestrator, delegates to repo orchestrators |
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
| M15: Diesel ORM | [`milestones/m15-diesel-migrations.md`](milestones/m15-diesel-migrations.md) | Diesel ORM + migrations, full SQLite persistence, multi-tenancy |
| M16: Security Hardening | [`milestones/m16-security-hardening.md`](milestones/m16-security-hardening.md) | Release automation endpoint, git argument injection prevention (M16-A), CISO finding resolutions |
| M17: Integration Testing | [`milestones/m17-integration-testing.md`](milestones/m17-integration-testing.md) | 68 API tests, 21 auth/RBAC tests, 12 git tests, 28 Playwright E2E tests |
| M18: Agent Identity | [`milestones/m18-agent-identity.md`](milestones/m18-agent-identity.md) | EdDSA JWT agent tokens, OIDC provider, token introspection, JWT revocation, stale-spec detection |
| M19: Container Runtime | [`milestones/m19-container-runtime.md`](milestones/m19-container-runtime.md) | Container-first agent runtime: Docker/Podman ContainerTarget with security defaults, procfs liveness monitor, workload attestation, SSH compute targets + reverse tunnels |
| M20: UI Accountability | [`milestones/m20-ui-coverage.md`](milestones/m20-ui-coverage.md) | 19 frontend findings resolved: admin panels, Repo Detail tabs, MR dependency panel, merge queue DAG, task detail view, token introspection, spec approvals |
| M21: Spec Registry | [`milestones/m21-spec-registry.md`](milestones/m21-spec-registry.md) | Spec manifest + ledger state machine (Approved/Pending/Deprecated), 8 `/api/v1/specs/` endpoints, Spec Dashboard UI |
| M22: Platform Entities | [`milestones/m22-platform-entities.md`](milestones/m22-platform-entities.md) | Workspace+Persona (M22.1), Budget (M22.2), SpecLinks (M22.3), DepGraph (M22.4), Platform UI - 7 views (M22.5), ABAC engine (M22.6), Search (M22.7), UserMgmt (M22.8) |
| M23: Platform Operations | [`milestones/m23-platform-operations.md`](milestones/m23-platform-operations.md) | Container lifecycle audit (M23.1), Analytics Decision API (M23.2), BCP graceful degradation (M23.3), SCIM 2.0 provisioning (M23.4) |
| M24: E2E Docker Agent | [`milestones/m24-e2e-docker.md`](milestones/m24-e2e-docker.md) | Full Docker agent Ralph loop: JWT pre-mint, container env injection, bridge networking, gyre-agent image, spawn modal compute target field (M24) |
| M25: Agent Runner | [`milestones/m25-agent-runner.md`](milestones/m25-agent-runner.md) | Bundled Claude Code runner (`agent-runner.mjs`), zero-config spawn via auto-registered `gyre-agent-default`, `GYRE_AGENT_CREDENTIALS` passthrough, Node 22 Alpine + pinned SDK |
| M26: WireGuard Mesh | [`milestones/m26-wireguard-mesh.md`](milestones/m26-wireguard-mesh.md) | Real WireGuard mesh networking: peer key exchange, DERP relay coordination, agent-to-agent encrypted tunnels _(Draft)_ |
| M27: Credential Opacity | [`milestones/m27-credential-opacity.md`](milestones/m27-credential-opacity.md) | Unix socket credential proxy (`cred-proxy` sidecar): credentials held in memory, never exposed to agent process env; supports Anthropic direct API, Vertex AI (GCP SA JSON), and git tokens _(Draft)_ |
| M28: UI Cohesion | [`milestones/m28-ui-cohesion.md`](milestones/m28-ui-cohesion.md) | Global workspace selector (topbar, localStorage persistence), entity deep-link URLs (`/repos/:id`, `/tasks/:id`, `/merge-requests/:id`, `/workspaces/:id`), cross-entity navigation via Svelte context, SpecDashboard repo filter |
| M29: Workspace Integration | [`milestones/m29-workspace-integration.md`](milestones/m29-workspace-integration.md) | Workspace-aware SPA: M29.4 wires `workspace_id` filter to agents/tasks/projects list calls; post-create reload preserves workspace context. **M29.5A** spec/gates/attestation stores migrated to async port traits + Diesel (migration 000009). **M29.5B** ABAC/compute/agent/workspace stores migrated to KvJsonStore + BudgetRepository (migration 000008). |
| M30: Knowledge Graph | [`milestones/m30-knowledge-graph.md`](milestones/m30-knowledge-graph.md) | Live knowledge graph extracted from source code: `GraphNode`/`GraphEdge` domain types, `RustExtractor` (syn-based AST), 13 graph API endpoints, push-triggered automatic extraction (M30b) |
| M31: UI Journeys | [`milestones/m31-ui-journeys.md`](milestones/m31-ui-journeys.md) | Intent-centric dashboard: Inbox (action queue), Briefing (narrative digest), System Explorer Canvas (MoldableView SVG graph), navigation restructure, keyboard shortcuts `i`/`b` |
| M32: Meta-Spec Reconciliation | [`milestones/m32-meta-spec-reconciliation.md`](milestones/m32-meta-spec-reconciliation.md) | Spec `kind` field (meta:persona/principle/standard/process), workspace meta-spec-set bindings, blast-radius endpoint, `meta_spec_set_sha` in spawn provenance, MetaSpecs UI |
| M33: Project Removal | [`milestones/m33-project-removal.md`](milestones/m33-project-removal.md) | Project entity removed; Workspace becomes the primary grouping entity. All APIs migrated to use `workspace_id` directly. |
| M34: Hierarchy Enforcement | [`milestones/m34-hierarchy-enforcement.md`](milestones/m34-hierarchy-enforcement.md) | Load-bearing ownership hierarchy: Tenant entity, non-optional workspace_id, ABAC middleware, tenant isolation, auth gap fixes, URL restructure |
| M35: Unified Message Bus | [`milestones/m35-message-bus.md`](milestones/m35-message-bus.md) | Signed message envelope replacing REST inbox + domain events + activity store. Three-tier model: directed (acked, persisted), telemetry (ring buffer), broadcast (fan-out). |
| HSI: Human-System Interface | [`milestones/hsi-human-system-interface.md`](milestones/hsi-human-system-interface.md) | Trust gradient, progressive Explorer (C4 + LLM views), agent interrogation, conversation provenance, MR timeline, gate traces, completion summaries, divergence detection, full Svelte UI rewrite (6-view model: Inbox/Briefing/Explorer/Specs/Meta-specs/Admin), 50 E2E tests. 29 PRs. |

## Open Questions

| Question | Status |
|---|---|
| SPIFFE integration details | **Resolved** - 3-layer stack: SPIFFE (workload attestation) + Gyre as OIDC provider (agent permissions) + Sigstore/Fulcio (keyless commit signing). Federated via standard protocols. |
| jj (Jujutsu) vs. Git | **Resolved** - jj adds value for agent workflows (atomic changes, auto-rebase, undo) |
| Agent collaboration model (hierarchy + ?) | **Resolved** - hierarchy (spawn tree) + typed peer messages (A2A) + cross-agent code awareness (M13.4) |
| Coordination primitives (blackboard vs. event stream vs. persistent work chains) | **Resolved** - event stream (domain events via WebSocket) + persistent work chains (task ref tree) |
| Persistent loop state (NDI pattern) | **Resolved** - Custom ref namespaces (M13.6): `refs/tasks/{task-id}` survives agent crash. Agent message inbox persists state between sessions. See [`system/agent-runtime.md`](system/agent-runtime.md) §1. |
| Decision library for learned interrupt resolutions | Open |
| Cost tracking model | **Resolved** - cost entries table, per-agent aggregation, dashboard cost view (M6.1) |
| CI as separate concept vs. emergent property | **Resolved** - emergent: pre-accept gates (M13.1) + merge queue gates (M12.1) = CI without a separate concept |
| Feature flags as CI alternative | Open |
| SSO/SCIM provider targets | **Resolved** - Keycloak primary, pluggable for others |
