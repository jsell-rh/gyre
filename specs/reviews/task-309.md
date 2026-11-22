# Review — task-309 (Adopt the specced Svelte 5 + shadcn-svelte web UI stack)

Spec: `specs/development/architecture.md` §Tech Stack — "Svelte 5 + shadcn-svelte
for the web UI" (shadcn half only; the Rust-half canonical conflict stays an
open controller escalation, review feedback id
`2a696c7f5c2ffc02e88742ae:rust-agent-runtime-reading-mischaracterized`).
Candidate under review: `6d7aa9706ab5a581e0bba35bcd924fcbde346716` (branch
`pipeline/task-309/6147b7338d73441dbd63731759064a7a-1`, base/main `50575cf6`;
product commits `4d5e209b`, `ae5d3eb4`/`9a56ebdf` dist builds, `f863a10d`
checkpoint; merge repairs `d88b8fe0`, `76110a31`).
Verdict: **approved** — no findings. Full evidence, commands, and logs under
`/tmp/stage/review-evidence/` (summary: `evidence-summary.md`).

## What was verified (independent probes, candidate checkout)

- **Foundations are real, not bolted-on config.** `web/package.json` declares
  `tailwindcss` ^4.3.5 + `@tailwindcss/vite`, `bits-ui`, `clsx`,
  `tailwind-merge`, `class-variance-authority`, `tw-animate-css`;
  `web/vite.config.js` wires the Tailwind plugin; `web/src/app.css` is the
  Tailwind entry stylesheet imported first by `web/src/main.js`;
  `web/components.json` follows the shadcn-svelte generation convention with
  generated components under `web/src/lib/components/ui/{button,badge,card,
  table,tabs,dialog,accordion,toast,skeleton}/`; `web/src/lib/utils.js`
  provides `cn()` (clsx + tailwind-merge).
- **Build pipeline probe (AC 1).** `npx vite build` from the merged source
  emits CSS with the `tailwindcss v4.3.5` marker, `@layer theme/utilities`,
  the shadcn token set carrying the Red Hat palette
  (`--background:#151515`, `--destructive-hover:#c04417`,
  `--color-primary:var(--primary)`), and utilities resolving to those tokens
  (`.bg-primary{background-color:var(--primary)}`, `.text-primary-foreground`,
  `.rounded-md{border-radius:.25rem}`).
- **Corroborating probe (verification intent).** With `import './app.css'`
  removed from `main.js`, the build emits zero Tailwind output (no marker, no
  layers, no tokens, no utilities) — the UI genuinely depends on the Tailwind
  pipeline rather than carrying dead configuration. Source restored; tree
  clean afterwards.
- **Component migration (AC 2).** `web/src/lib/Button.svelte` and
  `Modal.svelte` are facades over the shadcn components on bits-ui primitives
  (`ButtonPrimitive.Root`, Dialog parts — verified by the
  `data-button-root`/`data-dialog-content` markers in the rendering tests);
  Badge, Card, Table, Tabs, AccordionItem, Toast, Skeleton are cva-variant +
  Tailwind-utility components. Consumed contracts (props, events, aria
  wiring) are preserved so views keep working. The bespoke `.btn`/`.card`
  scoped class layer is retired from the primitives (the base's styling lived
  in Svelte `<style>` blocks, which are gone from the migrated components;
  legacy class names remain only as inert DOM hooks). The Red Hat palette is
  carried as the shadcn theme token values in `app.css`.
- **Test quality (no test inflation).** Mutation probe: restoring the base
  bespoke `Button.svelte` makes `ShadcnUiStack.test.js` fail 2/7 (missing
  bits-ui marker; `btn btn-secondary btn-md` instead of cva classes) — the
  probe kills the exact regression the verification intent names.
- **Full web vitest suite (AC 3, first half).** `npx vitest run` on the merged
  tree: 128 files, 2599 passed, 41 skipped, 0 failed — matches the shipped
  claim, including the merged task-281 `ImpactAnalysisModal` suite (23
  tests), validating the merge resolution of the conflicted
  `web/src/lib/api.js` / `ImpactAnalysisModal.svelte` / test files.
- **Explorer-visual e2e against the real migrated bundle (AC 3, visual
  half).** Served the freshly built Tailwind bundle with a plain-node static
  server (the spec route-intercepts all browser API traffic; only the seed
  fixture's two node-side endpoints were answered) and ran
  `npx playwright test tests/e2e/explorer-visual.spec.js`: **16/16 passed**
  with the committed, unchanged baselines (2% maxDiffPixelRatio). The restyle
  is pixel-faithful, so the task's "regenerate where the restyle changes
  pixels" condition is vacuously satisfied; functional assertions are
  unchanged (`web/tests` has no delta from base).
- **Scope discipline (AC 5 + coordination).** No Rust delta from main
  (`git diff 50575cf6..HEAD -- crates/ Cargo.toml docker/` is empty). No
  coverage row exists for the development/architecture spec (coverage
  matrices cover `specs/coverage/system/` only), and the task notes keep the
  Rust-half canonical conflict as an open controller escalation — no code or
  coverage claim disposes of the "(server, CLI, agent runtime)" parenthetical.

## Recorded observations (non-blocking)

- **Generated output (verifier-owned).** The committed `web/dist` is
  byte-identical to upstream main `50575cf6` (bespoke-CSS build; no Tailwind
  output). Per the review framework the deterministic verifier rebuilds
  `web/dist` from the merged source and binds publication to that tested
  tree; the source itself builds a genuine Tailwind v4 bundle (probe above).
- **Test-asset history.** No commit touches `web/tests` — the visual
  baselines are unchanged since task-082 — so the shipped note's claim that
  baselines "were already regenerated for the shadcn restyle in the prior
  verified round" is not backed by git history. Substantively harmless:
  the unchanged baselines pass against the migrated UI (16/16, probe above).

## Verifier must run (deferred per the 4 GiB build budget)

1. `cd web && npm run test:e2e` — the full playwright suite against the real
   `gyre-server`, including the 49 functional `app.spec.js` tests that consume
   real admin-seeded data (cannot run in the review sandbox: boots
   `cargo run -p gyre-server`). The 16 explorer-visual tests already pass
   against the rebuilt bundle in this review (see above).
2. Rebuild `web/dist` from the merged source, commit, and verify
   `SKIP_WEB_BUILD=1 cargo build -p gyre-server` serves the migrated
   (Tailwind) UI — the committed dist currently matches upstream main.
3. Standard Rust workspace checks (the branch carries no Rust delta).
