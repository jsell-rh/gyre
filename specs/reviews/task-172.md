# Review — task-172 (Editor Split layout component for spec and meta-spec editing)

Spec: `specs/system/ui-layout.md` §2 Editor Split (+ §9 Meta-specs Preview Loop per the task instructions).
Candidate under review: `493f6ab2611b3c05128634eb8fdb703841f246d4` (base `2427ccc1`). This is the round after the 70971f1e review and its repair commit `a44ba473`.
Verdict: **needs-revision** (2 open code findings, 1 test finding — all durable from review 8fdde267 and never repaired).

## Probes (evidence under /tmp/stage/review-evidence/, README.md)

- Shipped suite claim: `npm ci` then vitest over EditorSplit + DetailPanel + SpecConflictDialog tests → **72/72 passed** — claim verified.
- Dist: fresh `vite build` to a temp outDir is `diff -r`-identical to the committed `web/dist`; no conflict markers; no `DEBUG EditorSplit` in src or dist; `__debug-esc.test.js` gone — all repair-round claims verified.
- thorough-preview 404 (production path for every spec-context Preview — the endpoint has no server route): returns to editing with an error banner, no fabricated §9 State 3 → prior finding 1 **fixed**.
- Real `PreviewResponse`/`PreviewStatusResponse` shapes (meta_specs.rs, incl. `preview_id` in the status body): meta-impact renders scope/affected-spec-count/affected-repos, Code Diff tab lists per-spec statuses → prior finding 4 **fixed**.
- Esc double-fire: one bubbling Escape from the editor textarea fires DetailPanel's div handler (`onback` spy called once) AND EditorSplit's window handler → prior finding 2 **open**.
- Attribution gate `scripts/check-task-commit-attribution.sh` → exit 0.

## Findings

- [-] **F1 (major): Esc double-fire violates the AC "Back/Esc returns to normal view"** (durable finding 2 from review 8fdde267, unrepaired). `DetailPanel.svelte:221-229` handles Escape on the panel div with no `showEditorSplit` guard (`if (onback) onback(); else close();`), while EditorSplit's `<svelte:window>` handler closes the split. With focus inside the panel — the normal case, the user was just editing the textarea — one Escape reaches both: the split closes AND the panel pops detail history / `window.history.back()` (both production call sites pass `onback`, App.svelte:1624/1660). Probe: `esc-double-fire-probe-result.json` (`onback_calls: 1`; expected 0), probe source `__probe-esc.test.js`. The candidate's diff modified EditorSplit's `handleKeydown` (specConflict/onClose guards) without fixing propagation. Note the div handler fires before the window handler, so the fix belongs in DetailPanel (`if (showEditorSplit) return;`) or an equivalent shared guard — stopPropagation in EditorSplit's window handler cannot help.
- [-] **F2 (major): meta-spec context unreachable from production UI** (durable finding 5, unrepaired). Nothing mounts EditorSplit with `context="meta-spec"`/`targetSpecs`/`onpublish`; `MetaSpecs.svelte` (unchanged in this diff) keeps its own parallel §9 selector/previewPersona implementation. The task instruction "used by BOTH spec editing and meta-spec editing" is met only at the component level — the §9 half is test-only code and the workspace carries two divergent §9 client implementations. The coverage row now says "built and tested on the component", but the task Shipped section still does not disclose the unreachability and no spec amendment dispositions the divergence.
- [-] **F3 (test): the suite still never exercises the production paths** (durable finding 8, unrepaired). (a) No test for `thoroughPreview` rejecting with the 404 every real deployment produces — the (now-correct) failPreview behavior is unpinned; (b) no test feeds `previewPersona`/`previewPersonaStatus` the real server shapes — existing meta-spec tests mock synthetic `architecture_diff`/`specs_diff` payloads the server never sends; (c) both shipped Esc tests fire on `window`, which cannot reach the panel-div handler, so the F1 double-fire is invisible to CI.

## Verified fixed since prior rounds

Prior review 8fdde267 findings 1 (fabricated Preview Complete on failure/404/timeout — now `failPreview` returns to editing with an error banner), 3 (conflict-dialog Esc teardown — `specConflict` guard + dialog open-guard), 4 (real response shape), 6 (fake diff highlighting in the locked editor — now verbatim), 7 (toolbar — TOOLBAR_BUTTONS with localized aria-labels, pinned by test). Prior review 70971f1e findings (Esc regression, stale dist, debug leftovers, assertionless test, dead i18n keys, false Shipped claims) all verified fixed at this head.
