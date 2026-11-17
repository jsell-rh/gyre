---
title: "Apply operator decision for task-082: Amend toward a context-dependent collapsible sidebar on navigation-heavy screens"
spec_ref: "human-system-interface.md §1.3"
depends_on: []
progress: complete
commits: []
---

## Approved operator decision

{
  "id": "16871182b1370b61693fc691",
  "parent": "task-082",
  "question": "Task-082 still instructs building the permanent 6-item sidebar from the older HSI \u00a71.3 spec, but the newer canonical spec ui-navigation.md forbids a sidebar and the shipped UI no longer has one \u2014 should we retire the task, restore the sidebar, or amend the spec toward a context-dependent collapsible sidebar?",
  "context": "Gyre's navigation has been specified twice, in opposite directions. The older spec, specs/system/human-system-interface.md \u00a71.3 ('Stable Sidebar, Adaptive Content'), required a permanent, unchanging left sidebar with exactly six items (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin) on every screen. Task-082 implemented exactly that (commits d99ace7b, f332f774, 72bd5823, 9561ee86, 79323318; 101 tests) and is marked progress: complete. Later, specs/system/ui-navigation.md landed through 40 review rounds (PRs #588/#599) and its header explicitly supersedes HSI \u00a71: it replaces the sidebar with a two-mode model \u2014 a Workspace Home dashboard and a repo view with horizontal tabs \u2014 and its Design Principle 5 states 'No sidebar.' The nav rework (PRs #589\u2013#599) then deleted the task-082 sidebar code entirely (commit 08e5f91f removed Sidebar.svelte and 4 other navigation components, 1,843 lines) and added web/src/__tests__/NoSidebar.test.js to lock the no-sidebar shell in. docs/ui.md documents the two-mode model as canonical and states the HSI \u00a71.3 sidebar is 'not shipped.' The coverage matrix followed: specs/coverage/system/human-system-interface.md row for \u00a71.3 is now classified n/a \u2014 'Superseded by ui-navigation.md Principle 5 and \u00a79: no persistent sidebar' (navigation binding correction, 2026-10-09). The conflict: task-082's record still carries the full sidebar implementation plan with checked acceptance criteria and binds to that superseded section, so when the pipeline re-validates the assignment it fails \u2014 the task's literal instructions now contradict the canonical spec and target a coverage section that no longer demands anything. Blindly implementing the task would reintroduce a sidebar the newer spec forbids, regress the shipped two-mode navigation, and break NoSidebar.test.js and docs/ui.md. Blindly keeping only the new spec leaves task-082 permanently unresolvable as written, so the loop keeps re-opening a task whose deliverable was deliberately removed. Two history facts bound the choice: (1) the HSI-era sidebar coexisted with scope-based content routing that task-083 already rebinding to ui-navigation.md depends on, so restoring the sidebar also touches task-083's bindings; (2) external design guidance consulted for this decision (IBM Carbon's left-panel guidelines; Microsoft's navigation-basics guidance) treats the sidebar-versus-tabs choice as application-dependent, not universal \u2014 Carbon suggests an optional left panel where users switch destinations frequently. The operator has noted sympathy for a sidebar 'depending on context.' No spec-lifecycle amendment currently exists that resolves the HSI \u00a71.3 / ui-navigation.md contradiction in the spec text itself \u2014 only the coverage-matrix correction records the supersession.",
  "options": [
    {
      "id": "option_a",
      "title": "Retire task-082 as superseded; keep the shipped no-sidebar navigation",
      "summary": "Record a versioned decision that HSI \u00a71.3 is superseded by ui-navigation.md (formalizing the 2026-10-09 coverage correction through the spec lifecycle, per the meta-spec reconciliation process), close task-082 against its superseded binding instead of re-implementing it, and leave the two-mode UI exactly as shipped. The sidebar stays deleted; NoSidebar.test.js stays as the guard.",
      "tradeoffs": "Benefits: zero new UI work; keeps the navigation that is shipped, documented (docs/ui.md), tested (NoSidebar.test.js), and already verified by task-083's scoped-content-routing work; ends the re-opening loop cleanly; smallest reviewable change since it ratifies existing reality. Costs and risks: it dismisses the operator's stated suspicion that a sidebar 'might make some sense' in some contexts, and leaves the HSI spec text internally contradicting ui-navigation.md until the amendment lands (the amendment itself is the fix). It also means the six fixed destinations (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin) remain reachable only via the workspace-home dashboard, repo tabs, and keyboard shortcuts rather than one persistent control.",
      "next_step": "The loop will create a requirements-amendment task that amends specs/system/human-system-interface.md \u00a71.3 to mark it superseded by ui-navigation.md (with the versioned decision record), update task-082's record to closed-superseded with its historical commits preserved, and update the stale REGRESSED note in specs/coverage/system/ui-navigation.md row 3 to reflect the sidebar's removal. Code implementation does not resume; independent review must approve the amendment before merge."
    },
    {
      "id": "option_b",
      "title": "Restore the permanent 6-item sidebar; amend the newer spec first",
      "summary": "Amend ui-navigation.md via the spec lifecycle to drop Principle 5 ('No sidebar') and re-affirm HSI \u00a71.3 as the navigation contract, then re-land task-082's sidebar code (Sidebar.svelte and its tests survive in git history at d99ace7b) alongside the two-mode content model.",
      "tradeoffs": "Benefits: honors the muscle-memory argument in HSI \u00a71.3 (fixed positions for the six destinations on every screen); the implementation already exists and passed 101 tests once, so re-landing is cheaper than designing something new. Costs and risks: reverses a deliberate, heavily reviewed rework (40 review rounds on the spec, 11 PRs #589\u2013#599 on the code) whose stated motivation was that the six data-type-named buttons meant different things at different scopes; breaks NoSidebar.test.js, contradicts docs/ui.md, and disturbs task-083's bindings, which target the replacement model; the nav-rework coverage matrix (specs/coverage/system/ui-navigation.md) would need regressing back. Highest blast radius of the three options.",
      "next_step": "The loop will create a requirements-amendment task that rewrites ui-navigation.md Principle 5 and \u00a79 to re-admit the stable sidebar, reconciles the HSI \u00a71.3 text, and only after independent review and merge re-opens code work to restore Sidebar.svelte, its test suite, and the docs. No code changes happen before the amendment merges."
    },
    {
      "id": "option_c",
      "title": "Amend toward a context-dependent collapsible sidebar on navigation-heavy screens",
      "summary": "Amend ui-navigation.md via the spec lifecycle to add a third navigation pattern: an optional, collapsible, scope-labelled left panel on navigation-heavy screens (workspace home and repo mode), with stable item ordering within each mode, collapsed by default to preserve canvas/editor space, and a drawer on narrow viewports. Retire task-082's literal six-fixed-items binding; the amendment task would define the concrete item set per mode.",
      "tradeoffs": "Benefits: matches the operator's expressed preference for context-dependent navigation and the consulted design guidance (Carbon's optional left panel for frequent destination switching; Microsoft's position that the choice depends on the app) \u2014 though this is application-specific inference, not claimed industry consensus; preserves the shipped two-mode model rather than replacing it, so NoSidebar.test.js becomes 'no permanent sidebar' rather than wrong; keeps stable ordering within each mode, retaining most of HSI \u00a71.3's muscle-memory benefit without its fixed-global-menu cost. Costs and risks: neither current spec encodes this design, so it needs real design decisions (item set per mode, collapse default, interaction with repo tabs) and the most spec-writing of the three; it is the largest amount of eventual implementation work; and a hybrid risks being a compromise that satisfies neither the pure-muscle-memory nor the pure-two-mode intent if the item sets are not chosen carefully.",
      "next_step": "The loop will create a requirements-amendment task that drafts the collapsible-sidebar pattern into ui-navigation.md (defining per-mode item sets, collapse behavior, and narrow-viewport drawer behavior), records the versioned decision superseding both the literal task-082 binding and the blanket no-sidebar rule, and routes it through independent review. Implementation tasks are decomposed only after the amendment merges."
    }
  ],
  "recommendation": "option_c",
  "selection": {
    "option": "option_c",
    "actor": "jsell",
    "source": "Chat delegation; context-sensitive recommendation informed by Carbon and Microsoft guidance",
    "instruction": "Operator: whatever industry experts say, I suspect sidebar might make some sense...but may differ based on context. Recorded by the assistant as a delegated recommendation, not a direct cockpit selection. Prefer scope-labelled collapsible navigation where frequent switching helps, focused editor/canvas views, and a narrow-screen drawer. No claim of universal expert consensus.",
    "at": 1794847025.5504084
  },
  "sources": [
    "specs/system/ui-navigation.md",
    "specs/system/human-system-interface.md",
    "specs/coverage/system/human-system-interface.md",
    "specs/coverage/system/ui-navigation.md",
    "docs/ui.md",
    "specs/tasks/task-082.md",
    "specs/tasks/task-083.md"
  ]
}

