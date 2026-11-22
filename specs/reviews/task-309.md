# Review — task-309 (shadcn-svelte UI stack adoption)

Candidate: `c4ca8e6991c45cc41e7d867f92038e5a729946b5` (base `73c504f2b5cfb30dd4670e06a0d90c97a1af03a1`)
Verdict: **approved** (independent evidence below; verifier checks recorded at the end)

## Scope

Diff base→candidate touches only `web/` and `specs/tasks/task-309.md`
(0 lines on `crates/`, `scripts/`, `docs/`, `Cargo.toml`, `Cargo.lock`,
`build.rs` — the merge commit d88b8fe0 took main's server side unchanged, as
the shipped notes claim). Reviewed: authored web source, tests, and task
record. `web/dist` is verifier-owned generated output (observation below).

## What was verified (evidence: /tmp/stage/review-evidence/, notes.md)

1. **Foundations are real, not bolted on.** `web/package.json` declares
   `tailwindcss` ^4 + `@tailwindcss/vite` (wired in `vite.config.js`),
   `bits-ui`, `clsx`, `tailwind-merge`, `class-variance-authority`,
   `tw-animate-css`; `components.json` follows the shadcn-svelte convention
   ($lib aliases, components under `web/src/lib/components/ui/`); `cn()` in
   `web/src/lib/utils.js`. Production build from the merged source
   (`vite build --outDir` temp; committed dist untouched) emits
   `.bg-primary{background-color:var(--primary)}`, `--primary:#ee0000`
   (Red Hat brand), tw-animate rules — the output is styled by the Tailwind
   pipeline.
2. **Pipeline dependency proven by negative probe.** Removing
   `import './app.css'` from `main.js` and rebuilding produces a CSS bundle
   with ZERO Tailwind utilities (unstyled output); the import was restored.
   The corroborating probe from the task's verification intent holds.
3. **Primitives genuinely migrated.** `lib/{Button,Badge,Card,Modal,Table,
   Tabs,Toast,AccordionItem,Skeleton}.svelte` are facades over
   `components/ui/*` parts (cva variant configs + `cn` + Tailwind utilities);
   Button is on bits-ui `Button.Root` (`data-button-root` marker), Modal on
   bits-ui Dialog (Root/Overlay/Content/Title/Close, focus trap/restore,
   Escape, scroll lock). The bespoke scoped `.btn`/`.card`/`.badge`/`.modal`
   style layers are deleted from the primitives; legacy class names remain
   only as inert DOM hooks for parent-scoped CSS and tests. The Red Hat
   palette is mirrored exactly as shadcn theme tokens in `app.css`
   (design-system.css is retained, wrapped in `@layer base`, as the app's
   reset/base — coherent layering: base < utilities < scoped styles).
4. **Verification test is discriminating.** `ShadcnUiStack.test.js` passes
   7/7 on the candidate and FAILS 2/7 with the base bespoke Button swapped
   in (negative control run and restored) — it excludes the
   bolted-on-config defect it names, and is not self-confirming.
5. **Full web suite passes.** `cd web && npm test` after `npm ci`:
   128 files / 2596 tests passed, 41 skipped — matching the shipped notes.
6. **Mechanical invariants.** `check-task-commit-attribution.sh` passes
   against the committed task file (4d5e209b recorded; 97f5da14 is a merge
   commit, correctly out of scope; the working-tree failure observed during
   review was an artifact of the stage setup resetting the task file's
   frontmatter, not of the candidate). `check-dead-components.sh`'s 4
   violations are byte-identical pre-existing baseline (same reference sets
   at base and candidate). Other JS test-quality checks: exit 0.
7. **Rust-half escalation stays open.** No code or coverage claim disposes of
   the "(server, CLI, agent runtime)" parenthetical; the escalation is
   recorded open in the task file; no spec was modified.

## Reviewed-and-accepted deviation

Tabs and AccordionItem under `components/ui/` keep hand-rolled WAI-ARIA
implementations instead of bits-ui Tabs/Accordion primitives, documented
in-component: the consumed contracts (caller-rendered tab panels wired via
fixed `tab-{id}`/`tabpanel-{id}` ids; standalone controlled accordion items
with caller-visible heading/body ids) cannot be satisfied by bits-ui's
content-registration model. The task's verification intent itself says
"bits-ui primitive behavior where applicable", and both components' styling
genuinely comes from the cva/Tailwind pipeline. Not a fake implementation.

## Generated-output observation (non-blocking; verifier-owned)

Committed `web/dist` is the assignment base's bundle (no Tailwind utilities
in its CSS). Per the review contract this is the deterministic verifier's
surface: it rebuilds remotely after integration and binds publication to the
tested tree. Not a candidate finding.

## Verifier must run (could not run in-sandbox within the build budget)

- Rebuild `web/dist` from the integrated tree; `SKIP_WEB_BUILD=1 cargo build
  -p gyre-server`; confirm the served UI is Tailwind-styled.
- Full functional e2e: `web/tests/e2e/app.spec.js` (needs the live server;
  requires a gyre-server build reserved to the verifier).
- Visual e2e: `web/tests/e2e/explorer-visual.spec.js` against the rebuilt
  dist (e2e specs and baselines are byte-identical to base; the visual spec's
  clipped regions — ExplorerView canvas/toolbar/tabPanel — do not import any
  migrated lib primitive, consistent with unchanged baselines and the prior
  round's recorded zero-diff `--update-snapshots` run).
- Standard Rust suite for the merge (zero Rust delta vs main, already
  CI-verified there).
