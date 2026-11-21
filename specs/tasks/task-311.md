---
title: "Publish reviewed spec backlog for specs/development/architecture.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/architecture.md",
  "plan_id": "68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "4156114b8040e24475627c78ec9050267109907a86602d02d50cb3a67c388e9f",
    "sections": [
      {
        "id": "7bfdba805ca368319f5c9740",
        "decision": "context",
        "requirement_quote": "Architecture & Engineering Standards",
        "detail": "Document title only; the section carries no body text and no implementable requirement. It introduces the standards defined in the child sections (Tech Stack, Architectural Invariants, Storage, API, Philosophy), each of which is accounted for individually below.",
        "evidence": []
      },
      {
        "id": "2a696c7f5c2ffc02e88742ae",
        "decision": "gap",
        "requirement_quote": "- **Rust** as the primary language (server, CLI, agent runtime).\n- **Svelte 5 + shadcn-svelte** for the web UI.",
        "detail": "The section has two failures, one a plain implementation gap and one a canonical conflict. (1) Svelte 5 is satisfied (web/package.json pins svelte ^5.0.0), but shadcn-svelte is NOT used anywhere: web/package.json contains no shadcn-svelte, no Tailwind, and no bits-ui (shadcn-svelte's required foundations); the UI is a hand-written component library (web/src/lib/*.svelte: Button, Card, Badge, Tabs, Table, Modal, ...) styled by a bespoke 'Gyre Design System - Red Hat Brand Standards' stylesheet. The specced UI stack is unimplemented - concrete missing production behavior. (2) Rust as primary language is satisfied for the server and CLI (both are crates in the Cargo workspace), but the '(agent runtime)' parenthetical is an unresolved conflict between canonical specs, not a reading this audit may resolve: specs/system/agent-runtime.md mandates a Node.js in-container agent image (agent-runner.mjs entrypoint on the Claude Agent SDK, Node.js pinned by the Nix flake: 'All backends run the same agent image'), and the shipped runtime docker/gyre-agent/agent-runner.mjs is JavaScript. architecture.md's parenthetical and agent-runtime.md's mandated image are incompatible canonical requirements; per the review rules this is a spec-lifecycle escalation (amend architecture.md's parenthetical, or mandate a Rust in-container runner), and existing code rationale cannot pre-resolve it. The section stays open: the shadcn-svelte half lacks production behavior, and the Rust-agent-runtime half requires a controller ruling before any code could close it.",
        "evidence": [
          {
            "file": "web/package.json",
            "quote": "\"svelte\": \"^5.0.0\",",
            "detail": "The web UI runs Svelte 5 as specced, but this manifest is the UI's complete dependency list: it contains no shadcn-svelte, no tailwindcss, and no bits-ui (the foundations shadcn-svelte is generated from), so the shadcn-svelte half of the specced stack is not installed anywhere in the project."
          },
          {
            "file": "web/src/lib/Button.svelte",
            "quote": "class=\"btn btn-{variant} btn-{size}\"",
            "detail": "UI components are hand-written Svelte components styled with bespoke CSS classes. shadcn-svelte components are bits-ui-based components composed with Tailwind utility classes; none exist under web/. The button and the rest of web/src/lib/*.svelte (Card, Badge, Tabs, Table, Modal, Toast, ...) form a custom component library instead."
          },
          {
            "file": "web/src/lib/design-system.css",
            "quote": "/* Gyre Design System — Red Hat Brand Standards */",
            "detail": "The UI's styling foundation is a bespoke design-system stylesheet (Red Hat brand palette as CSS custom properties), not shadcn-svelte's Tailwind-based theming; the shipped UI substitutes its own design system for the specced stack."
          },
          {
            "file": "Cargo.toml",
            "quote": "members = [\n    \"crates/gyre-server\",\n    \"crates/gyre-cli\",",
            "detail": "Rust is the primary language for the server and the CLI: both are crates in the Cargo workspace (with gyre-domain, gyre-ports, gyre-adapters, gyre-common, gyre-spiffe). The server/CLI half of the Rust requirement is satisfied by production Rust code."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "The Nix flake pins all dependencies (Node.js version, Claude Agent SDK version, git, curl) for bit-reproducible builds.",
            "detail": "The canonical agent-runtime spec mandates a Node.js in-container agent image (Node.js and the Claude Agent SDK pinned by the Nix flake), which is incompatible with architecture.md's parenthetical listing the 'agent runtime' among Rust surfaces. This is a conflict between two canonical requirements and is reserved for spec-lifecycle escalation; it cannot be closed by code under either reading without amending one spec."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "agent-runner.mjs       # Agent entrypoint (Claude Agent SDK)",
            "detail": "The canonical agent image's entrypoint is a JavaScript file; the in-container agent runtime is Node.js by canonical contract, not Rust."
          },
          {
            "file": "docker/gyre-agent/agent-runner.mjs",
            "quote": "const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {",
            "detail": "The shipped agent runtime is a 778-line JavaScript entrypoint performing all agent-server communication via HTTP fetch - the production agent runtime on the ground is Node.js, confirming the canonical conflict with architecture.md's '(server, CLI, agent runtime)' Rust parenthetical."
          }
        ]
      },
      {
        "id": "8ecfda7e74dfb6561b2ea5d3",
        "decision": "satisfied",
        "requirement_quote": "- **Domain-Driven Design** and **Hexagonal Architecture** (ports & adapters) are **invariants**, not guidelines.\n- These must be **enforced mechanically** - tests, git hooks, linters - not by convention alone.",
        "detail": "Both requirements hold in production. Hexagonal architecture is realized as the workspace layering: gyre-common (shared types), gyre-ports (port traits, no infrastructure deps), gyre-domain (pure business logic depending only on ports + common), gyre-adapters (SQLite/Diesel/PostgreSQL/mem implementations of the ports), gyre-server (wiring), gyre-cli. The invariant is enforced mechanically on three layers, not by convention: (1) scripts/check-arch.sh scans gyre-domain's Cargo.toml and sources for forbidden dependencies (gyre-adapters, rusqlite, sqlx, reqwest, hyper, axum, tonic) and for subprocess I/O patterns, failing with remediation instructions; (2) the same script runs as the arch-lint pre-commit hook in .pre-commit-config.yaml; (3) it runs as the dedicated 'Architecture lint' CI job in .github/workflows/ci.yml. gyre-domain's Cargo.toml declares and honors the invariant (only gyre-common plus pure parsing/evaluation libraries - serde, syn, tree-sitter, cel-interpreter; no storage or transport crate).",
        "evidence": [
          {
            "file": "crates/gyre-domain/Cargo.toml",
            "quote": "# INVARIANT: gyre-domain MUST NOT depend on gyre-adapters or any infrastructure crate.",
            "detail": "The domain crate declares and honors the hexagonal invariant: its only workspace dependency is gyre-common (plus pure parsing/eval libraries); no diesel/rusqlite/sqlx, no HTTP or transport crate appears in its dependency list."
          },
          {
            "file": "scripts/check-arch.sh",
            "quote": "# Invariant: gyre-domain MUST NOT import gyre-adapters or any infrastructure crate.",
            "detail": "Mechanical enforcement exists as a linter: the script greps gyre-domain's Cargo.toml and source for forbidden dependencies (gyre-adapters, rusqlite, sqlx, reqwest, hyper, axum, tonic), forbidden module imports, and subprocess I/O patterns (std::process::Command, Command::new), exiting non-zero with remediation instructions on any hit."
          },
          {
            "file": "docs/development.md",
            "quote": "Hook summary (see `.pre-commit-config.yaml` for the full list; ~60 local hooks):\n- `cargo-fmt` / `cargo-clippy`: formatting and lint with denied warnings\n- `arch-lint`: hexagonal boundary enforcement",
            "detail": "The lint is wired as a git hook: the development documentation's pre-commit hook summary lists `arch-lint` (entry scripts/check-arch.sh in .pre-commit-config.yaml) among the ~60 local hooks run on every commit, satisfying the 'git hooks' enforcement limb of the requirement. (The hook config itself lives in .pre-commit-config.yaml: id arch-lint, entry scripts/check-arch.sh.)"
          },
          {
            "file": ".github/workflows/ci.yml",
            "quote": "      - run: bash scripts/check-arch.sh",
            "detail": "The lint also runs in CI as the dedicated 'Architecture lint' job (actions/checkout followed by this run step), satisfying the 'tests/linters' limb; the invariant is enforced mechanically on commit and in CI, not by convention alone."
          },
          {
            "file": "Cargo.toml",
            "quote": "members = [\n    \"crates/gyre-server\",\n    \"crates/gyre-cli\",",
            "detail": "The workspace member list realizes the hexagonal layering the invariant names: gyre-common, gyre-ports, gyre-domain, gyre-adapters, gyre-server, gyre-cli (plus gyre-spiffe) - ports & adapters as separate crates with the domain between them."
          }
        ]
      },
      {
        "id": "df78f83918d8549c48c15ea5",
        "decision": "gap",
        "requirement_quote": "- **Abstracted** - storage is behind a port (hexagonal architecture). Implementation is swappable.\n- **SQLite** as the default (simple, vertical scaling, zero ops).\n- **PostgreSQL** supported as an alternative for multi-node / enterprise deployments.\n- Do not leak storage implementation details into domain logic.",
        "detail": "Three of the four bullets are satisfied; the 'SQLite as the default' bullet is not, so the section is a gap. (1) Abstraction/swappability: satisfied - every repository trait lives in gyre-ports (e.g. gyre_ports::storage::StoragePort) and AppState wires each port via the store! macro, which selects PgStorage, SqliteStorage, or a mem implementation for the same trait. (2) SQLite as the default: GAP - storage selection is purely environmental: SQLite is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL, and with the variable unset (the shipped and documented default) no durable storage is opened at all; every port falls back to an in-memory implementation, so a server started with 'cargo run -p gyre-server' and no env persists nothing across restarts. docs/server-config.md documents the inverse of the spec: 'Unset = in-memory (default, stateless)'. A fix must also handle process-level harnesses: web/playwright.config.js boots the real server binary with only GYRE_PORT/GYRE_AUTH_TOKEN, i.e. on today's in-memory default - if SQLite becomes the default without an explicit in-memory opt-in for such harnesses, e2e runs silently become durable outside the working tree (controller review feedback on the prior plan's sqlite-default-storage task). (3) PostgreSQL: satisfied - a postgres:// URL constructs PgStorage, preferred by the store! wiring; crates/gyre-adapters/src/postgres/ implements the port traits across 50+ repository modules. (4) No storage leak into domain: satisfied - gyre-domain has no storage dependencies and scripts/check-arch.sh mechanically forbids rusqlite/sqlx/gyre-adapters in the domain crate.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let db_url = std::env::var(\"GYRE_DATABASE_URL\").ok();",
            "detail": "Storage backend selection is purely environmental: SQLite (SqliteStorage) is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL. With the variable unset - the shipped default - db_url is None and no durable storage is opened at all."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    macro_rules! store {\n        ($trait:ty, $mem:expr) => {\n            if let Some(ref d) = pg_db {\n                Arc::clone(d) as Arc<$trait>\n            } else if let Some(ref d) = sqlite_db {\n                Arc::clone(d) as Arc<$trait>\n            } else {\n                Arc::new($mem) as Arc<$trait>\n            }\n        };\n    }",
            "detail": "Every repository port in AppState falls back to an in-memory implementation when neither PgStorage nor SqliteStorage is configured: the default deployment is stateless in-memory, not SQLite. This contradicts 'SQLite as the default (simple, vertical scaling, zero ops)'. The abstraction/swappability bullets are satisfied (same port trait, three interchangeable implementations), but the default-storage bullet is unimplemented."
          },
          {
            "file": "docs/server-config.md",
            "quote": "| `GYRE_DATABASE_URL` | _(unset -- in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |",
            "detail": "The operator documentation confirms the default is the inverse of the spec: in-memory is the default and SQLite is an opt-in via an explicit sqlite:// URL, so 'SQLite as the default' is not the shipped behavior."
          },
          {
            "file": "web/playwright.config.js",
            "quote": "      GYRE_PORT: '2222',\n      GYRE_AUTH_TOKEN: 'e2e-test-token',",
            "detail": "The process-level e2e harness boots the real server binary ('cargo run -p gyre-server' locally, the release binary in CI) with only GYRE_PORT and GYRE_AUTH_TOKEN - i.e. on today's in-memory default, with no GYRE_DATABASE_URL. Any change that makes SQLite the default must give such harnesses an explicit in-memory opt-in (controller review feedback on the prior plan's sqlite-default-storage task), or e2e runs silently become durable at the default path outside the working tree."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let pg_db: Option<Arc<gyre_adapters::PgStorage>> = db_url",
            "detail": "PostgreSQL support is real production wiring: a postgres:// URL constructs PgStorage, which the store! macro prefers over SQLite for every port - satisfying 'PostgreSQL supported as an alternative'."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/mod.rs",
            "quote": "use gyre_ports::storage::StoragePort;",
            "detail": "PgStorage is an adapter implementing the port traits defined in gyre-ports (the postgres/ directory holds 50+ repository modules with 95 'impl ... for PgStorage' blocks), confirming implementation swappability behind the same port abstraction."
          },
          {
            "file": "crates/gyre-domain/Cargo.toml",
            "quote": "# INVARIANT: gyre-domain MUST NOT depend on gyre-adapters or any infrastructure crate.",
            "detail": "No storage implementation details leak into domain logic: gyre-domain's dependency list contains no storage crate (no diesel, rusqlite, or sqlx), and scripts/check-arch.sh mechanically forbids those dependencies in the domain - the fourth bullet is satisfied."
          }
        ]
      },
      {
        "id": "5e7a44c5f15c34d70101b959",
        "decision": "gap",
        "requirement_quote": "- **REST** for admin panel, user-facing endpoints, and external integrations.\n- **gRPC** for internal service-to-service communication where performance matters.\n- **WebSocket** as the primary transport for agent ↔ server communication.",
        "detail": "The REST bullet is satisfied; the WebSocket bullet is a gap, so the section is a gap. (1) REST: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in api/mod.rs plus the top-level router in lib.rs), covering the admin panel (api/admin.rs), user-facing endpoints, and external integrations (webhooks, SCIM, MCP-over-HTTP). (2) WebSocket as the primary transport for agent-servers communication: GAP - the canonical agent runtime (docker/gyre-agent/agent-runner.mjs, the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket (zero matches for WebSocket/ws:// in the file): every agent-to-server interaction is HTTP - heartbeat (POST /api/v1/agents/{id}/heartbeat), log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), completion (POST /api/v1/agents/{id}/complete) - and server-to-agent message delivery is MCP/REST polling (gyre_message_poll, GET /api/v1/agents/{id}/messages) per docs/agent-protocol.md. The /ws endpoint is consumed by the browser dashboard SPA (web/src/lib/ws.js) and the CLI TUI (crates/gyre-cli/src/ws.rs) - human clients receiving event push. WebSocket is a UI push channel in the realized system, not the primary agent transport; making it the primary transport would require a WebSocket client in the agent runtime and server push of agent-directed messages over it. (3) gRPC: the realized platform is a single server binary with no internal service-to-service communication, so the 'where performance matters' condition has no trigger; the gRPC surfaces that exist are the OTLP trace receiver (gate-time telemetry ingest from the application under test - explicitly scoped, not general internal service-to-service) and the SPIFFE Workload API client to an external SPIRE server (crates/gyre-spiffe/src/workload_api.rs), neither of which is internal service-to-service communication. This is recorded for the controller to rule whether the monolithic realization already satisfies the conditional (no such communication exists to carry) or whether the spec's services decomposition was never built; it is not counted as an independent missing behavior because there is no realized internal service-to-service path that performs matters.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/merge-requests\",",
            "detail": "The REST bullet is satisfied: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in this file alone), covering admin (api/admin.rs), user-facing, and external-integration surfaces."
          },
          {
            "file": "docker/gyre-agent/agent-runner.mjs",
            "quote": "const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {",
            "detail": "The canonical agent runtime (the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket - grep for WebSocket/ws:// in the 778-line file returns zero matches. All agent-to-server communication is HTTP fetch: heartbeat, log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), and completion (POST /api/v1/agents/{id}/complete). The specced primary transport for agent-server communication is absent from the agent side."
          },
          {
            "file": "web/src/lib/ws.js",
            "quote": "ws = new WebSocket(`${protocol}//${window.location.host}/ws`);",
            "detail": "The /ws WebSocket endpoint is consumed by the browser dashboard SPA (and by the CLI TUI via crates/gyre-cli/src/ws.rs, DEFAULT_SERVER ws://localhost:3000/ws) - human clients receiving domain-event push. WebSocket serves the UI, not the agent transport, so it is not 'the primary transport for agent-server communication' in either direction."
          },
          {
            "file": "docs/agent-protocol.md",
            "quote": "can poll the message via the MCP surface (`gyre_message_poll`, which derives the inbox from the verified token subject — inherently own-inbox)",
            "detail": "Server-to-agent message delivery is also not WebSocket: the documented delivery path for agent inbound messages is MCP/REST polling (gyre_message_poll; the REST inbox route GET /api/v1/agents/{id}/messages exists for the same purpose), confirming the transport gap runs in both directions."
          },
          {
            "file": "crates/gyre-server/src/otlp_receiver.rs",
            "quote": "//! Lightweight OTLP gRPC receiver for gate-time trace capture (HSI §3a).",
            "detail": "On the gRPC bullet: the only in-process gRPC server is this OTLP trace receiver, scoped to gate-time trace capture from the application under test (the module doc explicitly disclaims being a general-purpose observability backend); the other tonic dependency is the SPIFFE Workload API client to an external SPIRE server. Neither is internal service-to-service communication - the realized platform is a single server binary with no internal service-to-service path, so the 'where performance matters' condition has no trigger."
          }
        ]
      },
      {
        "id": "e88a3722c28e67201fd62208",
        "decision": "satisfied",
        "requirement_quote": "- All REST API endpoints **must be versioned** with a URL prefix: `/api/v1/`.\n- The version prefix is part of the route, not a header or query parameter.\n- When a breaking change is required, introduce a new version (`/api/v2/`) and deprecate the old one with a sunset timeline.\n- Non-versioned convenience endpoints (`/health`, `/ws`) are allowed for infrastructure concerns that are not part of the domain API contract.\n- WebSocket message types are versioned implicitly via the `type` tag in the JSON protocol. New message types are additive; removed types go through a deprecation cycle.",
        "detail": "All five bullets hold in production. (1-2) Every domain REST route is registered with the literal /api/v1/ URL prefix as part of the route string (e.g. 331 .route registrations in api/mod.rs), never via header or query parameter. The non-versioned production routes are exactly the allowed/specced classes: /health, /healthz, /readyz, /metrics (infrastructure concerns), /ws and /ws/agents/:id/tty (WebSocket, not REST; /ws is the spec's own example of an allowed convenience endpoint), /.well-known/openid-configuration, /.well-known/jwks.json and the SPIFFE trust bundle (standard discovery paths), /git/:workspace_slug/:repo_name/* (git smart HTTP wire protocol, not a REST API), /mcp and /mcp/sse (JSON-RPC, not REST), and the SPA static handlers ('/' and '/*path'). Routes like /protected, /roles, /tenant found in auth.rs live inside #[cfg(test)] test modules and are not production. (3) No /api/v2 exists and no breaking change has been shipped, so the introduce-v2-and-deprecate machinery has no trigger; nothing circumvents the prefix that would require it. (4) The SCIM REST endpoints live at /scim/v2 rather than /api/v1 - URL-versioned (v2), and that base path is mandated by the canonical specs/system/identity-security.md ('Base path: /scim/v2 (separate from /api/v1 - SCIM clients expect this path)'), so this is a canonically-sanctioned, still-URL-versioned exception rather than an unversioned domain endpoint; recorded so the controller can confirm the cross-spec reading. (5) WebSocket message types are discriminated by the JSON 'type' tag (serde tagged enum on WsMessage), and an unrecognized type tag deserializes to the Unknown variant without error - new message types are additive exactly as specced; no type has been removed, so the removal deprecation cycle has no trigger.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/merge-requests\",",
            "detail": "Representative of the registration pattern for every domain REST route: the /api/v1/ version prefix is a literal part of the route path string (331 .route registrations in this file), not a header or query parameter."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": ".route(\"/health\", get(health::health_handler))",
            "detail": "The specced non-versioned convenience endpoints exist exactly as allowed: /health (with siblings /healthz, /readyz, /metrics) serves infrastructure concerns outside the versioned domain API contract."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": ".route(\"/ws\", get(ws::ws_handler))",
            "detail": "The /ws convenience endpoint is non-versioned exactly as the spec explicitly permits for infrastructure concerns; the remaining non-versioned production routes are the git smart HTTP protocol (/git/:workspace_slug/:repo_name/*), the MCP JSON-RPC endpoints (/mcp, /mcp/sse), standard /.well-known discovery paths, and SPA static handlers - none of which are REST domain endpoints."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/scim/v2/Users\",",
            "detail": "The only REST routes outside /api/v1/ are the SCIM provisioning endpoints at /scim/v2 - URL-versioned (v2) per the SCIM standard and mandated at that base path by the canonical identity-security.md, a canonically-sanctioned exception rather than an unversioned domain endpoint."
          },
          {
            "file": "specs/system/identity-security.md",
            "quote": "Base path: `/scim/v2` (separate from `/api/v1` — SCIM clients expect this path)",
            "detail": "Canonical cross-reference that sanctions the SCIM base path outside /api/v1/: SCIM clients expect the standard /scim/v2 path, so the versioning rule's /api/v1/ prefix is canonically narrowed for this protocol while remaining URL-versioned."
          },
          {
            "file": "crates/gyre-common/src/protocol.rs",
            "quote": "#[serde(tag = \"type\")]",
            "detail": "WebSocket message types are versioned implicitly via the 'type' tag in the JSON protocol exactly as specced: WsMessage is a serde internally-tagged enum discriminated by the type field."
          },
          {
            "file": "crates/gyre-common/src/protocol.rs",
            "quote": "// An unrecognized type tag should deserialize to Unknown without error.",
            "detail": "Additivity is real production behavior: an unrecognized (future) message type tag deserializes to the Unknown variant instead of erroring, so new message types are additive as the spec requires; no message type has been removed, so the removal deprecation cycle has no trigger."
          }
        ]
      },
      {
        "id": "b15de676849c8ac7ce992c55",
        "decision": "context",
        "requirement_quote": "- This orchestrator exists to **maximize throughput and quality of code**.\n- It is **opinionated by design**. Where an opinion improves throughput or quality, enforce it. Don't be neutral when a strong default is clearly better.",
        "detail": "Rationale-only vision statement: it declares the orchestrator's purpose and design posture, not an implementable requirement against production code. There is no behavior to verify or enforce here - the 'enforce strong opinions' posture is a disposition realized procedurally elsewhere (the guard corpus of ~60 pre-commit hooks and CI lints, the mechanical invariants in AGENTS.md), not a contract of this section. No gap can attach to a section with no normative requirement.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "2a696c7f5c2ffc02e88742ae",
        "decision": "gap",
        "requirement_quote": "- **Rust** as the primary language (server, CLI, agent runtime).\n- **Svelte 5 + shadcn-svelte** for the web UI.",
        "detail": "The section has two failures, one a plain implementation gap and one a canonical conflict. (1) Svelte 5 is satisfied (web/package.json pins svelte ^5.0.0), but shadcn-svelte is NOT used anywhere: web/package.json contains no shadcn-svelte, no Tailwind, and no bits-ui (shadcn-svelte's required foundations); the UI is a hand-written component library (web/src/lib/*.svelte: Button, Card, Badge, Tabs, Table, Modal, ...) styled by a bespoke 'Gyre Design System - Red Hat Brand Standards' stylesheet. The specced UI stack is unimplemented - concrete missing production behavior. (2) Rust as primary language is satisfied for the server and CLI (both are crates in the Cargo workspace), but the '(agent runtime)' parenthetical is an unresolved conflict between canonical specs, not a reading this audit may resolve: specs/system/agent-runtime.md mandates a Node.js in-container agent image (agent-runner.mjs entrypoint on the Claude Agent SDK, Node.js pinned by the Nix flake: 'All backends run the same agent image'), and the shipped runtime docker/gyre-agent/agent-runner.mjs is JavaScript. architecture.md's parenthetical and agent-runtime.md's mandated image are incompatible canonical requirements; per the review rules this is a spec-lifecycle escalation (amend architecture.md's parenthetical, or mandate a Rust in-container runner), and existing code rationale cannot pre-resolve it. The section stays open: the shadcn-svelte half lacks production behavior, and the Rust-agent-runtime half requires a controller ruling before any code could close it.",
        "evidence": [
          {
            "file": "web/package.json",
            "quote": "\"svelte\": \"^5.0.0\",",
            "detail": "The web UI runs Svelte 5 as specced, but this manifest is the UI's complete dependency list: it contains no shadcn-svelte, no tailwindcss, and no bits-ui (the foundations shadcn-svelte is generated from), so the shadcn-svelte half of the specced stack is not installed anywhere in the project."
          },
          {
            "file": "web/src/lib/Button.svelte",
            "quote": "class=\"btn btn-{variant} btn-{size}\"",
            "detail": "UI components are hand-written Svelte components styled with bespoke CSS classes. shadcn-svelte components are bits-ui-based components composed with Tailwind utility classes; none exist under web/. The button and the rest of web/src/lib/*.svelte (Card, Badge, Tabs, Table, Modal, Toast, ...) form a custom component library instead."
          },
          {
            "file": "web/src/lib/design-system.css",
            "quote": "/* Gyre Design System — Red Hat Brand Standards */",
            "detail": "The UI's styling foundation is a bespoke design-system stylesheet (Red Hat brand palette as CSS custom properties), not shadcn-svelte's Tailwind-based theming; the shipped UI substitutes its own design system for the specced stack."
          },
          {
            "file": "Cargo.toml",
            "quote": "members = [\n    \"crates/gyre-server\",\n    \"crates/gyre-cli\",",
            "detail": "Rust is the primary language for the server and the CLI: both are crates in the Cargo workspace (with gyre-domain, gyre-ports, gyre-adapters, gyre-common, gyre-spiffe). The server/CLI half of the Rust requirement is satisfied by production Rust code."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "The Nix flake pins all dependencies (Node.js version, Claude Agent SDK version, git, curl) for bit-reproducible builds.",
            "detail": "The canonical agent-runtime spec mandates a Node.js in-container agent image (Node.js and the Claude Agent SDK pinned by the Nix flake), which is incompatible with architecture.md's parenthetical listing the 'agent runtime' among Rust surfaces. This is a conflict between two canonical requirements and is reserved for spec-lifecycle escalation; it cannot be closed by code under either reading without amending one spec."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "agent-runner.mjs       # Agent entrypoint (Claude Agent SDK)",
            "detail": "The canonical agent image's entrypoint is a JavaScript file; the in-container agent runtime is Node.js by canonical contract, not Rust."
          },
          {
            "file": "docker/gyre-agent/agent-runner.mjs",
            "quote": "const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {",
            "detail": "The shipped agent runtime is a 778-line JavaScript entrypoint performing all agent-server communication via HTTP fetch - the production agent runtime on the ground is Node.js, confirming the canonical conflict with architecture.md's '(server, CLI, agent runtime)' Rust parenthetical."
          }
        ]
      },
      {
        "id": "df78f83918d8549c48c15ea5",
        "decision": "gap",
        "requirement_quote": "- **Abstracted** - storage is behind a port (hexagonal architecture). Implementation is swappable.\n- **SQLite** as the default (simple, vertical scaling, zero ops).\n- **PostgreSQL** supported as an alternative for multi-node / enterprise deployments.\n- Do not leak storage implementation details into domain logic.",
        "detail": "Three of the four bullets are satisfied; the 'SQLite as the default' bullet is not, so the section is a gap. (1) Abstraction/swappability: satisfied - every repository trait lives in gyre-ports (e.g. gyre_ports::storage::StoragePort) and AppState wires each port via the store! macro, which selects PgStorage, SqliteStorage, or a mem implementation for the same trait. (2) SQLite as the default: GAP - storage selection is purely environmental: SQLite is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL, and with the variable unset (the shipped and documented default) no durable storage is opened at all; every port falls back to an in-memory implementation, so a server started with 'cargo run -p gyre-server' and no env persists nothing across restarts. docs/server-config.md documents the inverse of the spec: 'Unset = in-memory (default, stateless)'. A fix must also handle process-level harnesses: web/playwright.config.js boots the real server binary with only GYRE_PORT/GYRE_AUTH_TOKEN, i.e. on today's in-memory default - if SQLite becomes the default without an explicit in-memory opt-in for such harnesses, e2e runs silently become durable outside the working tree (controller review feedback on the prior plan's sqlite-default-storage task). (3) PostgreSQL: satisfied - a postgres:// URL constructs PgStorage, preferred by the store! wiring; crates/gyre-adapters/src/postgres/ implements the port traits across 50+ repository modules. (4) No storage leak into domain: satisfied - gyre-domain has no storage dependencies and scripts/check-arch.sh mechanically forbids rusqlite/sqlx/gyre-adapters in the domain crate.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let db_url = std::env::var(\"GYRE_DATABASE_URL\").ok();",
            "detail": "Storage backend selection is purely environmental: SQLite (SqliteStorage) is constructed only when GYRE_DATABASE_URL is set to a non-postgres URL. With the variable unset - the shipped default - db_url is None and no durable storage is opened at all."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    macro_rules! store {\n        ($trait:ty, $mem:expr) => {\n            if let Some(ref d) = pg_db {\n                Arc::clone(d) as Arc<$trait>\n            } else if let Some(ref d) = sqlite_db {\n                Arc::clone(d) as Arc<$trait>\n            } else {\n                Arc::new($mem) as Arc<$trait>\n            }\n        };\n    }",
            "detail": "Every repository port in AppState falls back to an in-memory implementation when neither PgStorage nor SqliteStorage is configured: the default deployment is stateless in-memory, not SQLite. This contradicts 'SQLite as the default (simple, vertical scaling, zero ops)'. The abstraction/swappability bullets are satisfied (same port trait, three interchangeable implementations), but the default-storage bullet is unimplemented."
          },
          {
            "file": "docs/server-config.md",
            "quote": "| `GYRE_DATABASE_URL` | _(unset -- in-memory)_ | Database URL. `sqlite://gyre.db` for SQLite or `postgres://user:pass@host/db` for PostgreSQL. When set, all port traits persist via Diesel ORM with auto-migrations. Unset = in-memory (default, stateless). (M10.1, M15.1, M15.2) |",
            "detail": "The operator documentation confirms the default is the inverse of the spec: in-memory is the default and SQLite is an opt-in via an explicit sqlite:// URL, so 'SQLite as the default' is not the shipped behavior."
          },
          {
            "file": "web/playwright.config.js",
            "quote": "      GYRE_PORT: '2222',\n      GYRE_AUTH_TOKEN: 'e2e-test-token',",
            "detail": "The process-level e2e harness boots the real server binary ('cargo run -p gyre-server' locally, the release binary in CI) with only GYRE_PORT and GYRE_AUTH_TOKEN - i.e. on today's in-memory default, with no GYRE_DATABASE_URL. Any change that makes SQLite the default must give such harnesses an explicit in-memory opt-in (controller review feedback on the prior plan's sqlite-default-storage task), or e2e runs silently become durable at the default path outside the working tree."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let pg_db: Option<Arc<gyre_adapters::PgStorage>> = db_url",
            "detail": "PostgreSQL support is real production wiring: a postgres:// URL constructs PgStorage, which the store! macro prefers over SQLite for every port - satisfying 'PostgreSQL supported as an alternative'."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/mod.rs",
            "quote": "use gyre_ports::storage::StoragePort;",
            "detail": "PgStorage is an adapter implementing the port traits defined in gyre-ports (the postgres/ directory holds 50+ repository modules with 95 'impl ... for PgStorage' blocks), confirming implementation swappability behind the same port abstraction."
          },
          {
            "file": "crates/gyre-domain/Cargo.toml",
            "quote": "# INVARIANT: gyre-domain MUST NOT depend on gyre-adapters or any infrastructure crate.",
            "detail": "No storage implementation details leak into domain logic: gyre-domain's dependency list contains no storage crate (no diesel, rusqlite, or sqlx), and scripts/check-arch.sh mechanically forbids those dependencies in the domain - the fourth bullet is satisfied."
          }
        ]
      },
      {
        "id": "5e7a44c5f15c34d70101b959",
        "decision": "gap",
        "requirement_quote": "- **REST** for admin panel, user-facing endpoints, and external integrations.\n- **gRPC** for internal service-to-service communication where performance matters.\n- **WebSocket** as the primary transport for agent ↔ server communication.",
        "detail": "The REST bullet is satisfied; the WebSocket bullet is a gap, so the section is a gap. (1) REST: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in api/mod.rs plus the top-level router in lib.rs), covering the admin panel (api/admin.rs), user-facing endpoints, and external integrations (webhooks, SCIM, MCP-over-HTTP). (2) WebSocket as the primary transport for agent-servers communication: GAP - the canonical agent runtime (docker/gyre-agent/agent-runner.mjs, the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket (zero matches for WebSocket/ws:// in the file): every agent-to-server interaction is HTTP - heartbeat (POST /api/v1/agents/{id}/heartbeat), log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), completion (POST /api/v1/agents/{id}/complete) - and server-to-agent message delivery is MCP/REST polling (gyre_message_poll, GET /api/v1/agents/{id}/messages) per docs/agent-protocol.md. The /ws endpoint is consumed by the browser dashboard SPA (web/src/lib/ws.js) and the CLI TUI (crates/gyre-cli/src/ws.rs) - human clients receiving event push. WebSocket is a UI push channel in the realized system, not the primary agent transport; making it the primary transport would require a WebSocket client in the agent runtime and server push of agent-directed messages over it. (3) gRPC: the realized platform is a single server binary with no internal service-to-service communication, so the 'where performance matters' condition has no trigger; the gRPC surfaces that exist are the OTLP trace receiver (gate-time telemetry ingest from the application under test - explicitly scoped, not general internal service-to-service) and the SPIFFE Workload API client to an external SPIRE server (crates/gyre-spiffe/src/workload_api.rs), neither of which is internal service-to-service communication. This is recorded for the controller to rule whether the monolithic realization already satisfies the conditional (no such communication exists to carry) or whether the spec's services decomposition was never built; it is not counted as an independent missing behavior because there is no realized internal service-to-service path that performs matters.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/merge-requests\",",
            "detail": "The REST bullet is satisfied: the domain API is REST, registered under literal /api/v1/ path prefixes (331 .route registrations in this file alone), covering admin (api/admin.rs), user-facing, and external-integration surfaces."
          },
          {
            "file": "docker/gyre-agent/agent-runner.mjs",
            "quote": "const resp = await fetch(`${serverUrl}/api/v1/agents/${agentId}/heartbeat`, {",
            "detail": "The canonical agent runtime (the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket - grep for WebSocket/ws:// in the 778-line file returns zero matches. All agent-to-server communication is HTTP fetch: heartbeat, log upload (POST /api/v1/agents/{id}/logs), MCP JSON-RPC (POST /mcp), and completion (POST /api/v1/agents/{id}/complete). The specced primary transport for agent-server communication is absent from the agent side."
          },
          {
            "file": "web/src/lib/ws.js",
            "quote": "ws = new WebSocket(`${protocol}//${window.location.host}/ws`);",
            "detail": "The /ws WebSocket endpoint is consumed by the browser dashboard SPA (and by the CLI TUI via crates/gyre-cli/src/ws.rs, DEFAULT_SERVER ws://localhost:3000/ws) - human clients receiving domain-event push. WebSocket serves the UI, not the agent transport, so it is not 'the primary transport for agent-server communication' in either direction."
          },
          {
            "file": "docs/agent-protocol.md",
            "quote": "can poll the message via the MCP surface (`gyre_message_poll`, which derives the inbox from the verified token subject — inherently own-inbox)",
            "detail": "Server-to-agent message delivery is also not WebSocket: the documented delivery path for agent inbound messages is MCP/REST polling (gyre_message_poll; the REST inbox route GET /api/v1/agents/{id}/messages exists for the same purpose), confirming the transport gap runs in both directions."
          },
          {
            "file": "crates/gyre-server/src/otlp_receiver.rs",
            "quote": "//! Lightweight OTLP gRPC receiver for gate-time trace capture (HSI §3a).",
            "detail": "On the gRPC bullet: the only in-process gRPC server is this OTLP trace receiver, scoped to gate-time trace capture from the application under test (the module doc explicitly disclaims being a general-purpose observability backend); the other tonic dependency is the SPIFFE Workload API client to an external SPIRE server. Neither is internal service-to-service communication - the realized platform is a single server binary with no internal service-to-service path, so the 'where performance matters' condition has no trigger."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "web/package.json",
        "sha256": "0252459af962c075563e00edde89de5602ad27780205c327e78fd667ef524f7d"
      },
      {
        "file": "web/src/lib/Button.svelte",
        "sha256": "22d9fb91bf4bf653d27f0da630a455c46c821e3119886c15c24d842146c26a1b"
      },
      {
        "file": "web/src/lib/design-system.css",
        "sha256": "c3da4938e691dd659c05c21d609e4750add2869ec527a33004615b3fa9b883eb"
      },
      {
        "file": "Cargo.toml",
        "sha256": "2298e83b544633e37c0ce6f857ee4879924d6f18efcf798ab1a8179e373b1a90"
      },
      {
        "file": "specs/system/agent-runtime.md",
        "sha256": "7429d169b0daee21b0823de52128b4691eb2d52b34bcec3130aad55beadb21ba"
      },
      {
        "file": "docker/gyre-agent/agent-runner.mjs",
        "sha256": "191665001a62a2a5f201e0e3a990abbe0bdef350a259dc4fb046ca9da54c79a1"
      },
      {
        "file": "crates/gyre-domain/Cargo.toml",
        "sha256": "8240418bb6771df07a1971b7c426a4297e226282795e683afe98ef920afe67ef"
      },
      {
        "file": "scripts/check-arch.sh",
        "sha256": "cdc29bf13ee7fe088d4053e37b7364f71824329453f58d1357af41191f528d0c"
      },
      {
        "file": "docs/development.md",
        "sha256": "1edb15b59093154a4c5a12a7fab657c948120c1f3dca1f955416456931e878ac"
      },
      {
        "file": ".github/workflows/ci.yml",
        "sha256": "a58652ba210d38bd13b8ae0171c0deb7fad593b916d9e2977078a9962404b97a"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "a23a71630b3302002b50d4dbc1f256cf95c2deeed11f69df4a02b083410f2771"
      },
      {
        "file": "docs/server-config.md",
        "sha256": "71223b8f614faaa87600eca60331dbf855640ee17845d5ec4d4a9081f8710f95"
      },
      {
        "file": "web/playwright.config.js",
        "sha256": "680c762582e4878b9b25ddd5c9eb3f6379d415efe53a99755538b7b47a8fd22a"
      },
      {
        "file": "crates/gyre-adapters/src/postgres/mod.rs",
        "sha256": "d7145875c33f42d1e78fa272f1e375f8e82d0568a46f684f7bc854b26b577866"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "036b32975f4110d6aa35cef408441c0be5ecb4cef4a9a7504eee9caf08a8bc35"
      },
      {
        "file": "web/src/lib/ws.js",
        "sha256": "d33daccdc771a4d892dfb883879d2f6043097f37320343827809b3bd42babb8f"
      },
      {
        "file": "docs/agent-protocol.md",
        "sha256": "43dc585da74f78f309cf243e94848f6feea8a3ad743f36b8f322dcc30721c39e"
      },
      {
        "file": "crates/gyre-server/src/otlp_receiver.rs",
        "sha256": "5bb1373dd02c5662dd87a63119f969339c845252c2654be6160f1ab84d3cfde1"
      },
      {
        "file": "specs/system/identity-security.md",
        "sha256": "280de5c11beaf947d40183cb200d49ef403bce79026bcc688bb25fc4295bb73e"
      },
      {
        "file": "crates/gyre-common/src/protocol.rs",
        "sha256": "e78f25a0483052879156f559381020417cc0e434819eb5cdf81ab4195e237c62"
      }
    ]
  },
  "plan": {
    "audit": "56c97d7f5ed5fd25692a0034fbb047628103c84a2ba4a40c86b12cab98680280",
    "tasks": [
      {
        "key": "sqlite-default-storage",
        "title": "SQLite as the default durable storage backend (with explicit in-memory opt-in and stateless harness migration)",
        "covers": [
          "df78f83918d8549c48c15ea5"
        ],
        "required_behavior": "Per specs/development/architecture.md §Storage: 'SQLite as the default (simple, vertical scaling, zero ops)'. Today crates/gyre-server/src/lib.rs build_state selects storage purely from GYRE_DATABASE_URL: SQLite only when the variable is set to a non-postgres URL, and with it unset (the shipped/documented default) every port falls back to an in-memory implementation — a server started with `cargo run -p gyre-server` persists nothing across restarts, and docs/server-config.md documents the inverse of the spec ('Unset = in-memory (default, stateless)'). Implement: (1) Default durable SQLite — when GYRE_DATABASE_URL is unset, build_state opens gyre_adapters::SqliteStorage at a platform-appropriate absolute default data path (XDG data home, e.g. ~/.local/share/gyre/gyre.db on Linux, via the dirs crate or equivalent; parent directories auto-created; NEVER a bare relative filename resolved against cwd — see scripts/check-relative-path-defaults.sh). SqliteStorage::new already runs the Diesel migrations; every port must be wired to SqliteStorage through the existing store! macro so the default deployment persists all entities across restarts with zero operator configuration. (2) Explicit in-memory opt-in — the previous stateless behavior is reachable ONLY via an explicit sentinel (e.g. GYRE_DATABASE_URL=memory), filtered out before the sqlite-path branch so it is never parsed as a sqlite file path; the in-memory policy-store sharing semantics for workspace/policy repos (the 'pure in-memory mode' sharing note in build_state) must be preserved under the sentinel. Explicit sqlite://path and postgres:// URLs keep their current selection behavior unchanged. (3) Test-harness migration — every test that relies on the in-memory default must explicitly opt in: the in-process integration tests calling build_state (112 call sites across crates/gyre-server/tests/*.rs) set the sentinel (or an equivalent explicit code path) so no test run writes to the real default data path or the repository working tree; tests that specifically exercise the SQLite default use isolated temp paths. (4) Process-level harness (controller review feedback id df78f83918d8549c48c15ea5:playwright-e2e-in-memory-optin) — update web/playwright.config.js webServer.env to set the explicit in-memory sentinel so the e2e suite keeps running stateless; .github/workflows/e2e.yml drives this same playwright config, and scripts/e2e-flow.sh requires an externally started server (verified: no other harness spawns the server binary). Audit for any other harness that spawns the gyre-server binary and give it the same explicit opt-in. (5) Update docs/server-config.md (GYRE_DATABASE_URL row: default is now durable SQLite at the documented absolute data path; memory sentinel for stateless runs; explicit sqlite:// and postgres:// URLs unchanged) so operator documentation matches the new shipped default.",
        "acceptance": [
          "With GYRE_DATABASE_URL unset, `cargo run -p gyre-server` boots on SQLite at the default absolute data path (path logged at startup): an entity created via the API before a full server stop/restart (e.g. a workspace via POST /api/v1/workspaces) is retrievable after the restart — a real persistence round-trip against real storage, including migrations having run on the fresh file.",
          "With GYRE_DATABASE_URL=memory the server is stateless exactly as before (state lost across restart) and no database file is created at the default data path.",
          "Explicit sqlite://<path> and postgres:// URLs select their backends exactly as before (no regression in backend selection or in the in-memory policy-store sharing under the sentinel).",
          "No test run creates or modifies a gyre.db/SQLite file in the repository working tree or at the user's real default data path; the playwright e2e webServer runs on the in-memory opt-in — an e2e run creates no durable file at the default path, and repeated local runs with reuseExistingServer behave identically (no state bleed across runs).",
          "docs/server-config.md documents the new default, the memory sentinel, and the default data path accurately."
        ],
        "negative_probe": "Boot the real server binary with GYRE_DATABASE_URL unset, create a workspace via POST /api/v1/workspaces, terminate the server, restart it, and GET the workspace by id: on the current code (in-memory default) the GET returns 404 because all state died with the process — the real defect this probe must expose; with the fix the workspace is served from the SQLite default. A second probe: run the playwright e2e suite twice locally with reuseExistingServer and assert no database file appears at the default data path — without the webServer env opt-in the harness silently boots on durable storage outside the working tree.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "shadcn-svelte-ui-stack",
        "title": "Adopt the specced Svelte 5 + shadcn-svelte web UI stack, replacing the bespoke component library",
        "covers": [
          "2a696c7f5c2ffc02e88742ae"
        ],
        "required_behavior": "This task closes ONLY the shadcn-svelte half of the section, pending a controller ruling on the Rust half (see escalation below). Per specs/development/architecture.md §Tech Stack: 'Svelte 5 + shadcn-svelte for the web UI'. Svelte 5 is satisfied (web/package.json pins svelte ^5.0.0), but shadcn-svelte is used nowhere: web/package.json contains no shadcn-svelte, no tailwindcss, and no bits-ui; the UI is a hand-written component library (web/src/lib/*.svelte: Button, Card, Badge, Tabs, Table, Modal, Toast, Accordion, ...) styled by the bespoke 'Gyre Design System — Red Hat Brand Standards' stylesheet (web/src/lib/design-system.css). Implement: (1) Foundations — add shadcn-svelte's required foundations to web/: Tailwind CSS wired into the Vite build (Tailwind entry stylesheet imported by the app), bits-ui, clsx/tailwind-merge, and shadcn-svelte component generation configured per its convention (components generated under e.g. web/src/lib/components/ui/). (2) Component migration — the reusable UI primitives in web/src/lib/*.svelte become shadcn-svelte components with the same consumed contracts (props, events, accessibility) so web/src/components/* and views keep working; the Red Hat brand palette is mapped into the Tailwind/shadcn theme tokens (the documented brand identity is preserved as theme values, not discarded); the bespoke .btn/.card-style class layer and design-system.css class system are retired from component styling. (3) Verification without test inflation — the existing vitest component tests and e2e functional assertions must pass against the migrated components; visual screenshot baselines are regenerated deliberately where the restyle changes pixels (functional assertions, not pixel equality, are the behavioral anchor), and the committed web/dist is rebuilt (SKIP_WEB_BUILD=1 Rust-only builds and the in-sandbox visual config serve web/dist). (4) Coordination — the ui-layout.md plan (planned) and assigned web-behavior tasks (task-285 dependency-graph, task-294 spec-links, task-296 activity-dashboard, task-307 explorer-canvas) modify the same component files behaviorally; this task changes only the styling/component foundation with no behavioral contract overlap — coordinate merge order, do not fold their behaviors in. RUST-HALF ESCALATION (controller review feedback id 2a696c7f5c2ffc02e88742ae:rust-agent-runtime-reading-mischaracterized): architecture.md's parenthetical 'Rust as the primary language (server, CLI, agent runtime)' conflicts with canonical specs/system/agent-runtime.md, which mandates a Node.js in-container agent image — agent-runner.mjs entrypoint on the Claude Agent SDK, with the Nix flake pinning 'Node.js version, Claude Agent SDK version, git, curl' and 'All backends run the same agent image' — and the shipped runtime docker/gyre-agent/agent-runner.mjs is JavaScript. Server and CLI are Rust (satisfied). This is a spec-lifecycle amendment decision for the controller (amend architecture.md's parenthetical, or mandate a Rust in-container runner); the task makes NO code change either way for the Rust half and does not assert either reading as verified.",
        "acceptance": [
          "web/package.json declares and the build genuinely uses the shadcn-svelte foundations (tailwindcss, bits-ui, clsx/tailwind-merge, generated ui components): `npm run build` output is styled by the Tailwind pipeline.",
          "The reusable UI primitives (Button, Card, Badge, Tabs, Table, Modal/Dialog, Toast, Accordion, ...) are shadcn-svelte components; the bespoke .btn/.card class library no longer styles them, with the Red Hat brand palette carried as theme tokens.",
          "`cd web && npm test` passes against the migrated components, and the playwright e2e suite passes with deliberately regenerated screenshot baselines (functional assertions unchanged).",
          "Committed web/dist serves the migrated UI (SKIP_WEB_BUILD=1 cargo build -p gyre-server serves it).",
          "The Rust-half canonical conflict is recorded as an open controller escalation, not closed: no code or coverage claim disposes of the '(server, CLI, agent runtime)' parenthetical."
        ],
        "negative_probe": "Mount web/src/lib Button in vitest and assert it renders as a shadcn-svelte component (cva/variant class structure, bits-ui primitive behavior where applicable): on the current bespoke Button (class=\"btn btn-{variant} btn-{size}\" styled by design-system.css) the probe fails — the real defect being excluded is a bolted-on config (tailwind present in package.json) while bespoke CSS still does the actual styling. Corroborating probe: remove the Tailwind entry stylesheet import from the app — the build/probe must fail or produce unstyled output, proving the UI genuinely depends on the Tailwind pipeline rather than carrying dead configuration.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "websocket-agent-transport",
        "title": "WebSocket as the primary agent↔server transport: WS client in the agent runtime, real-time agent-directed push, and WS-borne liveness",
        "covers": [
          "5e7a44c5f15c34d70101b959"
        ],
        "required_behavior": "Per specs/development/architecture.md §API: 'WebSocket as the primary transport for agent ↔ server communication'. REST and the versioning rules in the same section are satisfied; the gap is that the canonical agent runtime (docker/gyre-agent/agent-runner.mjs, the entrypoint mandated by specs/system/agent-runtime.md) never opens a WebSocket — every agent→server interaction is HTTP fetch (heartbeat POST /api/v1/agents/{id}/heartbeat, logs, MCP JSON-RPC POST /mcp, completion) and server→agent delivery is MCP/REST polling (gyre_message_poll / GET /api/v1/agents/{id}/messages), which the runner does not even perform — so the agent receives no server-directed signal in real time. Much of the server side already exists (ws.rs authenticates agent JWTs via the Auth message, subscribes agent connections to their own workspace, and delivers Destination::Agent bus messages to the agent's own live session). Implement: (1) Agent-side WS client — agent-runner.mjs opens and maintains a WebSocket connection to /ws for the entire session using the agent's token (Auth message, Subscribe to its own workspace), with reconnect/backoff so the transport survives transient drops. (2) Real-time inbound — the runner receives server-pushed agent-directed messages (Destination::Agent: gate-failure feedback, shutdown commands, spec-rejection grace signals) over the WS and acts on them per the protocol (e.g. a shutdown signal triggers the graceful completion/stop path), instead of never receiving them. (3) WS-borne liveness — the runner's heartbeat travels over the WS as the primary channel, and the server feeds WS heartbeats into the same domain liveness path as POST /api/v1/agents/{id}/heartbeat so stale-agent detection behaves identically; the HTTP heartbeat and MCP polling surfaces remain as compatibility/fallback per their own canonical contracts (docs/agent-protocol.md, agent-gates.md live-session broadcast) — the spec says primary, not exclusive, and those surfaces keep their documented semantics. (4) Documentation — update docs/agent-protocol.md to describe the WS as the agent's primary channel with HTTP/MCP as fallback (docs, not spec amendment). (5) Coordination — task-272 (agent-workflow pre-PR local verification) modifies agent-runner.mjs's completion flow under a different contract; coordinate merge order, do not fold its behavior in. (6) gRPC bullet — per the supplied audit, the realized platform is a single server binary with no internal service-to-service path (only the scoped OTLP trace receiver and the external SPIFFE Workload API client exist), so the 'where performance matters' conditional has no trigger; that reading is recorded for the controller and this task implements no gRPC service-to-service surface either way.",
        "acceptance": [
          "agent-runner.mjs maintains an authenticated WS connection for the session's duration (server-side observable: the connection is registered under the agent's verified identity).",
          "A bus message sent to Destination::Agent(<id>) is received by the running agent over the WebSocket without any polling, and the runner reacts to it (shutdown signal → graceful stop within the specced window).",
          "WS heartbeats keep the agent Alive in stale-agent detection through the same liveness path as the HTTP heartbeat: with no HTTP heartbeat sent, an agent heartbeating only over WS is not marked Dead; with the WS heartbeat path removed, the agent is marked Dead per the existing staleness window.",
          "The HTTP heartbeat, MCP polling, and /ws human-client (SPA/CLI) paths keep their current documented semantics — fallback and UI channels, unchanged."
        ],
        "negative_probe": "Start the real agent runner (preview/local mode) against a live test server, send a shutdown message to that agent's id on the bus, and assert the runner initiates its graceful stop within a bounded window: on the current code the probe times out — the runner never connects (zero WebSocket usage in agent-runner.mjs) and never polls, so no server-directed signal can reach it; the fixed transport must deliver the message over the WS and produce the reaction. A second probe: disable the WS-heartbeat feed and assert stale-agent detection marks the agent Dead, proving WS heartbeats genuinely drive liveness rather than existing as an unused client code path.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "sqlite-default-storage": "task-308",
    "shadcn-svelte-ui-stack": "task-309",
    "websocket-agent-transport": "task-310",
    "_publication": "task-311",
    "_base": "3ee882f1ddb29ed9dce4b47919d5da3e79702400"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
