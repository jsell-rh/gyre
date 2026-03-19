# Architecture & Engineering Standards

## Tech Stack

- **Rust** as the primary language (server, CLI, agent runtime).
- **Svelte 5 + shadcn-svelte** for the web UI.

## Architectural Invariants

- **Domain-Driven Design** and **Hexagonal Architecture** (ports & adapters) are **invariants**, not guidelines.
- These must be **enforced mechanically** - tests, git hooks, linters - not by convention alone.

## Storage

- **Abstracted** - storage is behind a port (hexagonal architecture). Implementation is swappable.
- **SQLite** as the default (simple, vertical scaling, zero ops).
- **PostgreSQL** supported as an alternative for multi-node / enterprise deployments.
- Do not leak storage implementation details into domain logic.

## API

- **REST** for admin panel, user-facing endpoints, and external integrations.
- **gRPC** for internal service-to-service communication where performance matters.
- **WebSocket** as the primary transport for agent ↔ server communication.

### API Versioning

- All REST API endpoints **must be versioned** with a URL prefix: `/api/v1/`.
- The version prefix is part of the route, not a header or query parameter.
- When a breaking change is required, introduce a new version (`/api/v2/`) and deprecate the old one with a sunset timeline.
- Non-versioned convenience endpoints (`/health`, `/ws`) are allowed for infrastructure concerns that are not part of the domain API contract.
- WebSocket message types are versioned implicitly via the `type` tag in the JSON protocol. New message types are additive; removed types go through a deprecation cycle.

## Philosophy

- This orchestrator exists to **maximize throughput and quality of code**.
- It is **opinionated by design**. Where an opinion improves throughput or quality, enforce it. Don't be neutral when a strong default is clearly better.
