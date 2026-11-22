---
title: "Pre-release dev versions: {next}-dev.{count} computed from commits since the last release, never tagged"
spec_ref: "Spec gap spec-17849f2036d5093f11e2a1ae / prerelease-dev-version / d1e5376399b8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/ci-docs-release.md §Pre-Release Versions, development builds between releases carry an automatically computed pre-release version: 'v1.3.0-dev.42 — 42 commits since last release; computed automatically, not tagged; available via gyre --version for debugging'. (1) Implement compute_dev_version in crates/gyre-server/src/version_compute.rs as a real function over real git state: given the repo path and branch, resolve the latest semver tag (existing latest_semver_tag) and the commit range (existing commits_since), compute the next version per the bump rules (parse_conventional/type_bump fold — a range with a feat yields the minor-next base, fix/perf the patch-next base, breaking the major-next, only no-bump types keep the current version as base), count the commits since the tag (no tag: base v0.0.0 with the full commit count), and produce '{next}-dev.{count}' matching the spec's example semantics (v1.2.3 + a feat + 41 other commits -> v1.3.0-dev.42). (2) The computation is surfaced as a real consumer: the release-prepare response gains the analyzed branch's current dev version (computed on every prepare from the actual repository — no caching, no hardcoding), giving operators the debugging affordance server-side; the build-time injection task (build-time-version-injection) consumes this same function so gyre --version reports it. (3) Never tagged: no code path creates a git tag for a dev version — the dev version is a computed string only (the release tag is created solely by the release flow's merge step); assert in tests that preparing/inspecting a dev version leaves the repo's tag list unchanged.

## Acceptance Criteria

- [ ] On a repo with tag v1.2.3 and a commit range since it containing one feat and 41 other commits, the dev version computes 'v1.3.0-dev.42' (minor-next base per the bump rules); with only fix commits it computes the patch-next base; with only docs/chore commits it keeps v1.2.3 as the base with the commit count.
- [ ] A repo with no semver tag computes a v0.0.0-dev.{total commits} dev version rather than erroring.
- [ ] POST /api/v1/release/prepare reports the branch's current dev version in the response, derived from the real repository state on each call.
- [ ] Computing the dev version creates no git tag: the repo's tag list is byte-identical before and after (dev versions are computed, never tagged).

## Verification intent

Create a repo, tag it v1.2.3, then add exactly 42 commits since the tag of which one is a feat (e.g. 'feat: x') and the rest no-bump types, then request the dev version (release-prepare response / the computation over the real repo): pre-task the probe fails because no code path computes a dev pre-release version at all — version_compute has only tag-stripping logic for reading tags, the prepare response has no dev-version field, and gyre --version prints the manifest constant — so the assertion that the computed value equals 'v1.3.0-dev.42' (and not '0.1.0', not 'v1.2.3', not a tag) has nothing to observe. Control: run git tag --list after the computation and assert it is unchanged — an implementation that realizes the dev version by creating a -dev tag fails this control.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/ci-docs-release.md`. Source: `7e74623345bd6c3cffe0d828aa1b7c8d3690c884`; plan: `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`.

[
  {
    "id": "3d8bcb425b5e801f88e7fb66",
    "decision": "gap",
    "requirement_quote": "- `v1.3.0-dev.42` - 42 commits since last release\n- Computed automatically, not tagged\n- Available via `gyre --version` for debugging",
    "detail": "Pre-release (dev) versions are not implemented at all. No code path computes a '{next}-dev.{count}' version from the commit count since the last release: version_compute has no dev-version function (inspected the whole module — parse_semver merely strips pre-release suffixes when reading tags), and release_prepare only computes the next release version. `gyre --version` reports clap's `version` attribute, which resolves to CARGO_PKG_VERSION from Cargo.toml ('0.1.0'), not a git-derived dev version — so the debugging affordance the spec requires is absent.",
    "evidence": [
      {
        "file": "crates/gyre-cli/src/main.rs",
        "quote": "#[command(name = \"gyre\", about = \"Gyre platform CLI\", version)]",
        "detail": "The CLI's --version output comes from clap's version attribute, i.e. the crate version from Cargo.toml (workspace version 0.1.0). It is a static string at build time from the manifest, not a computed 'v1.3.0-dev.42'-style pre-release version with commit count since the last tag; gyre --version therefore cannot serve the specced debugging purpose."
      },
      {
        "file": "crates/gyre-server/src/version_compute.rs",
        "quote": "    let v = tag.strip_prefix('v').unwrap_or(tag);\n    // Strip any pre-release suffix (e.g., `-dev.42`).\n    let v = v.split('-').next().unwrap_or(v);",
        "detail": "The only handling of pre-release version strings in the version module is stripping them when parsing tags for release computation. The module was inspected in full: there is no function that computes a dev pre-release version (commit-count-since-tag), and no caller anywhere in crates/ constructs one — the 'computed automatically, not tagged' behavior does not exist."
      }
    ]
  }
]
