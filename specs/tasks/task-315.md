---
title: "CI enforcement for the two named missing invariants: web i18n coverage and doc freshness"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / ci-invariant-i18n-doc-freshness / d1e5376399b8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §CI, the pipeline must enforce the named mechanical invariants that are currently absent: i18n coverage and doc freshness. (1) scripts/check-i18n-coverage.sh — real, blocking enforcement of web i18n completeness: every svelte-i18n message key referenced in web/src (t('…') / $_('…') / format lookups) resolves in web/src/locales/en.json, and any additional locale file under web/src/locales/ has full key parity with en.json; violations print the component file and key and exit 1; the unmodified tree exits 0. This exact script and contract are cross-referenced by the speed-backpressure plan's i18n-coverage-web pre-commit hook (it reuses this script; do not fork a second copy — exactly one i18n coverage script exists in scripts/). (2) scripts/check-doc-freshness.sh — real doc-freshness enforcement with code as source of truth, two anchors: (a) every REST route registered in crates/gyre-server/src/api/mod.rs (.route(...) path strings) is documented in docs/api-reference.md; (b) every GYRE_* environment variable read in crates/ (std::env::var("GYRE_*") / env! / option_env!) is documented in docs/server-config.md. Violations name the route/env var and the doc file that is missing it; exit 1. Where the current tree has genuinely undocumented routes or env vars, fix the docs (real content, derived from the code's actual behavior) so the check lands green — no exemption entries in any frozen scripts/*-exemptions.txt. (3) Wire both scripts as mandatory steps in .github/workflows/ci.yml following the existing check-step conventions (e.g. the arch-lint job), never advisory (`|| true` forbidden). Coordination: the architecture plan (task-311) and this task both add steps to ci.yml — additions only, no modification of their steps.

## Acceptance Criteria

- [ ] scripts/check-i18n-coverage.sh exits 0 on the unmodified tree and exits 1 naming the component file and missing key when a key referenced by a web/src component's t()/$_() call is removed from web/src/locales/en.json; exactly one i18n coverage script exists in scripts/.
- [ ] scripts/check-doc-freshness.sh exits 0 on the unmodified tree and exits 1 naming the route when a route registered in api/mod.rs is removed from docs/api-reference.md, and exits 1 naming the variable when a GYRE_* env read is added to crates/ without a docs/server-config.md entry.
- [ ] ci.yml runs both scripts as mandatory steps; inserting a deliberate violation locally makes the corresponding step fail (verifiable by running the scripts directly).
- [ ] No entry is added to any existing frozen scripts/*-exemptions.txt.

## Verification intent

Plant `t('definitely.missing.key')` in a web/src component and run scripts/check-i18n-coverage.sh: pre-task no such script exists (the CI pipeline has no i18n coverage enforcement — the only locale is en and nothing checks key usage), so the probe cannot pass; post-task it must exit 1 naming that component file and key, and exit 0 again once the key is defined. Doc-freshness arm: add `std::env::var("GYRE_PROBE_UNDOCUMENTED")` to a crate and run scripts/check-doc-freshness.sh — it must exit 1 naming GYRE_PROBE_UNDOCUMENTED and docs/server-config.md, proving the check tracks code (the source of truth) rather than asserting a symbol exists.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "b7c86bc0e72bd8b20c253c5c",
    "decision": "gap",
    "requirement_quote": "- **GitHub Actions** - robust CI pipeline from day one.\n- CI enforces all mechanical invariants (architecture lints, tests, i18n coverage, doc freshness, etc.).",
    "detail": "The first bullet is satisfied: a robust GitHub Actions pipeline exists from day one (.github/workflows/ci.yml with fmt, clippy, an extensive architecture/invariant lint job, nextest, web build + vitest, agent-runner tests, release build; plus e2e.yml), and it enforces many mechanical invariants including architecture lints and tests. However the second bullet names two invariants CI does not enforce: (1) i18n coverage — the web app uses svelte-i18n with exactly one locale (web/src/locales/en.json), and no CI step or script checks i18n key coverage; (2) doc freshness — no CI step or script under scripts/ checks documentation freshness against code. Both named invariants are absent from the enforcement surface, so the section is only partially implemented.",
    "evidence": [
      {
        "file": ".github/workflows/ci.yml",
        "quote": "      - name: ABAC route registry lint\n        run: bash scripts/check-abac-route-registry.sh",
        "detail": "ci.yml's arch-lint job enforces many mechanical invariants (check-arch.sh, check-api-auth.sh, check-tenant-filter.sh, ABAC route registry, migration versions, dead MessageKinds, notification toggle coverage, etc.) and the test job runs cargo nextest --all — the 'architecture lints' and 'tests' invariants are genuinely enforced. But the full workflow was inspected: no step enforces i18n coverage or documentation freshness; there is no scripts/check-* script for either concern (inspected scripts/ directory listing and grepped scripts/ and .github/workflows/ for i18n/freshness)."
      },
      {
        "file": "web/src/lib/i18n.js",
        "quote": "addMessages('en', en);",
        "detail": "The product's i18n system (svelte-i18n) registers exactly one locale, en, from web/src/locales/en.json (the only file in web/src/locales/). No CI step or script verifies i18n coverage (e.g. that components use message keys or that locales are complete); the named 'i18n coverage' invariant is not enforced anywhere in the CI pipeline."
      }
    ]
  }
]