Only option `option_c` is authorized. The other alternatives are recorded history, not requirements.

## Selected direction

Amend toward a context-dependent collapsible sidebar on navigation-heavy screens

Amend ui-navigation.md via the spec lifecycle to add a third navigation pattern: an optional, collapsible, scope-labelled left panel on navigation-heavy screens (workspace home and repo mode), with stable item ordering within each mode, collapsed by default to preserve canvas/editor space, and a drawer on narrow viewports. Retire task-082's literal six-fixed-items binding; the amendment task would define the concrete item set per mode.

The loop will create a requirements-amendment task that drafts the collapsible-sidebar pattern into ui-navigation.md (defining per-mode item sets, collapse behavior, and narrow-viewport drawer behavior), records the versioned decision superseding both the literal task-082 binding and the blanket no-sidebar rule, and routes it through independent review. Implementation tasks are decomposed only after the amendment merges.

## Required behavior

Prepare the task/spec amendment authorized by the recorded choice. This is a
requirements correction, not permission to guess at product behavior. Amend the
canonical spec, the parent task's requirements/spec binding, and documentation
so they express the selected direction consistently. Preserve historical intent
in clearly labelled history; never claim the newly selected behavior is already
implemented. Update specs/task-spec-inputs.json if a spec binding changes. Preserve registry
bindings still referenced by historical tasks, including this amendment task.

