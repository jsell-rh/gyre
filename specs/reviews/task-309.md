# Review — task-309 (shadcn-svelte UI stack adoption)

Spec: `specs/development/architecture.md` §Tech Stack — "Svelte 5 + shadcn-svelte
for the web UI" (shadcn-svelte half only; the Rust-agent-runtime half stays an
open controller escalation).
Candidate under review: `5338b5c038f2677a25e88d25fca8ee0e9b3bb240` (branch
`pipeline/task-309/51353eabee634d84901616aac0f0818f-1`, base
`973068a86ab3154b6cba448fda9369125326cab7`; product commit `4d5e209b` plus
merge-repair/CI-repair rounds up to `5338b5c0`).
Verdict: **approved** — no findings. All acceptance criteria verified with
independent probes (evidence: `/tmp/stage/review-evidence/task-309/verification.md`).
One non-blocking generated-output observation for the verifier (stale committed
`web/dist`, see end).

## Probes run (checkout at candidate `5338b5c0`; working tree carries only the
stage's protected-task-text restoration of `specs/tasks/task-309.md`, not part
of the product diff)

- `cd web && npm ci` (locked versions) → 205 packages.
- `cd web && npm test` → **exit 0**: 128 files, **2603 passed / 41 skipped** —
  the existing suites are unchanged by the diff (only
  `web/src/__tests__/ShadcnUiStack.test.js` is new), so they exercise the
  migrated shadcn-svelte components.
- `npm run build` → exit 0; emitted CSS contains the generated Tailwind
  utilities (`.bg-primary{`, `.text-primary-foreground{`, `.animate-shimmer{`)
  — the build output is styled by the Tailwind pipeline, not dead config.
- **Corroborating probe**: temporarily removed the `import './app.css'` from
  `main.js` and rebuilt → the emitted CSS contains **zero** Tailwind utility
  definitions (the ui components' classes go unstyled) — the UI genuinely
  depends on the Tailwind entry stylesheet. Restored; tree clean.
- **Visual e2e** (repo playwright + committed chromium headless shell, static
  dist server serving a fresh `vite build` of the current source):
  `gyre-playwright-visual.config.js` → **16/16 passed** against the COMMITTED
  baselines — the restyle is pixel-identical, so no baseline regeneration is
  required (the task allows regeneration "deliberately where the restyle
  changes pixels"; it doesn't). Functional assertions unchanged (no
  `web/tests/` changes in the diff).
- **Probe non-vacuity (mutation)**: swapped `web/src/lib/Button.svelte` back to
  the base bespoke version → `ShadcnUiStack.test.js` fails its two Button
  probes (cva classes / bits-ui marker absent) — the probe detects exactly the
  defect class the verification intent names (bolted-on config while bespoke
  CSS still styles). Restored; tree clean.
- **SQLite CI repair** (`b9bcaf5e`, in-diff): both new gyre-adapters regression
  guards pass (`pooled_connections_have_wal_foreign_keys_and_busy_timeout`,
  `concurrent_writers_wait_on_busy_timeout_instead_of_failing`); mutation check
  reverting `SqliteCustomizer` to the batched single-string PRAGMA form makes
  BOTH fail (`busy_timeout must be 5000ms`) — real guards, real fix (diesel's
  `sqlite3_prepare_v2` compiles only the first statement of a multi-statement
  string). File restored byte-identical.

## Verified by inspection

- **Foundations**: `web/package.json` declares tailwindcss ^4 + `@tailwindcss/vite`
  (in the Vite plugin chain), bits-ui, clsx, tailwind-merge,
  class-variance-authority, tw-animate-css; `web/components.json` follows the
  shadcn-svelte generation convention; components generated under
  `web/src/lib/components/ui/{button,badge,card,table,tabs,dialog,accordion,
  toast,skeleton}/`; `cn()` in `web/src/lib/utils.js`; `web/src/app.css` is the
  Tailwind entry imported first by `main.js`.
- **Migration with contracts intact**: `web/src/lib/*.svelte` are facades over
  the shadcn-svelte components; Button and Modal/Dialog sit on bits-ui
  primitives (`ButtonPrimitive.Root`; Dialog Root/Overlay/Content/Title/Close
  with focus trap/restore, Escape, scroll lock — the full former a11y
  contract); Badge/Card/Table/Tabs/Toast/Skeleton/AccordionItem are
  cva-variant + Tailwind-utility components. No scoped `<style>` blocks remain
  in the lib primitives (AccordionItem keeps a documented intentionally-empty
  block for the AccordionScoping CSS-AST probe). Tabs/AccordionItem keep their
  WAI-ARIA manual implementations with documented rationale (consumed
  contracts — caller-rendered panels, caller-visible ids — don't fit bits-ui's
  content-registration model); their behavior is pinned by the unchanged
  existing tests, which pass.
- **Brand preserved as theme tokens**: `app.css` mirrors design-system.css
  exactly (#ee0000 primary, #151515/#1f1f1f/#292929 surfaces, #383838/#4d4d4d
  borders, status colors, 4px radius scale, Red Hat font stacks;
  `--destructive-hover: #c04417` equals the former `color-mix(80% #f0561d,
  black)`). design-system.css is retained as `@layer base` for view-level
  element rules/legacy DOM hooks; the primitives' styling no longer comes from
  it. Remaining `.btn-primary` scoped rules in App.svelte/views are
  pre-existing view-level styles for hand-rolled buttons (App.svelte untouched
  by the diff) — outside this task's primitives scope, and unchanged.
- **Escalation kept open**: committed `specs/tasks/task-309.md` carries the
  "Open controller escalation (Rust half — NOT closed by this task)" section;
  the diff touches no spec, no coverage file, and asserts no reading of the
  "(server, CLI, agent runtime)" parenthetical.
- **Coordination**: the web diff touches only `web/src/lib/*` primitives,
  configs, and the new probe test — no `web/src/components/*` or view
  behavioral changes (no behavioral contract overlap with task-285/294/296/307
  or the ui-layout plan).

## Generated-output observation (NON-BLOCKING — for the verifier)

The committed `web/dist` at this candidate is the pre-migration bundle (its
CSS has 36 `.btn-primary` rules and zero Tailwind utilities; a fresh build
yields different asset hashes). The shipped-note sentence claiming a fresh
`vite build` reproduces the committed dist hashes is inaccurate at this head
(it held on an earlier merged state; the final merge took main's dist). Per the
stage rules `web/dist` is verifier-owned: the verifier must rebuild it from
this source and commit the tested assets — until then `SKIP_WEB_BUILD=1`
builds serve the pre-migration bundle. Source defects: none.

## Verifier must run (deferred per stage rules)

- `cargo nextest run -p gyre-server --test scim_groups` (the repaired CI test)
  and the full gyre-server suite.
- Full functional playwright e2e (`cd web && npx playwright test`, needs a
  running gyre-server) — the in-sandbox visual suite is done above.
- web/dist rebuild + commit; workspace gates (arch lint, clippy, fmt),
  check-* scripts, GitHub CI on the exact head.
