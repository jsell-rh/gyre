---
title: "Commit-time enforcement for the unenumerated speed-backpressure invariants: backup files, secret scanning, nix/cargo sync, i18n coverage (web+rust), shellcheck, web build"
spec_ref: "Spec gap spec-17abbc950d9bce3d8a575239 / commit-time-backpressure-invariants / 8207800eca00"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per specs/development/speed-backpressure.md §Pre-Commit Hooks, the enumerated invariants must be mechanically enforced at commit time by the pre-commit hook suite, blocking bad patterns before they enter the repo. The general mechanism already exists (.pre-commit-config.yaml, 78 local hooks, standard pre-commit runner — the spec's 'prek' example is config-compatible), and em-dash rejection is already enforced (no-em-dash / scripts/check-no-em-dash.sh). Implement the remaining invariants with real, blocking commit-time enforcement, adapted to the repo's actual architecture (crane-based Nix build, svelte-i18n web catalog, email.rs Rust locale catalog), following existing scripts/check-*.sh conventions (set -euo pipefail, no network, print file + offender on failure, exit nonzero):

1. block-backup-files: a pre-commit hook that rejects any staged file whose name matches backup/junk patterns (*.bak, *~, *.swp, *.swo, *.orig, *.rej, '#...#' editor locks, '.#...' symlinks), including files force-added past .gitignore (pre-commit passes staged filenames matching a `files:` pattern; the hook exits 1 naming the file). This is enforcement, not ignore-based mitigation — .gitignore's single '*.swp' entry offers no resistance to a forced add or a *.bak/*~ file.

2. block-secrets-backup: a real content-based secret scanner (scripts/check-secrets-scan.sh) over staged text files, detecting credential material by anchored pattern: '-----BEGIN … PRIVATE KEY-----' blocks, AWS access key ids (AKIA[0-9A-Z]{16}), GitHub tokens (ghp_/gho_/ghu_/ghs_/ghr_ + 30+ alphanumerics), GitLab glpat-, Slack xox[baprs]-, Google AIza[0-9A-Za-z_-]{35}, JWTs (eyJ… two-dot structure), and high-entropy assignments to password/secret/token/api_key-style keys (Shannon entropy threshold tuned so the current tree passes). It must exit 0 on the untouched tree (the only tree-wide near-match is a literal "[REDACTED PRIVATE KEY]" placeholder in crates/gyre-adapters/src/llm/rig_vertexai.rs, which anchored patterns do not match) and exit 1 naming file and line on a planted credential. If any narrowly-scoped exemption is genuinely unavoidable it goes in a NEW check-owned exemption file starting empty; no entries may be added to any existing frozen scripts/*-exemptions.txt.

3. cargo2nix-update (keep the Nix build in sync with Cargo — crane-adapted): no Cargo.nix exists and the Nix build uses crane (flake.nix), which reads Cargo.toml/Cargo.lock directly, so the real sync invariant is bin-target coverage: scripts/check-nix-cargo-sync.sh verifies every Cargo workspace bin target (each member crate with src/main.rs or a [[bin]] section — currently gyre-server and gyre-cli) is referenced by flake.nix build outputs (packages/checks), so a new or renamed binary crate that the Nix build silently does not build/test fails the check with the crate named; it also verifies flake.lock is committed and parses. Current tree passes (both bin crates are in flake.nix packages and checks).

4. i18n-coverage-web: a commit-time hook enforcing web i18n completeness: every svelte-i18n key referenced in web/src (t('…') / $_('…') / format lookups) resolves in web/src/locales/en.json, and any additional locale file under web/src/locales/ has full key parity with en.json; violations name the component file and key. NOTE: the active ci-docs-release plan (plan b569dd2616598865fc72c3b7dc3b0f32e81cdb62e23bcc9283249a02f713bc22, task 'ci-invariant-i18n-doc-freshness') creates scripts/check-i18n-coverage.sh for CI enforcement of the identical invariant. REUSE that exact script if it is already present in the tree; implement it to that identical contract only if absent. The deliverable here is the commit-time wiring: a pre-commit hook (id i18n-coverage-web) scoped to web/ paths running the script, complementing rather than duplicating the CI job. Do not fork the script into a second copy.

5. i18n-coverage-rust: a check enforcing completeness of the Rust-side i18n catalog in crates/gyre-server/src/email.rs: every NotificationType variant (parsed from crates/gyre-common/src/notification.rs) must have a heading arm in EVERY locale arm of type_heading() (currently Locale::En and Locale::De), and the per-locale matches must contain no wildcard '_ =>' fallback for NotificationType; a missing variant/locale arm or an introduced wildcard fails with variant + locale named. This guards the real silent-drift risk: today exhaustive matches make the compiler catch a new variant, but converting an arm to a catch-all would silently give every future variant generic copy (the repo already shipped this class of bug — task-209's ComputeTargetUnavailable toggle gap). Current tree passes: both arms list all 41 variants.

6. shellcheck: a pre-commit hook (language: system, shell file types) running shellcheck over the repo's ~100 scripts in scripts/ (and other shell scripts), plus a mandatory CI step (no '|| true' advisory escape) that installs shellcheck (e.g. apt on ubuntu-latest) and runs it repo-wide; severity threshold chosen and documented so the current tree passes (at minimum -S error), with per-line inline shellcheck directives where a specific finding is justified — no blanket exemption file. Add shellcheck to the flake.nix devShell so developer environments have the binary the language: system hook invokes.

7. gyre-web-build: a pre-commit hook (id gyre-web-build) that runs the real web build (npm --prefix web run build) when web sources or web/package.json are staged (files: ^web/ filter), so a web change that breaks the build is blocked at commit time rather than only in the CI web-build job or transitively via crates/gyre-server/build.rs (which is explicitly skippable via SKIP_WEB_BUILD=1 and therefore not commit-time enforcement).

All hooks are registered in .pre-commit-config.yaml following existing entry conventions, are blocking (nonzero exit on violation, never advisory), land green when run across the current tree, and are documented in the docs/development.md hook summary. Do not alter the spec or the existing em-dash/no-em-dash hook.

## Acceptance Criteria

- [ ] .pre-commit-config.yaml registers new hooks for all seven deliverables (backup-file blocking, secret content scanning, nix-cargo sync, i18n-coverage-web, i18n-coverage-rust, shellcheck, gyre-web-build), each wired per existing conventions, and `pre-commit run --all-files` (equivalently: each new script run directly at repo root) exits 0 on the unmodified tree.
- [ ] A file named e.g. crates/gyre-server/src/main.rs.bak staged with `git add -f` is rejected at commit time by the backup-files hook with a nonzero exit naming the file — reproducible via `pre-commit run <hook> --files crates/gyre-server/src/main.rs.bak` after staging; on the pre-change tree the same file commits without resistance (only '*.swp' is gitignored).
- [ ] scripts/check-secrets-scan.sh exits 1 naming file and line when run against a file containing a '-----BEGIN OPENSSH PRIVATE KEY-----' block or an AKIA-style access key id / ghp_-style token, and exits 0 against the unmodified tree; no entry is added to any existing frozen scripts/*-exemptions.txt.
- [ ] scripts/check-nix-cargo-sync.sh exits 0 on the current tree (gyre-server and gyre-cli bin targets both referenced in flake.nix) and exits 1 naming the crate when a workspace member gains a bin target (src/main.rs or [[bin]]) that flake.nix does not reference.
- [ ] The i18n-coverage-web hook exits 1 naming the component file and missing key when a key referenced by a web/src component's t()/$_() call is deleted from web/src/locales/en.json, and exits 0 on the unmodified tree; exactly one i18n coverage script exists in scripts/ (reused from or identical to the ci-docs-release plan's contract — no forked duplicate).
- [ ] scripts checking i18n-coverage-rust exit 1 naming variant and locale when a NotificationType heading arm is removed from (or replaced by a wildcard in) one locale's match in email.rs while the variant exists in the enum, and exit 0 on the unmodified tree where both locale arms list all 41 variants.
- [ ] The shellcheck hook and the mandatory CI step exit nonzero on a scripts/*.sh file containing an unquoted-variable defect at the declared severity (e.g. `if [ $foo ]`), and the full current scripts/ tree passes at that severity; the CI step has no '|| true' or continue-on-error escape.
- [ ] The gyre-web-build pre-commit hook runs the real `npm --prefix web run build` (exit status propagated — a broken staged web change blocks the commit) and is a no-op/pass when no web files are staged.
- [ ] docs/development.md's hook summary documents each new hook, and .github/workflows/ci.yml gains the mandatory shellcheck (and secrets-scan/nix-cargo-sync, consistent with the arch-lint job pattern) steps so the invariants are enforced in both hook and CI contexts.

## Verification intent

Stage two defects in one commit attempt: (a) `git add -f crates/gyre-server/src/main.rs.bak` — a backup-named file that bypasses the '*.swp'-only .gitignore; (b) a staged file whose content contains '-----BEGIN OPENSSH PRIVATE KEY-----' and 'AKIAIOSFODNN7EXAMPLE'. On the current tree this commit succeeds: no hook in .pre-commit-config.yaml matches backup filename patterns (verified by grep across the 78-hook config), no content-based secret scanner exists in hooks or either CI workflow (grep for gitleaks/detect-secrets/trufflehog/block-secrets returns nothing; the only 'secret' check, check-lossy-secret-conversion.sh, is a Rust code-pattern lint), and .gitignore offers no resistance to a forced add. After the task, the same commit attempt must be blocked with a nonzero exit: the backup-files hook naming the .bak file and the secrets scanner naming the file and line holding the key material — proving real commit-time enforcement rather than advisory output. Each per-invariant probe in the acceptance list (deleted en.json key, removed De heading arm, flake-unreferenced bin crate, unquoted shell variable, broken staged web build) must likewise flip from silently-accepted on the current tree to blocked with a specific named offender.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/speed-backpressure.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `9a77d1e70c188ae89e145fdad017cbde0e4fb165fef7b2ab6920df2d568bdb01`.

[
  {
    "id": "2a09dd756eff40d15b609010",
    "decision": "gap",
    "requirement_quote": "These hooks mechanically enforce invariants at commit time - blocking bad patterns before they enter the repo, keeping the wheel spinning clean.",
    "detail": "The section presents the commit-time backpressure mechanism as a pre-commit hook set (framed as an example using prek, a pre-commit-compatible runner) that mechanically enforces eight invariants at commit time: block-backup-files (prevent accidental commits), block-secrets-backup (catch leaked secrets), cargo2nix-update (keep Cargo.nix in sync), i18n-coverage-web and i18n-coverage-rust (enforce i18n completeness), gyre-web-build (verify web builds), shellcheck (lint shell scripts), emdash-check (reject em-dashes). The general mechanism is satisfied and far exceeded in breadth: .pre-commit-config.yaml registers 78 local hooks and the format works with any pre-commit-compatible runner, so the prek framing is config-compatible (docs/development.md documents standard pre-commit). Of the eight enumerated invariants, only one is directly enforced at commit time: em-dash rejection, via the no-em-dash hook (scripts/check-no-em-dash.sh), the functional equivalent of the specced emdash-check. A second is partially covered outside commit time: web-build verification runs as a CI job, and transitively during cargo hooks because crates/gyre-server/build.rs runs 'npm run build' when web sources change (skippable via SKIP_WEB_BUILD=1, so not a dedicated commit-time hook). The remaining six enumerated invariants have no enforcement anywhere -- not at commit time, not in CI, under no hook name: (1) no block-backup-files hook; the only backup-file-adjacent protection is a single '*.swp' entry in .gitignore, which is ignore-based (a forcibly added or differently named backup file such as *.bak or *~ commits without resistance), swp-only, and not commit-time enforcement; (2) no secret-leak scanning at all -- the only 'secret' check in the suite, scripts/check-lossy-secret-conversion.sh, is a Rust code-pattern lint about from_utf8_lossy, not a content scanner, and no gitleaks/detect-secrets/trufflehog runs in hooks or CI; (3) no Cargo.nix exists in the tree and the Nix build uses crane (flake.nix), so the cargo2nix-update sync invariant has no artifact and no enforcement -- the spec example is stale relative to the crane-based build, but it has not been amended via spec lifecycle, so the enumerated invariant remains unimplemented; (4) no i18n-coverage-web hook -- web i18n is svelte-i18n with exactly one locale file (web/src/locales/en.json), and the only locale-label enforcement anywhere is scripts/check-notification-toggle-coverage.sh, which locks NotificationType ids to the single user_profile.notif_types namespace, not general i18n completeness; (5) no i18n-coverage-rust hook -- the Rust side has no i18n message catalog at all (only a locale preference string in gyre-cli), so there is nothing enforcing (or to enforce) rust i18n completeness; (6) no shellcheck hook or CI step despite scripts/ containing roughly one hundred shell scripts. Even read generously (the YAML is labeled 'Example', so hook ids may be illustrative), the enumerated invariants themselves -- leaked-secret detection, shell-script linting, backup-file blocking, i18n completeness -- are not enforced under any name in the 78-hook suite or in either CI workflow, which I verified by function (grep for secret scanners, shellcheck, backup patterns, and i18n coverage across .pre-commit-config.yaml, .github/workflows/, and scripts/), not just by hook id. The section's requirement that these invariants be mechanically enforced at commit time is therefore only partially realized: mechanism present, em-dash enforced, web build enforced out-of-band, six invariants unenforced.",
    "evidence": [
      {
        "file": "scripts/check-no-em-dash.sh",
        "quote": "# Em-dash check: reject Unicode em-dash (U+2014) in source files.",
        "detail": "The specced emdash-check invariant is genuinely enforced at commit time: this script is wired as the no-em-dash hook in .pre-commit-config.yaml (passing rust/toml/yaml/markdown filenames), greps each file for the U+2014 UTF-8 bytes (grep -Pn \"\\xe2\\x80\\x94\"), prints a violation, and exits nonzero. This is the one enumerated invariant with direct commit-time enforcement, and it is blocking, not audit-only."
      },
      {
        "file": "docs/development.md",
        "quote": "- `no-em-dash`: rejects Unicode em-dashes in source",
        "detail": "The hook summary in docs/development.md documents the no-em-dash hook as part of the production pre-commit suite, corroborating that the em-dash invariant (the specced emdash-check) is the one enumerated invariant enforced at commit time."
      },
      {
        "file": "scripts/check-lossy-secret-conversion.sh",
        "quote": "# check-lossy-secret-conversion.sh — byte-oriented security material\n# (secret values) must not pass through String::from_utf8_lossy.",
        "detail": "This is the only check in the entire hook suite whose name or description mentions secrets, and it is a Rust code-pattern lint (rejecting from_utf8_lossy on secret-domain bytes), not a leaked-secret content scanner. grep of .pre-commit-config.yaml and .github/workflows/*.yml for block-secrets, gitleaks, trufflehog, and detect-secrets returns nothing: the specced block-secrets-backup invariant (catch leaked secrets before they enter the repo) is unenforced at commit time and in CI. The same absence search covers backup files: no hook in .pre-commit-config.yaml matches block-backup-files or any backup-pattern scanner; the sole mitigation anywhere is a '*.swp' line in .gitignore, which is ignore-based, swp-only (nothing for *.bak or *~), and offers no resistance to a forced add."
      },
      {
        "file": ".github/workflows/ci.yml",
        "quote": "  web-build:\n    name: Web build + bundle",
        "detail": "Web-build verification (the specced gyre-web-build invariant) exists only as this CI job (npm ci, npm run build, vitest), not as a commit-time hook; no web-build hook exists in .pre-commit-config.yaml. Additionally, neither ci.yml nor e2e.yml contains any shellcheck step (grep for shellcheck across .github/ and scripts/ returns nothing), so the specced shellcheck invariant is unenforced anywhere despite scripts/ holding roughly one hundred shell scripts."
      },
      {
        "file": "crates/gyre-server/build.rs",
        "quote": "    // Watch web source files for changes\n    println!(\"cargo:rerun-if-changed=../../web/src/\");\n    println!(\"cargo:rerun-if-changed=../../web/package.json\");",
        "detail": "Partial, out-of-band coverage of gyre-web-build: build.rs runs 'npm run build' for the Svelte UI whenever web sources change during a cargo build, so the cargo-clippy pre-commit hook transitively exercises the web build. This is incidental (a cargo build script, not a dedicated hook) and explicitly skippable via SKIP_WEB_BUILD=1, so it does not constitute the specced commit-time enforcement."
      },
      {
        "file": "flake.nix",
        "quote": "    crane = {\n      url = \"github:ipetkov/crane\";",
        "detail": "The Nix build uses crane, and no Cargo.nix exists anywhere in the tree (ls Cargo.nix fails; grep finds no cargo2nix references outside this spec). The specced cargo2nix-update invariant (keep Cargo.nix in sync) therefore has no artifact to keep in sync and no enforcement. The spec example appears stale relative to the crane-based build, but it has not been amended through spec lifecycle, so the enumerated invariant remains unimplemented rather than reclassified."
      },
      {
        "file": "web/src/lib/i18n.js",
        "quote": "import en from '../locales/en.json';\n\naddMessages('en', en);",
        "detail": "Web i18n is svelte-i18n initialized with exactly one locale file (web/src/locales/en.json is the only file under web/src matching locale paths), and no i18n-coverage-web hook exists in .pre-commit-config.yaml. General i18n completeness (e.g., every t() key resolving) is not enforced anywhere; web tests only assert that specific namespaces never render raw keys."
      },
      {
        "file": "scripts/check-notification-toggle-coverage.sh",
        "quote": "# Toggle-coverage lint: the HSI §12 notification-preferences UI must offer a\n# toggle for EVERY canonical NotificationType variant.",
        "detail": "The only locale-label enforcement in the entire hook suite is this script (wired as the notification-toggle-coverage hook), which locks the NotificationType id list to labels under the single user_profile.notif_types namespace of web/src/locales/en.json. That is one hand-maintained namespace kept in lockstep with the Rust enum, not the specced i18n completeness (web); i18n-coverage-rust has no counterpart at all."
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "quote": "        /// i18n locale, e.g. \"en-US\"\n        #[arg(long)]\n        locale: Option<String>,",
        "detail": "The only i18n-related code on the Rust side is this locale preference CLI argument in gyre-cli; there is no Rust message catalog or i18n system (grep for i18n across crates/ finds only this and comments). Hence no i18n-coverage-rust hook exists and no rust-side completeness is enforced -- the specced invariant is unimplemented on both the hook and the underlying surface."
      }
    ]
  }
]
