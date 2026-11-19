---
title: "Publish reviewed spec backlog for specs/development/speed-backpressure.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["5fcc43d33e4850cab6dc007397c79d76ef0844cd"]
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/speed-backpressure.md",
  "plan_id": "9a77d1e70c188ae89e145fdad017cbde0e4fb165fef7b2ab6920df2d568bdb01",
  "audit": {
    "base": "fb1424fbac55cd23cb08606f4cd667a96f4dbc43",
    "requirements": "8207800eca00d5d85eda5dcdc141eca92428eeb7864a00b4ab2f6791aaf2da90",
    "sections": [
      {
        "id": "d4c39dbf614e7deb700b2d55",
        "decision": "context",
        "requirement_quote": "Eventually, **agent-to-agent coordination** should provide much of this backpressure naturally. For now, **pre-commit hooks** are the practical mechanism.",
        "detail": "The document introduction is rationale, not an implementable requirement: it names two dimensions of concern (generation speed; backpressure via tests, lints, hooks), states an explicitly aspirational future ('Eventually, agent-to-agent coordination should provide much of this backpressure naturally'), and defers the current mechanism to the child section 'Pre-Commit Hooks', which is audited separately below. The only factual claim about present state -- that pre-commit hooks are the current practical backpressure mechanism -- is true in production: the repo carries a real, installed pre-commit suite (.pre-commit-config.yaml with 78 local hooks, documented in docs/development.md). No requirement is counted here that the child section accounts for.",
        "evidence": [
          {
            "file": "docs/development.md",
            "quote": "Hook summary (see `.pre-commit-config.yaml` for the full list; ~60 local hooks):",
            "detail": "The 'for now' mechanism the intro asserts exists in production: docs/development.md documents a pre-commit hook suite installed per clone ('pre-commit install' / 'pre-commit install --hook-type commit-msg'), backed by .pre-commit-config.yaml registering 78 local hooks (cargo-fmt, cargo-clippy, arch-lint, no-em-dash, conventional-commits on the commit-msg stage, etc.). The intro's remaining statements are aspirational rationale with no implementable requirement; the normative content lives in the child heading."
          }
        ]
      },
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
    ],
    "gaps": [
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
    ],
    "source_evidence": [
      {
        "file": "docs/development.md",
        "sha256": "1edb15b59093154a4c5a12a7fab657c948120c1f3dca1f955416456931e878ac"
      },
      {
        "file": "scripts/check-no-em-dash.sh",
        "sha256": "aebaeb7f455cf04968c7e706248a2b547704b3872b93c7ecd15eb266db0c7812"
      },
      {
        "file": "scripts/check-lossy-secret-conversion.sh",
        "sha256": "468b86b16c56d30cabcae3ee8fbb1760530e7af213de3a2cf5ea327170cca3e7"
      },
      {
        "file": ".github/workflows/ci.yml",
        "sha256": "a58652ba210d38bd13b8ae0171c0deb7fad593b916d9e2977078a9962404b97a"
      },
      {
        "file": "crates/gyre-server/build.rs",
        "sha256": "ba9a93a3cf1143b76ad556ed98bc807416412283d0692a6f778e18d4879f8a68"
      },
      {
        "file": "flake.nix",
        "sha256": "aef29d3223604d6d4a472ffaa71a0ef485882a2d860fe2d6fb73e60035e4232f"
      },
      {
        "file": "web/src/lib/i18n.js",
        "sha256": "d1a71f93f2c459f0f0f325fac5a986eca4d6131deceaad977229dfd09799f1f8"
      },
      {
        "file": "scripts/check-notification-toggle-coverage.sh",
        "sha256": "d1835cdae4e270aadfe6f7fd7e9e53aa93fe8f2728ee43f46aae204180191a0a"
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "sha256": "b16446e777779fcd02857cd61e536961e6c3283b2443339bd35935e9f9ec5c4d"
      }
    ]
  },
  "plan": {
    "audit": "530a0a1ebfc700938d5d81ec86f7037e5a56375081a4a211fd1cf76f1d536fdc",
    "tasks": [
      {
        "key": "commit-time-backpressure-invariants",
        "title": "Commit-time enforcement for the unenumerated speed-backpressure invariants: backup files, secret scanning, nix/cargo sync, i18n coverage (web+rust), shellcheck, web build",
        "covers": [
          "2a09dd756eff40d15b609010"
        ],
        "required_behavior": "Per specs/development/speed-backpressure.md §Pre-Commit Hooks, the enumerated invariants must be mechanically enforced at commit time by the pre-commit hook suite, blocking bad patterns before they enter the repo. The general mechanism already exists (.pre-commit-config.yaml, 78 local hooks, standard pre-commit runner — the spec's 'prek' example is config-compatible), and em-dash rejection is already enforced (no-em-dash / scripts/check-no-em-dash.sh). Implement the remaining invariants with real, blocking commit-time enforcement, adapted to the repo's actual architecture (crane-based Nix build, svelte-i18n web catalog, email.rs Rust locale catalog), following existing scripts/check-*.sh conventions (set -euo pipefail, no network, print file + offender on failure, exit nonzero):\n\n1. block-backup-files: a pre-commit hook that rejects any staged file whose name matches backup/junk patterns (*.bak, *~, *.swp, *.swo, *.orig, *.rej, '#...#' editor locks, '.#...' symlinks), including files force-added past .gitignore (pre-commit passes staged filenames matching a `files:` pattern; the hook exits 1 naming the file). This is enforcement, not ignore-based mitigation — .gitignore's single '*.swp' entry offers no resistance to a forced add or a *.bak/*~ file.\n\n2. block-secrets-backup: a real content-based secret scanner (scripts/check-secrets-scan.sh) over staged text files, detecting credential material by anchored pattern: '-----BEGIN … PRIVATE KEY-----' blocks, AWS access key ids (AKIA[0-9A-Z]{16}), GitHub tokens (ghp_/gho_/ghu_/ghs_/ghr_ + 30+ alphanumerics), GitLab glpat-, Slack xox[baprs]-, Google AIza[0-9A-Za-z_-]{35}, JWTs (eyJ… two-dot structure), and high-entropy assignments to password/secret/token/api_key-style keys (Shannon entropy threshold tuned so the current tree passes). It must exit 0 on the untouched tree (the only tree-wide near-match is a literal \"[REDACTED PRIVATE KEY]\" placeholder in crates/gyre-adapters/src/llm/rig_vertexai.rs, which anchored patterns do not match) and exit 1 naming file and line on a planted credential. If any narrowly-scoped exemption is genuinely unavoidable it goes in a NEW check-owned exemption file starting empty; no entries may be added to any existing frozen scripts/*-exemptions.txt.\n\n3. cargo2nix-update (keep the Nix build in sync with Cargo — crane-adapted): no Cargo.nix exists and the Nix build uses crane (flake.nix), which reads Cargo.toml/Cargo.lock directly, so the real sync invariant is bin-target coverage: scripts/check-nix-cargo-sync.sh verifies every Cargo workspace bin target (each member crate with src/main.rs or a [[bin]] section — currently gyre-server and gyre-cli) is referenced by flake.nix build outputs (packages/checks), so a new or renamed binary crate that the Nix build silently does not build/test fails the check with the crate named; it also verifies flake.lock is committed and parses. Current tree passes (both bin crates are in flake.nix packages and checks).\n\n4. i18n-coverage-web: a commit-time hook enforcing web i18n completeness: every svelte-i18n key referenced in web/src (t('…') / $_('…') / format lookups) resolves in web/src/locales/en.json, and any additional locale file under web/src/locales/ has full key parity with en.json; violations name the component file and key. NOTE: the active ci-docs-release plan (plan b569dd2616598865fc72c3b7dc3b0f32e81cdb62e23bcc9283249a02f713bc22, task 'ci-invariant-i18n-doc-freshness') creates scripts/check-i18n-coverage.sh for CI enforcement of the identical invariant. REUSE that exact script if it is already present in the tree; implement it to that identical contract only if absent. The deliverable here is the commit-time wiring: a pre-commit hook (id i18n-coverage-web) scoped to web/ paths running the script, complementing rather than duplicating the CI job. Do not fork the script into a second copy.\n\n5. i18n-coverage-rust: a check enforcing completeness of the Rust-side i18n catalog in crates/gyre-server/src/email.rs: every NotificationType variant (parsed from crates/gyre-common/src/notification.rs) must have a heading arm in EVERY locale arm of type_heading() (currently Locale::En and Locale::De), and the per-locale matches must contain no wildcard '_ =>' fallback for NotificationType; a missing variant/locale arm or an introduced wildcard fails with variant + locale named. This guards the real silent-drift risk: today exhaustive matches make the compiler catch a new variant, but converting an arm to a catch-all would silently give every future variant generic copy (the repo already shipped this class of bug — task-209's ComputeTargetUnavailable toggle gap). Current tree passes: both arms list all 41 variants.\n\n6. shellcheck: a pre-commit hook (language: system, shell file types) running shellcheck over the repo's ~100 scripts in scripts/ (and other shell scripts), plus a mandatory CI step (no '|| true' advisory escape) that installs shellcheck (e.g. apt on ubuntu-latest) and runs it repo-wide; severity threshold chosen and documented so the current tree passes (at minimum -S error), with per-line inline shellcheck directives where a specific finding is justified — no blanket exemption file. Add shellcheck to the flake.nix devShell so developer environments have the binary the language: system hook invokes.\n\n7. gyre-web-build: a pre-commit hook (id gyre-web-build) that runs the real web build (npm --prefix web run build) when web sources or web/package.json are staged (files: ^web/ filter), so a web change that breaks the build is blocked at commit time rather than only in the CI web-build job or transitively via crates/gyre-server/build.rs (which is explicitly skippable via SKIP_WEB_BUILD=1 and therefore not commit-time enforcement).\n\nAll hooks are registered in .pre-commit-config.yaml following existing entry conventions, are blocking (nonzero exit on violation, never advisory), land green when run across the current tree, and are documented in the docs/development.md hook summary. Do not alter the spec or the existing em-dash/no-em-dash hook.",
        "acceptance": [
          ".pre-commit-config.yaml registers new hooks for all seven deliverables (backup-file blocking, secret content scanning, nix-cargo sync, i18n-coverage-web, i18n-coverage-rust, shellcheck, gyre-web-build), each wired per existing conventions, and `pre-commit run --all-files` (equivalently: each new script run directly at repo root) exits 0 on the unmodified tree.",
          "A file named e.g. crates/gyre-server/src/main.rs.bak staged with `git add -f` is rejected at commit time by the backup-files hook with a nonzero exit naming the file — reproducible via `pre-commit run <hook> --files crates/gyre-server/src/main.rs.bak` after staging; on the pre-change tree the same file commits without resistance (only '*.swp' is gitignored).",
          "scripts/check-secrets-scan.sh exits 1 naming file and line when run against a file containing a '-----BEGIN OPENSSH PRIVATE KEY-----' block or an AKIA-style access key id / ghp_-style token, and exits 0 against the unmodified tree; no entry is added to any existing frozen scripts/*-exemptions.txt.",
          "scripts/check-nix-cargo-sync.sh exits 0 on the current tree (gyre-server and gyre-cli bin targets both referenced in flake.nix) and exits 1 naming the crate when a workspace member gains a bin target (src/main.rs or [[bin]]) that flake.nix does not reference.",
          "The i18n-coverage-web hook exits 1 naming the component file and missing key when a key referenced by a web/src component's t()/$_() call is deleted from web/src/locales/en.json, and exits 0 on the unmodified tree; exactly one i18n coverage script exists in scripts/ (reused from or identical to the ci-docs-release plan's contract — no forked duplicate).",
          "scripts checking i18n-coverage-rust exit 1 naming variant and locale when a NotificationType heading arm is removed from (or replaced by a wildcard in) one locale's match in email.rs while the variant exists in the enum, and exit 0 on the unmodified tree where both locale arms list all 41 variants.",
          "The shellcheck hook and the mandatory CI step exit nonzero on a scripts/*.sh file containing an unquoted-variable defect at the declared severity (e.g. `if [ $foo ]`), and the full current scripts/ tree passes at that severity; the CI step has no '|| true' or continue-on-error escape.",
          "The gyre-web-build pre-commit hook runs the real `npm --prefix web run build` (exit status propagated — a broken staged web change blocks the commit) and is a no-op/pass when no web files are staged.",
          "docs/development.md's hook summary documents each new hook, and .github/workflows/ci.yml gains the mandatory shellcheck (and secrets-scan/nix-cargo-sync, consistent with the arch-lint job pattern) steps so the invariants are enforced in both hook and CI contexts."
        ],
        "negative_probe": "Stage two defects in one commit attempt: (a) `git add -f crates/gyre-server/src/main.rs.bak` — a backup-named file that bypasses the '*.swp'-only .gitignore; (b) a staged file whose content contains '-----BEGIN OPENSSH PRIVATE KEY-----' and 'AKIAIOSFODNN7EXAMPLE'. On the current tree this commit succeeds: no hook in .pre-commit-config.yaml matches backup filename patterns (verified by grep across the 78-hook config), no content-based secret scanner exists in hooks or either CI workflow (grep for gitleaks/detect-secrets/trufflehog/block-secrets returns nothing; the only 'secret' check, check-lossy-secret-conversion.sh, is a Rust code-pattern lint), and .gitignore offers no resistance to a forced add. After the task, the same commit attempt must be blocked with a nonzero exit: the backup-files hook naming the .bak file and the secrets scanner naming the file and line holding the key material — proving real commit-time enforcement rather than advisory output. Each per-invariant probe in the acceptance list (deleted en.json key, removed De heading arm, flake-unreferenced bin crate, unquoted shell variable, broken staged web build) must likewise flip from silently-accepted on the current tree to blocked with a specific named offender.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "commit-time-backpressure-invariants": "task-256",
    "_publication": "task-257",
    "_base": "bd6110839d3639df7436ec10960e27518829c703"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Publication of reviewed backlog plan 9a77d1e70c18 for specs/development/speed-backpressure.md, plus repair of the one verification finding retained against the current head. No product behavior, spec requirement, coverage status or gate was changed.

### Backlog publication (f046fe63)

- `specs/tasks/task-256.md` created from the reviewed plan task `commit-time-backpressure-invariants`: title, required behavior, all nine acceptance criteria, verification intent and discovery evidence reproduced verbatim from the plan (byte-compared against the plan JSON, not paraphrased). Progress stays `not-started` until this publication merges.
- `specs/tasks/task-257.md` carries the full reviewed plan record: audit base fb1424fb, requirements hash 8207800e, one gap section (2a09dd756eff40d15b609010) and one context section.
- `specs/task-spec-inputs.json` gained exactly two entries: the section binding `backlog/2a09dd756eff40d15b609010` -> specs/development/speed-backpressure.md under heading ["The Wheel: Speed & Backpressure", "Pre-Commit Hooks"], and the task binding for spec gap `spec-17abbc950d9bce3d8a575239 / commit-time-backpressure-invariants / 8207800eca00`.
- Audit challenged against the current tree before approving, not just trusted: 78 local hooks with only no-em-dash among the eight enumerated invariants enforced at commit time; no backup-file, secret-scanner, shellcheck, i18n-coverage or web-build hook in .pre-commit-config.yaml or either CI workflow; .gitignore covers only *.swp; no Cargo.nix exists (crane-based flake); web/src/locales holds only en.json; email.rs type_heading() lists all 41 NotificationType variants in both Locale::En and Locale::De with no wildcard fallback. No other task covers this gap; task-256 has no dependencies and no existing-task collision (scripts/check-i18n-coverage.sh is absent, so the reuse-or-implement note is a correct contingency, not a fork of an existing script).
- Nothing else was touched by the publication commit: only the three files above.

### Verification-finding repair (5fcc43d3)

Retained finding 68e962da (npm test --no-file-parallelism at f046fe63, exit 1) failed exactly one test: WorkspaceSettings "renders invitation status and expiry, with revoke only for pending rows" expected "Expires in 30m", received "Expires in 29m". Root cause is a timing flake, not a product defect: the fixture computed `expires_at` from `Date.now()` at mock-setup time while `invitationExpiry()` re-reads `Date.now()` at render time, so any elapsed milliseconds floored 1799s to 29 whole minutes. Fixed by pinning `Date.now` via `vi.spyOn` for that test so fixture and render see the same instant; the exact `toBe('Expires in 30m')` assertion is unchanged in strength. The CI log itself is the pre-fix reproduction.

### Test evidence

- `npx vitest run src/__tests__/WorkspaceSettings.test.js` (locked versions, after `npm ci`): 85/85 passed, including the repaired test. Saved under /tmp/stage/review-evidence/workspace-settings-focused.txt.
- No Rust code changed; no server tests are required of the verifier beyond the standard suite (cargo test --all belongs to full verification as usual).
- Full `npm test`, exact-head GitHub checks and repo-wide gates remain with independent verification, per assignment scope.
