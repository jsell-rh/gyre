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
| **Platform Model** | [`system/platform-model.md`](system/platform-model.md) | **Foundational spec:** tenant/workspace/repo hierarchy, persona model, orchestration, MCP coordination, budgets, rollback, secrets, bootstrap |
| User Management & Notifications | [`system/user-management.md`](system/user-management.md) | User profiles, workspace membership, teams, invitations, sessions, notification channels, "my stuff" views |
| Search | [`system/search.md`](system/search.md) | Full-text search across tenant, access-scoped, faceted, MCP-queryable, Cmd+K |
| ABAC Policy Engine | [`system/abac-policy-engine.md`](system/abac-policy-engine.md) | Attribute-based access control with declarative policies, scope cascade, dry-run evaluation, audit logging |
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
| Agent Gates & Spec Binding | [`system/agent-gates.md`](system/agent-gates.md) | Agents as merge gates, composable gate chains, spec-to-code cryptographic binding, spec approval ledger |
| Spec Lifecycle | [`system/spec-lifecycle.md`](system/spec-lifecycle.md) | Auto-task on spec change, approval invalidation, drift detection, forge-native hooks |
| Merge Dependencies | [`system/merge-dependencies.md`](system/merge-dependencies.md) | MR dependency DAG, atomic groups, auto-detection from branch lineage, dependency-aware merge queue |
| Spec Registry | [`system/spec-registry.md`](system/spec-registry.md) | Git manifest + forge ledger for explicit spec registration, per-spec policies, approval tracking, auto-generated index |
| Spec Links | [`system/spec-links.md`](system/spec-links.md) | SHA-pinned links between specs (implements, supersedes, depends_on, conflicts_with, extends), mechanical gates, tenant-wide spec graph |
| Cross-Repo Dependency Graph | [`system/dependency-graph.md`](system/dependency-graph.md) | Auto-detected code/spec/API/schema dependencies, breaking change enforcement, cascade testing, version drift, impact analysis |
| Trusted Foundry (Future) | [`system/trusted-foundry-integration.md`](system/trusted-foundry-integration.md) | Reference pattern: IR-first development with audited components, deterministic compilation, reduced review surface. Not scheduled for implementation. |

## Development of This System

How Gyre gets built - process and standards for the agent team.

| Spec | Path | Summary |
|---|---|---|
| Architecture & Standards | [`development/architecture.md`](development/architecture.md) | Rust, Svelte, DDD, hexagonal, storage, API |
| Database & Migrations | [`development/database-migrations.md`](development/database-migrations.md) | Diesel ORM, paired up/down migrations, multi-tenant row isolation, startup behavior |
| Ralph Loops | [`development/ralph-loops.md`](development/ralph-loops.md) | Core loop definition, meta loops, coordination |
| Agent Experience (Day One) | [`development/agent-experience.md`](development/agent-experience.md) | Testing, observability, repo as system of record, cache hits, entropy management |
| Speed & Backpressure | [`development/speed-backpressure.md`](development/speed-backpressure.md) | The wheel, pre-commit hooks, quality gates |
| CI, Docs & Release | [`development/ci-docs-release.md`](development/ci-docs-release.md) | GitHub Actions, Starlight docs, semver, conventional commits |
| Manager Agent Orchestration | [`development/manager-agent.md`](development/manager-agent.md) | Manager Ralph loop, sub-agent dispatch, lifecycle rules |
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