Do not change product code, tests, scripts, CI, exemptions or verification gates.
The amendment must pass independent review, remote verification and GitHub checks.
Once it merges, the controller will reassess the parent task against the new
requirements and implement any remaining gaps. Keep changes within specs/ and
docs/ (plus this task's own bookkeeping). Do not modify unrelated task contracts.

## Acceptance Criteria

- [ ] Canonical spec and parent task agree with the recorded choice.
- [ ] The parent task is no longer instructed to implement contradictory behavior.
- [ ] The choice, rationale and authority remain recorded in this task document.
- [ ] Existing implementation status is reported honestly; no fabricated coverage.
- [ ] No production code, tests or enforcement have been changed in this amendment.

## Shipped

Specs/docs-only requirements amendment applying operator decision
`16871182b1370b61693fc691` (option_c). No production code, tests, scripts,
CI, exemptions, or verification gates were touched — `git diff --stat` for
this change touches only `specs/` and `docs/ui.md`.

### What changed

- `specs/system/ui-navigation.md` — the canonical spec: Design Principle 5
  amended from the blanket "No sidebar" to "No permanent sidebar"; new
  §1a (Collapsible Navigation Rail, Optional) defines the third navigation
  pattern: where it appears (workspace home and repo mode only, never on
  focused editor/canvas surfaces), scope-labeling, the concrete per-mode
  item sets (workspace-home rail mirrors the §2 dashboard sections in
  canonical order plus Settings; repo-mode rail mirrors the §3 tab strip
  exactly), stable ordering within each mode, collapsed-by-default (48px
  icon-only, 240px expanded, preference persisted per mode),
  never-load-bearing rule, and narrow-viewport drawer behavior (§8
  hamburger drawer). §8 Mobile, §9 replacement tables, and the Relationship
  table updated consistently; amendment + full decision record added to the
  header and §1a.
- `specs/system/human-system-interface.md` — §1.3 now carries the
  supersession record (decision id, date, pointer to `ui-navigation.md` §1a)
  with the original text preserved under a clearly labelled
  "Historical text (superseded)" block; header notes the §1 supersession.
- `specs/system/ui-layout.md` — §1 Application Shell carries a supersession
  note pointing at `ui-navigation.md` §1/§1a so the old six-item sidebar
  shell is not implementable from this spec.
- `docs/ui.md` — nav-rework note and Shell Layout section state the amended
  canonical requirement and report implementation status honestly: the rail
  is **specified, not shipped**; the shipped UI remains the two-mode
  no-sidebar model guarded by `NoSidebar.test.js`.
- `specs/tasks/task-082.md` — the parent task's literal six-fixed-items
  binding is retired: `spec_ref` retargeted to `ui-navigation.md §1a`,
  progress honestly reset to `not-started` against the new requirement,
  superseding plan/acceptance criteria preserved verbatim under a
  "Historical record (SUPERSEDED)" heading, new unchecked acceptance
  criteria added for the retargeted requirement, historical `commits:`
  preserved.
- `specs/task-spec-inputs.json` — new section entry
  `ui-navigation.md#…/1a-collapsible-navigation-rail-optional` and binding
  `"ui-navigation.md §1a"`; the existing
  `"human-system-interface.md §1.3"` binding is preserved (still referenced
  by this task's `spec_ref`).

### Verification evidence

- `python3 -c` probe of `scripts/pipeline/requirements.py::snapshot` for
  both `specs/tasks/task-082.md` and `specs/tasks/task-246.md` (after edit):
  zero errors, every binding resolves to a unique heading in the amended
  specs (recorded under `/tmp/stage/review-evidence`).
- `python3 -m json.tool specs/task-spec-inputs.json` — valid JSON.
- `git diff --name-only` — only the eight authorized amendment paths
  touched (ui-journeys.md needed no change: it already carries a DEPRECATED
  banner and is not implicated by the decision).

### Deferred to the controller

- Coverage-matrix rows for the new `ui-navigation.md` §1a section
  (`specs/coverage/` was outside this task's authorized paths).
- Decomposition and implementation of the rail itself, plus any
  `NoSidebar.test.js` assertion refinement, after this amendment merges.
