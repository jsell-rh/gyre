---
title: "Adopt the specced Svelte 5 + shadcn-svelte web UI stack, replacing the bespoke component library"
spec_ref: "Spec gap spec-f907eda3bd014fe546b838e2 / shadcn-svelte-ui-stack / 4156114b8040"
depends_on: []
progress: complete
commits: ["ae5d3eb45e44bf40a01680b674fd2b821d6c9496", "4d5e209b474cc94fd5b4f26ba3144b742df9e55d", "f629be5ed3af9df91d642770d28ddecd7f47d890"]
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
implemented (commit 4d5e209b, retained through this round's merge repair);
the Rust-half canonical conflict remains an open controller escalation
(see the end of this section).

### Implementation (shadcn-svelte foundations and component migration)

- `web/package.json` declares and the build genuinely uses `tailwindcss` ^4
  + `@tailwindcss/vite` (wired into `web/vite.config.js`), `bits-ui`,
  `clsx`, `tailwind-merge`, `class-variance-authority`, `tw-animate-css`.
  `web/src/app.css` is the Tailwind entry stylesheet imported first by
  `web/src/main.js`; it carries the shadcn token set with the Red Hat brand
  palette as theme values (exact mirror of the design-system.css palette).
  `web/component.json`-style generation config (`web/components.json`)
  follows the shadcn-svelte convention; generated components live under
  `web/src/lib/components/ui/{button,badge,card,table,tabs,dialog,
  accordion,toast,skeleton}/`; `web/src/lib/utils.js` provides `cn()`.
- The reusable primitives in `web/src/lib/*.svelte` are shadcn-svelte
  components behind the existing consumed contracts, so `web/src/
  components/*` and all views keep working: Button and Modal/Dialog are
  built on bits-ui primitives (Button.Root; Dialog Root/Overlay/Content/
  Title/Close with focus trap, focus restore, Escape, scroll lock); Badge,
  Card, Table, Tabs, AccordionItem, Toast and Skeleton are cva-variant +
  Tailwind-utility components. Tabs and AccordionItem keep the WAI-ARIA
  manual-activation / controlled-item implementations because their
  consumed contracts (caller-rendered tab panels with `tab-{id}`/
  `tabpanel-{id}` aria wiring; standalone controlled accordion items with
  caller-visible heading/body ids) cannot be satisfied by bits-ui's
  content-registration model — documented in each component. The bespoke
  `.btn`/`.card`/`.badge`-style scoped class layers are retired from
  component styling (legacy class names remain only as inert DOM hooks
  that tests and parent-scoped CSS target); the Red Hat brand identity is
  preserved as the shadcn theme token values.

### This round: merge repair (rebase category)

The published head (ec5da78f) conflicted with current main (73c504f2,
task-301 Meta-spec API). Resolved as merge commit d88b8fe0, preserving the
retained implementation: the task-309 branch is web-only, main's delta is
server/docs plus the `web/src/lib/api.js` route rename
(`/meta-specs-registry` -> `/meta-specs`), so the merge takes main's
server side unchanged (byte-identical to main for crates/docs/scripts —
no Rust source was hand-merged) and keeps the branch's shadcn web source,
with api.js aligned to the merged server's routes. Committed `web/dist`
is main's coherent build (the assignment base); the executor restores
dist from base and final integration rebuilds and publishes it remotely.

### Test evidence

- Focused probe on the merged tree: `npx vitest run
  src/__tests__/ShadcnUiStack.test.js` — 7/7 passed (Button renders cva
  variant classes on the bits-ui primitive, not the retired
  `btn btn-{variant}` layer; Badge/Modal render shadcn/bits-ui
  structures; Tailwind pipeline genuinely wired).
- Full web suite on the merged tree: `cd web && npm test` (after
  `npm ci`) — 128 files / 2596 tests passed, 41 skipped.
- Production build from the merged source (`npx vite build --outDir
  /tmp/dist-check`) succeeds; committed `web/dist` untouched.
- Visual e2e: the merge's only web/src delta is api.js route-name
  strings, which the explorer-visual specs never intercept (no
  meta-spec references in web/tests/e2e), so the rendering surface is
  identical to the previously verified ec5da78f state (16/16 with
  existing baselines; a deliberate `--update-snapshots` rerun produced
  zero baseline changes — prior round's review evidence). The
  dist-serving visual run belongs to final integration after the remote
  dist rebuild.
- Evidence artifacts: /tmp/stage/review-evidence/task-309/ (notes.md,
  shadcn-probe.txt, merge-commit-stat.txt).

### Deferred to verifier / integration

- Full functional e2e (`tests/e2e/app.spec.js`) and the standard server
  suite for the merge commit: run in the independent verifier (the merge
  introduces zero Rust source changes relative to main, which is already
  CI-verified there; no cargo build was run in this sandbox per the
  4 GiB budget).
- `web/dist` rebuild + publication, then `SKIP_WEB_BUILD=1 cargo build
  -p gyre-server` serving the migrated UI.
- Environment notes from the prior round remain: check-web-tests.sh
  fails on the assignment base too in-sandbox (npm arg forwarding); the
  in-sandbox DOM probe's 2 lens-toggle failures are pre-existing on base.

### Rust-half escalation (open, not closed)

architecture.md's "(server, CLI, agent runtime)" parenthetical conflicts
with canonical specs/system/agent-runtime.md (Node.js in-container agent
image, agent-runner.mjs entrypoint). This task makes no code change and
no coverage claim for that half; the section's Rust-half disposition
remains a controller spec-lifecycle ruling (feedback id
2a696c7f5c2ffc02e88742ae).
