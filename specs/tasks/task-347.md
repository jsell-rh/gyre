---
title: "Repository as system of record: in-repo quality grades, CI-enforced CLI/doc parity, and a recurring doc-gardening agent that opens fix-up work"
spec_ref: "Spec gap spec-8f4edcf188b12e2a350050e2 / doc-garden-quality-grades / fd38a2fca0f8"
depends_on: [task-346]
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-experience.md §Repository as System of Record: "Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo" and "Mechanical enforcement that docs stay current: linters, CI, doc-gardening agents." The artifact half is real at base 87377fe1 (AGENTS.md is the specced map-not-manual entry point; specs/, specs/tasks/, architecture docs and coverage matrices are versioned in-repo); three obligations are missing. Deliver: (1) Quality grades: a production surface that computes and versions quality grades in-repo — at merge completion the merge path computes a deterministic quality grade for the merged work from real recorded outcomes (gate results, review outcomes, verification status — data the MR record and gate executor already hold) and records it as a versioned in-repo artifact committed on the default branch (e.g. a structured per-repo quality ledger at a predictable path, appended/updated by the merge), retrievable via API and present in the repo tree; the grade is a function of the recorded outcomes — differing inputs yield differing grades, never a constant standing in for a computed value. (2) Mechanical docs-current enforcement in CI: scripts/check-cli-spec-parity.sh's header claims "Run during pre-commit and CI" but it appears only in .pre-commit-config.yaml and nowhere in .github/workflows/ci.yml — wire it into ci.yml as a mandatory step (never advisory, no `|| true`; additions only — coordinate with the ci-docs-release plan's check-doc-freshness.sh/check-i18n-coverage.sh steps and the architecture plan's ci.yml additions; do not modify or fork their scripts; exactly one CLI-parity script exists in scripts/). (3) Doc-gardening agent: a recurring job registered in jobs.rs (following the meta_spec_conformance_sweep registration pattern) running as the server-side system principal that scans the forge's stored repositories (default-branch tree content read via the existing git operations, for repos under spec governance) for documentation drift with mechanically checkable signals — at minimum: (a) broken cross-references: relative file links in tracked *.md files (AGENTS.md, docs/, specs/) pointing at paths that do not exist in the tree; (b) coverage-ledger drift: specs/coverage rows still marked not-started for sections whose owning task document (specs/tasks/*.md frontmatter) records terminal completion — and converts each finding into a fix-up task via the fixup-mr-pipeline's ensure_fixup_task (system principal, deduped, scoped to the workspace/repo the drifted artifact belongs to; unscopable findings are skipped and logged), so a recurring cleanup agent opens fix-up MRs for stale documentation. The scan evaluates real tree state, never a hardcoded file list.

## Acceptance Criteria

- [ ] A merge whose recorded gate/review outcomes are all green produces a quality-grade entry versioned in the repo (present in the default-branch tree and retrievable via the API), and a merge with materially different recorded outcomes produces a correspondingly different grade — the grade tracks the recorded outcomes.
- [ ] ci.yml runs scripts/check-cli-spec-parity.sh as a mandatory step; introducing a CLI-spec parity violation makes the step fail (verifiable by running the script directly).
- [ ] The doc-garden job is registered in the job registry and runs as the system principal; invoked against repo state containing a tracked *.md file with a relative link to a nonexistent path (and/or a coverage row still not-started for a section whose task document records complete), it creates exactly one scoped, deduped fix-up task via ensure_fixup_task; a second run creates no duplicate; clean state creates none.
- [ ] Doc-garden findings that cannot be scoped to a workspace/repo are skipped and logged, never created under a fabricated scope.
- [ ] check-arch.sh, check-in-memory-state-stores.sh (any durable job state goes through ports + an adapter), check-fabricated-scope-defaults.sh, and check-migration-versions.sh (if a migration is needed) pass; no new exemption entries.

## Verification intent

Quality-grades arm: complete two merges with materially different recorded gate/review outcomes and compare the recorded grades — on current code no quality-grade surface exists anywhere (grep for quality grade/quality_grade across crates/ returns zero matches), so there is nothing to compare; after the task the grades must differ consistently with the differing outcomes — a constant or input-independent grade fails this probe. Doc-garden arm: plant a tracked markdown file referencing ./nonexistent-target.md in a governed repo's default branch and run the job once — on current code no gardening code exists (grep garden/gardening across scripts/, crates/, and .github/ returns zero matches), so no fix-up task can appear; after the task exactly one fix-up task referencing the broken link must exist, and removing the link and re-running must not recreate it (proving evaluation of current tree state, not a remembered list). CI arm: temporarily introduce a CLI-spec parity violation and run the ci.yml step's command — on current code CI never invokes check-cli-spec-parity.sh (it appears only in .pre-commit-config.yaml), so a violating tree passes CI today; after the task the step must fail, proving the docs-current enforcement actually runs in CI.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-experience.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`.

[
  {
    "id": "07a6866179016f8676c1640d",
    "decision": "gap",
    "requirement_quote": "- Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo.\n- **Mechanical enforcement** that docs stay current: linters, CI, doc-gardening agents.",
    "detail": "The artifact half is largely realized; the enforcement half is not. Realized: AGENTS.md is exactly the specced 'map, not a manual' entry point ('This file is the map. Follow the links below for detail.'), and design docs (specs/), execution plans (specs/tasks/ with enforced commits: attribution), architecture docs, and coverage matrices are all versioned in-repo. Missing: (1) quality grades — grep for 'quality grade'/'quality_grade' across crates/ returns zero matches; the concept exists only in spec prose, and its other source (specs/development/manager-agent.md) is marked Superseded in specs/index.md, so no production surface versions or tracks quality grades. (2) Mechanical enforcement that docs stay current — CI's arch-lint job runs ~25 scripts/check-*.sh lints, but they enforce code invariants (architecture boundaries, ABAC registry, byte-slice safety, etc.), not documentation currency; the one doc/spec-parity gate (scripts/check-cli-spec-parity.sh, header claiming 'Run during pre-commit and CI') is wired only into .pre-commit-config.yaml and appears nowhere in .github/workflows/ci.yml. (3) Doc-gardening agents — grep for 'garden'/'gardening' across scripts/, crates/ and .github/ returns zero matches; no production code path spawns an agent to detect or fix stale documentation. The recurring jobs that exist target spec-link staleness and meta-spec pins, not docs generally.",
    "evidence": [
      {
        "file": "scripts/check-task-commit-attribution.sh",
        "quote": "# check-task-commit-attribution.sh — Every task-labeled product-surface",
        "detail": "The artifact half of the section is realized: design docs, architecture docs and execution plans all live versioned in-repo (specs/, specs/tasks/, specs/reviews/, specs/coverage/), and this lint mechanically ties every task-labeled product-surface commit to its task's versioned `commits:` frontmatter. AGENTS.md (inspected) is exactly the specced 'map, not a manual' entry point ('**This file is the map. Follow the links below for detail.**') with progressive disclosure via Documentation Index and Codebase Map tables. What is missing is the quality-grades artifact and the doc-currency enforcement shown by the other evidence."
      },
      {
        "file": ".github/workflows/ci.yml",
        "quote": "run: bash scripts/check-arch.sh",
        "detail": "CI's arch-lint job runs the check-*.sh fleet, but inspected script purposes show they enforce code invariants (hexagonal boundaries, ABAC route registry, migration numbering, UTF-8 slicing, etc.) — none checks that documentation files stay current. No markdownlint, doc-link checker, or api-reference parity check exists in scripts/."
      },
      {
        "file": "scripts/check-cli-spec-parity.sh",
        "quote": "# Run during pre-commit and CI. Not exhaustive — supplements the",
        "detail": "The closest doc/spec-parity gate claims CI wiring in its header, but grep shows cli-spec-parity only in .pre-commit-config.yaml:35 — it is absent from .github/workflows/, so the doc-parity enforcement that does exist is pre-commit-only and bypassable (pre-commit is local)."
      },
      {
        "file": "specs/index.md",
        "quote": "| Manager Agent (superseded) | [`development/manager-agent.md`](development/manager-agent.md) | Superseded -- split into [`system/platform-model.md`](system/platform-model.md) + [`system/agent-runtime.md`](system/agent-runtime.md) §1 |",
        "detail": "Quality grades have no production support: grep for 'quality grade'/'quality_grade' in crates/ returns zero matches, and the only specs mentioning them are this section and the superseded manager-agent.md, whose Manager Agent (periodic quality-grade updates) does not exist in code. Combined with the zero matches for 'garden'/'gardening' across scripts/, crates/ and .github/ (inspected for any doc-gardening agent spawn path), both the quality-grades artifact and the doc-gardening-agent enforcement the section requires are missing."
      }
    ]
  }
]
