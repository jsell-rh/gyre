# Review — task-090 (HSI Explorer Three Lenses: Structural, Evaluative, Observable)

Spec: `specs/system/human-system-interface.md` §3 "Three Lenses" (coverage
section `human-system-interface.md §3 Three Lenses`).
Assigned base: `2100127c4` · Assigned candidate: `8adb4b914`.
Verdict: **needs-revision** — the candidate does not compile.

## Round 1 (this review)

### Probes (evidence under /tmp/stage/review-evidence)

- `npm ci` in `web/` → exit 0 (`npm-ci.exit`, `npm-ci.log`).
- `npx vitest run src/__tests__/ExplorerCanvas.test.js` → **exit 1**,
  suite-level `CompileError: web/src/lib/ExplorerCanvas.svelte:20:0
  Unexpected token` — the parse error is on the literal `<<<<<<< HEAD`
  marker line; 0 tests ran (`vitest-ExplorerCanvas.log/.exit`).
- `npx vitest run src/__tests__/ExplorerViewLensUrl.test.js` → **exit 1**,
  `CompileError: web/src/components/ExplorerView.svelte:7:0 Unexpected
  token`, 0 tests ran (`vitest-ExplorerViewLensUrl.log/.exit`).
- `grep -n '^<<<<<<<'` on the two production files → 7 conflict blocks
  (`conflict-markers.txt`): `ExplorerCanvas.svelte` lines 20, 3911, 5591;
  `ExplorerView.svelte` lines 7, 950, 970, 1041.

### Finding

- [ ] **F1 (fatal): the candidate is a merge commit with unresolved merge
  conflict markers committed in two production files — the web build and
  every Explorer test suite are dead at this SHA.** The assigned candidate
  `8adb4b914` ("implement(task-090): pipeline checkpoint") merges the
  assigned base `2100127c4` into the task branch, but the two files that
  both branches touched — `web/src/lib/ExplorerCanvas.svelte` and
  `web/src/components/ExplorerView.svelte` — were committed with their
  conflict markers intact instead of resolved (this is exactly the rebase
  failure recorded in the durable finding: the `74f0875c6` rebase of the
  base hit CONFLICT in these two files; the checkpoint then committed the
  conflicted tree). Concretely, seven blocks:

  - `ExplorerCanvas.svelte:20` — imports: task-090's
    `lens-rendering.js` imports vs. the base's (task-086) `liveness.svelte.js`
    imports;
  - `ExplorerCanvas.svelte:3911` — evaluative edge coloring: task-090's
    spec-path gate coloring vs. task-086's node-gate-status coloring;
  - `ExplorerCanvas.svelte:5591` — evaluative legend entries vs. task-086's
    gate legend entries;
  - `ExplorerView.svelte:7` — imports; `:950` — canvas-load state resets
    and best-effort data loads; `:970` — which loader is invoked;
    `:1041` — task-090's `loadEvaluativeData` vs. task-086's
    `loadGateData`/`loadLivenessTimeline`.

  Reproduction: `cd web && npx vitest run src/__tests__/ExplorerCanvas.test.js`
  → CompileError at `ExplorerCanvas.svelte:20:0`, exit 1. The acceptance
  criterion "`npm test` passes in `web/`" is therefore unmet, and none of
  the shipped lens behavior (coverage badges, gate rings/edges, approval
  icons, URL persistence) can be exercised at this candidate.

  The shipped record's focused-probe claims (ExplorerViewLensUrl 6/6,
  combined suites 192/192 on "the current head") cannot refer to this
  candidate SHA — every suite importing either component fails to compile —
  so that evidence is stale relative to the assigned candidate and no
  acceptance criterion is currently proven.

  Remedy: resolve the seven conflict blocks by combining both sides — keep
  both import sets, both state variables, both loaders, and merge the edge
  coloring and legend entries (task-090's spec-path gate coloring and
  task-086's liveness features are not mutually exclusive) — then re-run
  the full `web/` vitest suite and the focused lens suites on the resolved
  candidate. The prior product commits (`0766a1679`, `7bb23bca6`,
  `f51574568`) were internally clean; only the final checkpoint merge is
  broken, so the repair is a resolution + re-checkpoint, not a rewrite.

No assessment of the lens implementation semantics beyond compilability is
possible at this candidate; the review of the substantive HSI §3 behavior
(gate ring/edge colors, approval icon resolution, URL persistence, no-
re-layout on lens switch) must be redone on the resolved candidate because
the shipped test evidence predates the fatal merge.
