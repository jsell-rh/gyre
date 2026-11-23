---
title: "specs/index.md completeness and mechanical cache-hit enforcement: every spec reachable from the entry point, validated in CI and pre-commit"
spec_ref: "Spec gap spec-8f4edcf188b12e2a350050e2 / spec-index-cache-hits / fd38a2fca0f8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-experience.md §Spec Organization & Cache Hit Optimization: "All specs live in specs/ with a specs/index.md entry point" and specs should be indexed to "maximize cache hits - clear naming, cross-linking, predictable paths." The structure is real at base 87377fe1 (kebab-case topic directories, pervasive cross-linking), but the hand-maintained index is already incomplete — active canonical specs specs/system/ui-navigation.md, specs/system/repo-lifecycle.md, and specs/system/ralph-loop.md exist on disk with no row in specs/index.md — and nothing validates index completeness or cross-link integrity (no script in scripts/ checks that specs are reachable from the index). Deliver: (1) Complete specs/index.md: add rows in the existing table format with accurate summaries for every spec file under specs/ that lacks one, with the full missing set mechanically derived by scanning the tree (not just the three known misses); superseded and reference-only material (prior-art/) keeps the classification treatment the index already applies. (2) scripts/check-spec-index.sh — real, blocking validation with two arms: (a) completeness — every specs/**/*.md file has a row in specs/index.md (honoring the index's own documented classification for reference-only material); (b) integrity — every relative file link in specs/index.md resolves to an existing file. Violations print the file and the missing row or dead link and exit 1; the completed tree exits 0; no exemption entries in any frozen scripts/*-exemptions.txt. (3) Wire the script as a mandatory CI step in .github/workflows/ci.yml following the arch-lint job's conventions (never advisory, no `|| true`; additions only — coordinate with the ci-docs-release and architecture plans that also add ci.yml steps) and into .pre-commit-config.yaml per the existing hook conventions. The runtime GET /api/v1/specs/index manifest-ledger surface and manifest registration policy are out of scope (separate generated surface; do not change find_unregistered_specs behavior).

## Acceptance Criteria

- [ ] scripts/check-spec-index.sh exits 0 on the completed tree.
- [ ] Adding a new file specs/system/probe-spec.md with no index row makes the script exit 1 naming it; removing a file referenced by an index row makes it exit 1 naming the dead link.
- [ ] specs/index.md contains rows for specs/system/ui-navigation.md, specs/system/repo-lifecycle.md, and specs/system/ralph-loop.md with accurate summaries, and every other on-disk spec is likewise listed (mechanically derivable, not hand-audited).
- [ ] ci.yml runs the check as a mandatory step and the pre-commit wiring follows the existing hook pattern.
- [ ] No new entries in any frozen scripts/*-exemptions.txt.

## Verification intent

Run the completeness check against the current tree — on current code no such script exists, and the tree itself fails the requirement (grep of specs/index.md for ui-navigation, repo-lifecycle, and system/ralph-loop returns nothing, so an agent whose first lookup goes through the specced entry point for UI navigation or repo lifecycle misses and must fall back to search — exactly the cache miss the section says to avoid); after the task the script must exit 1 on that exact unmodified-tree condition until the rows are added, then 0. Integrity arm: delete the file behind any index link and run the script — it must exit 1 naming the dead link, proving it validates cross-link integrity against the filesystem rather than asserting that specs/index.md exists.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-experience.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`.

[
  {
    "id": "fff96bf529e90ad943501101",
    "decision": "gap",
    "requirement_quote": "- All specs live in `specs/` with a `specs/index.md` entry point.\n- Think about spec structure in terms of **cache hits vs. misses**: if an agent tool call (search, read, etc.) finds what it needs on the first try, that's a cache hit. If it fails and requires additional lookups, that's a cache miss.\n- Specs should be structured and indexed to **maximize cache hits** - clear naming, cross-linking, predictable paths.",
    "detail": "Structurally strong but the entry point does not actually cover all specs, and nothing keeps it current. Realized: all specs live under specs/ in consistently kebab-case-named topic directories (system/, development/, personas/, milestones/, prior-art/), specs/index.md exists as the entry point with topic tables, cross-links and summaries, and pervasive cross-linking between specs (e.g. AGENTS.md's Key Architecture Specs table). Missing: the index is hand-maintained and already incomplete — active canonical specs specs/system/ui-navigation.md and specs/system/repo-lifecycle.md (and specs/system/ralph-loop.md) exist on disk but have no row in specs/index.md (grep of index.md for these filenames returns nothing; the only ralph row is the different, superseded development/ralph-loops.md). An agent whose first lookup goes through the specced entry point for UI navigation or repo lifecycle misses and must fall back to search — exactly the cache miss the section defines and says to avoid. Nothing mechanically validates index completeness or cross-link integrity: no script in scripts/ checks that every specs/**/*.md is reachable from specs/index.md. The runtime GET /api/v1/specs/index is a separate auto-generated surface built from the manifest ledger, not a validation of specs/index.md, and manifest registration enforcement (find_unregistered_specs, used to reject pushes adding unregistered specs) is policy-gated per repo and explicitly excludes index.md.",
    "evidence": [
      {
        "file": "specs/index.md",
        "quote": "This is the entry point for all Gyre specs. Read this first.",
        "detail": "The specced entry point exists, organized into topic tables (System / Development / Prior Art / Personas / Milestones / Open Questions) with cross-links and one-line summaries, and naming across specs/ is consistent and descriptive — the structure/naming/cross-linking practice the section asks for is genuinely present."
      },
      {
        "file": "specs/system/ui-navigation.md",
        "quote": "# UI Navigation Model",
        "detail": "This active canonical spec (it supersedes human-system-interface.md §1 and is linked from AGENTS.md's Key Architecture Specs table) has no row in specs/index.md — grep of index.md for 'ui-navigation' returns nothing. The same holds for specs/system/repo-lifecycle.md ('# Repo Lifecycle', defines repository CRUD lifecycle) and specs/system/ralph-loop.md. These are first-lookup cache misses through the specced entry point, the precise failure mode the section says to design against."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "// GET /api/v1/specs/index — auto-generated markdown index",
        "detail": "The only mechanically generated spec index is this runtime API handler, built from the manifest-driven ledger — a different surface from the repo's specs/index.md. Inspected scripts/ for any index-completeness or cross-link validation (orphan/reachable/link-check): none exists, and manifest-based registration enforcement (spec_registry.rs find_unregistered_specs, 'Excludes `index.md`, `prior-art/`, `milestones/`') is policy-gated per repo and does not validate index.md rows, so nothing keeps the hand-maintained entry point complete."
      }
    ]
  }
]
