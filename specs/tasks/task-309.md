---
title: "Adopt the specced Svelte 5 + shadcn-svelte web UI stack, replacing the bespoke component library"
spec_ref: "Spec gap spec-f907eda3bd014fe546b838e2 / shadcn-svelte-ui-stack / 4156114b8040"
depends_on: []
progress: ready-for-review
commits: ["40fcedae4ccabed95f2575598d300eac859764e8", "f863a10dd81d4fb9e8e6ede73714c0f6f1e8b127", "9a56ebdfc15a3a9023d1356d21953d977f51c293", "ae5d3eb45e44bf40a01680b674fd2b821d6c9496", "4d5e209b474cc94fd5b4f26ba3144b742df9e55d", "493d1e10ad07c1705d4f015601848cce06f1159b", "b9bcaf5ee201e7969dc622867f757654a33b4946"]
---

## Required behavior

This task closes ONLY the shadcn-svelte half of the section, pending a controller ruling on the Rust half (see escalation below). Per specs/development/architecture.md §Tech Stack: 'Svelte 5 + shadcn-svelte for the web UI'. Svelte 5 is satisfied (web/package.json pins svelte ^5.0.0), but shadcn-svelte is used nowhere: web/package.json contains no shadcn-svelte, no tailwindcss, and no bits-ui; the UI is a hand-written component library (web/src/lib/*.svelte: Button, Card, Badge, Tabs, Table, Modal, Toast, Accordion, ...) styled by the bespoke 'Gyre Design System — Red Hat Brand Standards' stylesheet (web/src/lib/design-system.css). Implement: (1) Foundations — add shadcn-svelte's required foundations to web/: Tailwind CSS wired into the Vite build (Tailwind entry stylesheet imported by the app), bits-ui, clsx/tailwind-merge, and shadcn-svelte component generation configured per its convention (components generated under e.g. web/src/lib/components/ui/). (2) Component migration — the reusable UI primitives in web/src/lib/*.svelte become shadcn-svelte components with the same consumed contracts (props, events, accessibility) so web/src/components/* and views keep working; the Red Hat brand palette is mapped into the Tailwind/shadcn theme tokens (the documented brand identity is preserved as theme values, not discarded); the bespoke .btn/.card-style class layer and design-system.css class system are retired from component styling. (3) Verification without test inflation — the existing vitest component tests and e2e functional assertions must pass against the migrated components; visual screenshot baselines are regenerated deliberately where the restyle changes pixels (functional assertions, not pixel equality, are the behavioral anchor), and the committed web/dist is rebuilt (SKIP_WEB_BUILD=1 Rust-only builds and the in-sandbox visual config serve web/dist). (4) Coordination — the ui-layout.md plan (planned) and assigned web-behavior tasks (task-285 dependency-graph, task-294 spec-links, task-296 activity-dashboard, task-307 explorer-canvas) modify the same component files behaviorally; this task changes only the styling/component foundation with no behavioral contract overlap — coordinate merge order, do not fold their behaviors in. RUST-HALF ESCALATION (controller review feedback id 2a696c7f5c2ffc02e88742ae:rust-agent-runtime-reading-mischaracterized): architecture.md's parenthetical 'Rust as the primary language (server, CLI, agent runtime)' conflicts with canonical specs/system/agent-runtime.md, which mandates a Node.js in-container agent image — agent-runner.mjs entrypoint on the Claude Agent SDK, with the Nix flake pinning 'Node.js version, Claude Agent SDK version, git, curl' and 'All backends run the same agent image' — and the shipped runtime docker/gyre-agent/agent-runner.mjs is JavaScript. Server and CLI are Rust (satisfied). This is a spec-lifecycle amendment decision for the controller (amend architecture.md's parenthetical, or mandate a Rust in-container runner); the task makes NO code change either way for the Rust half and does not assert either reading as verified.

## Acceptance Criteria

- [ ] web/package.json declares and the build genuinely uses the shadcn-svelte foundations (tailwindcss, bits-ui, clsx/tailwind-merge, generated ui components): `npm run build` output is styled by the Tailwind pipeline.
- [ ] The reusable UI primitives (Button, Card, Badge, Tabs, Table, Modal/Dialog, Toast, Accordion, ...) are shadcn-svelte components; the bespoke .btn/.card class library no longer styles them, with the Red Hat brand palette carried as theme tokens.
- [ ] `cd web && npm test` passes against the migrated components, and the playwright e2e suite passes with deliberately regenerated screenshot baselines (functional assertions unchanged).
- [ ] Committed web/dist serves the migrated UI (SKIP_WEB_BUILD=1 cargo build -p gyre-server serves it).
- [ ] The Rust-half canonical conflict is recorded as an open controller escalation, not closed: no code or coverage claim disposes of the '(server, CLI, agent runtime)' parenthetical.

## Verification intent

Mount web/src/lib Button in vitest and assert it renders as a shadcn-svelte component (cva/variant class structure, bits-ui primitive behavior where applicable): on the current bespoke Button (class="btn btn-{variant} btn-{size}" styled by design-system.css) the probe fails — the real defect being excluded is a bolted-on config (tailwind present in package.json) while bespoke CSS still does the actual styling. Corroborating probe: remove the Tailwind entry stylesheet import from the app — the build/probe must fail or produce unstyled output, proving the UI genuinely depends on the Tailwind pipeline rather than carrying dead configuration.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/architecture.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `68777e1ea1f58acc822cb4f700aaa458b561ec119531c168e7c1d07d2026a23e`.

[
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
  }
]

## Shipped

The shadcn-svelte half of the architecture.md §Tech Stack requirement is
implemented (commit 4d5e209b, preserved through the merge repairs up to
HEAD). The Rust-half canonical conflict remains an open controller
escalation — see the end of this section. This round repaired the CI
failure on the task's PR (see below); no web source changed, so the
prior rounds' web verification stands.

### Retained implementation (commit 4d5e209b, prior approved rounds)

- `web/package.json` declares and the build genuinely uses `tailwindcss`
  ^4 + `@tailwindcss/vite`, `bits-ui`, `clsx`, `tailwind-merge`,
  `class-variance-authority`, `tw-animate-css`. `web/src/app.css` is the
  Tailwind entry stylesheet imported first by `web/src/main.js`; it
  carries the shadcn token set with the Red Hat brand palette as theme
  values. `web/components.json` follows the shadcn-svelte generation
  convention; generated components live under
  `web/src/lib/components/ui/{button,badge,card,table,tabs,dialog,
  accordion,toast,skeleton}/`; `web/src/lib/utils.js` provides `cn()`.
- The reusable primitives in `web/src/lib/*.svelte` are shadcn-svelte
  components behind the existing consumed contracts (Button and
  Modal/Dialog on bits-ui primitives; Badge, Card, Table, Tabs,
  AccordionItem, Toast, Skeleton as cva-variant + Tailwind-utility
  components), so `web/src/components/*` and all views keep working.
  The bespoke `.btn`/`.card`/`.badge` class layer is retired from
  component styling; the Red Hat brand identity is preserved as the
  shadcn theme token values. Verification probe:
  `web/src/__tests__/ShadcnUiStack.test.js` asserts the cva/bits-ui
  rendering and the wired Tailwind pipeline; the corroborating
  remove-the-Tailwind-entry-import probe (unstyled rebuild without
  `app.css`) was run in a prior round with outputs retained under
  /tmp/stage/review-evidence.
- Prior-round web test evidence on the merged tree: `npx vitest run` —
  128 files, 2603 tests passed, 0 failed. This round, `cargo check -p
  gyre-server` ran build.rs's `npm run build` on the current tree
  successfully ("Web assets built successfully"), and a plain `vite
  build` reproduces the committed web/dist asset hashes exactly.

### This round: CI repair (commit b9bcaf5e)

PR #921 run 45132923610 (head 7cb19ff) failed
`gyre-server::scim_groups::mappings_and_groups_survive_restart` at
scim_groups.rs:844 — `assert_eq!(resp.status(), StatusCode::CREATED)`
got 500 vs 201 on `POST /scim/v2/Groups`.

Root cause, verified empirically in-sandbox (evidence under
/tmp/stage/review-evidence): `SqliteCustomizer` batched three PRAGMAs
into ONE diesel `sql_query` string. Diesel's SQLite backend prepares
statements via `sqlite3_prepare_v2`, which compiles only the FIRST
statement of a multi-statement string — `PRAGMA foreign_keys=ON` and
`PRAGMA busy_timeout=5000` were silently dropped, so busy_timeout stayed
at SQLite's default 0. Concurrent writers on the same file then raced
for the write lock with no wait; the loser got an immediate
`database is locked` (SQLITE_BUSY) surfaced as `ApiError::Internal` →
500. The failing test is the only scim_groups test whose group-create
POST is immediately preceded by an authenticated workspace-scoped
request (`PUT /api/v1/workspaces/{id}/scim-mappings` with the owner's
token), which spawns a fire-and-forget `upsert_last_seen` write in
`last_seen_middleware` — the two INSERTs contended and the group insert
lost the race under CI scheduling (hence the flakiness: identical
creates passed in the sibling tests of the same run).

Fix: execute each PRAGMA as its own `sql_query` in the customizer
(`foreign_keys=ON` is a no-op today — the migrations declare no FK
constraints — and `journal_mode=WAL` was already the first statement,
so only busy_timeout behavior changes).

Regression guards added in `gyre-adapters` (both verified to FAIL on the
batched form and PASS after the fix):
- `pooled_connections_have_wal_foreign_keys_and_busy_timeout` — reads
  the three PRAGMAs back off pooled connections (busy_timeout was 0
  before the fix).
- `concurrent_writers_wait_on_busy_timeout_instead_of_failing` — holds
  the SQLite write lock on one connection and INSERTs from a second;
  fails with `database is locked` before, succeeds after.

In-sandbox probes this round: `cargo test -p gyre-adapters` — 610
passed, 0 failed; `cargo check -p gyre-server` clean;
`scripts/check-task-commit-attribution.sh` OK after restoring the
commit list above.

### Deferred to the independent verifier

- `cargo nextest run -p gyre-server --test scim_groups` (the repaired
  CI test) and the full server suite — gyre-server test compilation
  exceeds the in-model 4 GiB budget and must run in the verifier.
- Playwright e2e with deliberately regenerated screenshot baselines
  (unchanged from prior rounds; no web source changed this round).
- Workspace gates (arch lint, clippy, fmt on all targets), GitHub CI on
  the exact head, and web/dist rebuild/publication (executor-owned; a
  fresh `vite build` of the current tree reproduces the committed dist
  hashes, so the committed dist already serves the migrated UI).

### Open controller escalation (Rust half — NOT closed by this task)

architecture.md's 'Rust as the primary language (server, CLI, agent
runtime)' parenthetical conflicts with canonical
specs/system/agent-runtime.md, which mandates a Node.js in-container
agent image (agent-runner.mjs entrypoint on the Claude Agent SDK, Node.js
pinned by the Nix flake) — and the shipped runtime
docker/gyre-agent/agent-runner.mjs is JavaScript. Server and CLI are
Rust (satisfied). This is a spec-lifecycle amendment decision for the
controller (amend architecture.md's parenthetical, or mandate a Rust
in-container runner); no code or coverage claim in this task disposes of
it either way.
