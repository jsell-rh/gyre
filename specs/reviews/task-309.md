# Review — task-309 (shadcn-svelte UI stack)

Candidate: `b3ca22b20bbc89d6e74081b5fd78f684ba764bae` (base `9cdd4b588524a23127b0250b349bb3f5e5789744`)
Verdict: **approved** (independent probe evidence in
`/tmp/stage/review-evidence/task-309/verification.md`).

## What was verified independently

- **Foundations genuinely used, not bolted on.** `npm ci` installs the
  locked stack (tailwindcss 4.3.5, bits-ui 2.22.0, clsx, tailwind-merge,
  class-variance-authority, tw-animate-css). A fresh `vite build` of the
  candidate emits CSS headed by the `tailwindcss v4.3.5` banner containing
  `.bg-primary{background-color:var(--primary)}`, `--primary:#e00` (the Red
  Hat brand value), the 4px brand radius scale, and the shimmer animation.
- **Corroborating probe (verification intent).** With `import './app.css'`
  removed from `main.js`, the rebuild contains zero Tailwind utilities and no
  Tailwind banner (CSS drops 767,370 → 738,862 bytes): the UI genuinely
  depends on the Tailwind pipeline. `main.js` restored afterwards.
- **Component migration is real.** The primitives in `web/src/lib/*.svelte`
  are facades over ui components in `web/src/lib/components/ui/` — cva
  variant configs styled by Tailwind utilities, with bits-ui primitives
  under Button (`Button.Root`) and Modal (Dialog Root/Overlay/Content/Title/
  Close: focus trap, restore, Escape, scroll lock). `design-system.css`
  defines no `.btn`/`.card`/`.badge`/`.modal` component classes anymore and
  is wrapped in `@layer base` beneath the utilities layer. The Red Hat
  palette is carried as the shadcn theme tokens in `web/src/app.css`.
  Tabs/AccordionItem keep WAI-ARIA manual-activation implementations with a
  documented contract rationale (caller-rendered panels with
  `tab-{id}`/`tabpanel-{id}` wiring do not fit bits-ui's
  content-registration model) — accepted, since the task requires the same
  consumed accessibility contracts.
- **Tests pass without inflation.** `cd web && npm test`: 128 files, 2603
  passed, 41 skipped, 0 failed. The candidate modifies no existing test
  file; the new `ShadcnUiStack.test.js` probes fail if Button reverts to
  the bespoke `btn btn-{variant}` class layer (they require the bits-ui
  marker and cva utility classes).
- **Visual e2e.** Recreated the in-sandbox static-server harness and ran
  the 16 explorer visual tests against a fresh Tailwind build of the
  candidate: 16/16 passed with the existing committed baselines — the
  restyle is pixel-identical, so no baseline regeneration is even needed.
- **Scope discipline.** The Rust tree is byte-identical to the assignment
  base (no Rust delta rode along in the merge repair). The Rust-half
  canonical conflict ("server, CLI, agent runtime" vs. the Node.js agent
  image mandated by `specs/system/agent-runtime.md`) is recorded as an open
  controller escalation in the task file; no code or coverage claim
  disposes of it.

## Recorded separately (generated output, verifier-owned — not blocking)

The committed `web/dist` in the candidate is the assignment base's bundle
(no Tailwind marker): the executor restored the merge-conflicted generated
assets to base, and the deterministic verifier rebuilds `web/dist` from
this merged source, runs all checks, and binds publication to that tested
tree. The fresh build of this exact source proves the rebuilt dist will be
Tailwind-styled.

## Verifier-must-run

- `cd web && npm ci && npm test` (re-run at integration).
- `cd web && npx playwright test` — full functional e2e (boots gyre-server;
  too large for the 4 GiB review sandbox).
- Workspace gates (`cargo test --all`, clippy, fmt, arch lint).
- Rebuild `web/dist` from the merged source and confirm the published
  bundle carries the Tailwind pipeline output before publication.
