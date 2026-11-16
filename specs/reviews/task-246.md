# Review — task-246 (Apply operator decision for task-082: collapsible navigation rail amendment)

Assignment: operator decision `16871182b1370b61693fc691` (option_c only authorized).
Candidate: `73e739ef` (base `047d6524`); commits `5c749b1f` (amendment), `f711e6ba`/`73e739ef` (attribution bookkeeping).
Verdict: **approved** — faithful specs/docs-only amendment; all five acceptance criteria met; no blocking findings.

## Probes run (checkout at candidate `73e739ef`)

- `git diff --name-only 047d6524..73e739ef` → exactly seven files: `docs/ui.md`, `specs/system/{human-system-interface,ui-layout,ui-navigation}.md`, `specs/task-spec-inputs.json`, `specs/tasks/task-082.md`, `specs/tasks/task-246.md`. No `crates/`, `web/`, `scripts/`, CI, exemption, or coverage files touched.
- `python3` probe of `scripts/pipeline/requirements.py::snapshot` for **every** `specs/tasks/task-*.md` (235+ tasks) → **zero binding errors** across the whole task set; the spec edits broke no other task's bindings (incl. task-083, which binds to ui-navigation.md sections).
- Registry probes: `specs/task-spec-inputs.json` parses as valid JSON; the new section entry `ui-navigation.md#…/1a-collapsible-navigation-rail-optional` resolves to exactly **one** heading path (`UI Navigation Model / 1a. Collapsible Navigation Rail (Optional)`); the preserved `human-system-interface.md §1.3` binding (task-246's own `spec_ref`) resolves to exactly one heading.
- Internal-consistency probes of the amended spec: §1a workspace-home rail item set (Decisions, Specs, Architecture, Repos, Briefing, Agent Rules, Settings) matches the §2 "Sections" heading order verbatim plus Workspace Settings; repo-mode rail is defined to mirror the §3 tab strip (`[ Specs ][ Architecture ][ Decisions ][ Code ][ ⚙ ]`) with the strip as declared source of truth.
- `bash scripts/check-no-em-dash.sh <touched md files>` → exit 0 (CI's invocation is crates-only and advisory anyway; the touched files already carried em-dashes at base).
- Attribution check scope analysis (`scripts/check-task-commit-attribution.sh`): all three candidate commits touch only `specs/` + `docs/` — outside the check's product-surface scope (`crates/`, `web/src`, `web/tests`) — so task-246's final `commits: []` frontmatter is compliant. (The full-history script run timed out in this sandbox; the scope rule was verified by reading the script and per-commit `--name-only` output. The verifier's full run should confirm, but per the script's own product-surface filter these commits cannot be flagged.)
- `specs/system/ui-journeys.md` confirmed to already carry the DEPRECATED banner cited as the reason for leaving it untouched.

## Verified (acceptance criteria)

- **AC1 — Canonical spec and parent task agree with the recorded choice.** `ui-navigation.md` Design Principle 5 amended from "No sidebar" to "No permanent sidebar" (with amendment provenance); new §1a defines exactly the option_c pattern: optional, collapsible, scope-labelled left panel on navigation-heavy screens (workspace home + repo mode only, never on focused editor/canvas surfaces), stable per-mode ordering, collapsed-by-default (48px icon-only / 240px expanded, persisted per user per mode), never-load-bearing, narrow-viewport drawer. §8, §9 replacement tables, and the Relationship table updated consistently; full decision record embedded in §1a. task-082 retargets to `ui-navigation.md §1a` with matching new acceptance criteria.
- **AC2 — Parent task no longer instructs contradictory behavior.** task-082's six-fixed-items binding retired; the old implementation plan and checked criteria are preserved verbatim under "Historical record (SUPERSEDED)" with explicit "do not execute" labels; the old agent instructions carry a retarget note pointing at §1a.
- **AC3 — Choice, rationale, authority recorded.** task-246 embeds the complete decision JSON (id, question, all three options, selection by jsell, operator instruction, sources) verbatim; §1a and HSI §1.3 both cite the decision id and date.
- **AC4 — Honest status, no fabricated coverage.** Every amended surface states the rail is "specified, not yet shipped": §1a header note, docs/ui.md (both the nav-rework note and Shell Layout), task-082 ("Progress is `not-started` against the retargeted requirement"). task-082's `progress:` honestly reset from `complete` to `not-started`. Historical `commits:` preserved and labelled as the superseded deliverable. Nothing claims the rail exists.
- **AC5 — No production code/tests/enforcement changed.** Confirmed by the diff file list; HSI §1.3 and ui-layout §1 preserve their historical text under clearly labelled supersession blocks rather than deleting it; registry bindings still referenced by historical tasks (incl. task-246's own `human-system-interface.md §1.3`) preserved.

## Non-blocking observations (for the controller / follow-up rounds)

- **§8/§1a drawer item-set wording.** §1a says the narrow-viewport drawer offers the rail's per-mode item set (7 items on workspace home, incl. Architecture + Settings), while §8's pre-existing parenthetical lists five sections (Decisions, Specs, Repos, Briefing, Agent Rules). §8's list was already an imprecise summary at base (it omits Architecture, which §2 marks "optional, expandable"), and the amendment's §8 bullet explicitly defers to §1a, so §1a governs; but the implementing task should reconcile §8's parenthetical (and note `NoSidebar.test.js`'s "five workspace-home sections" case) when the rail lands.
- **Coverage matrix.** No row exists yet for §1a in `specs/coverage/system/ui-navigation.md`, and row 3's REGRESSED note ("No spec-lifecycle amendment exists") is now stale given this amendment. The executor's justification ("specs/coverage/ was outside this task's authorized paths") is imprecise — `specs/coverage/` is inside `specs/` — but the deferral itself matches the task's structure (controller reassesses the parent task after merge; the verifier regenerates matrix summaries). The controller should add the §1a row and refresh row 3's note when decomposing implementation.
- **Test docstring drift (deferred by design).** `web/src/__tests__/NoSidebar.test.js`'s header still quotes the former "No sidebar." Principle 5 wording, and docs/ui.md's "asserts the absence of a *permanent* sidebar" is interpretive framing (the test asserts no `.sidebar` element at all — vacuously implying no permanent one). Both are acknowledged deferrals in task-082's new acceptance criteria ("updated, not deleted"), not fabricated coverage.

## Test-inflation check

Not applicable — specs/docs-only amendment; no tests were added or changed (verified by the diff file list), which is correct per the task's "do not change tests" constraint.

## Verifier follow-ups (remote)

- Full `bash scripts/check-task-commit-attribution.sh` run (needs full history; the per-commit scope analysis above predicts a pass).
- Standard CI suite on the merged tree; nothing in this amendment touches compiled surfaces.
