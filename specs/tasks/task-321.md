---
title: "Publish reviewed spec backlog for specs/development/ci-docs-release.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/ci-docs-release.md",
  "plan_id": "a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267",
  "audit": {
    "base": "7e74623345bd6c3cffe0d828aa1b7c8d3690c884",
    "requirements": "d1e5376399b86047195a0a04d683e471167228df24baf1814c0cca2e75a475d4",
    "sections": [
      {
        "id": "84f92b694520715c7c4678be",
        "decision": "context",
        "requirement_quote": "# Documentation, CI & Release",
        "detail": "Document title heading only; it carries no standalone requirement. Its three child areas (User-Facing Docs, CI, Versioning & Release) are audited under their own heading IDs.",
        "evidence": []
      },
      {
        "id": "cdd27aa428258fd587e513cd",
        "decision": "gap",
        "requirement_quote": "- **Starlight** (Astro) for user-facing documentation.\n- Deployed via **GitHub Pages**.\n- Docs are a first-class deliverable, not an afterthought.",
        "detail": "The spec requires user-facing documentation to be built with Starlight (Astro) and deployed via GitHub Pages. The repository's user-facing documentation is plain Markdown under docs/ (api-reference.md, cli.md, server-config.md, ui.md, etc.) served by nothing; there is no Astro/Starlight project anywhere in the repo (no astro.config.*, no astro/starlight dependency in any package manifest — the only web build is the Svelte dashboard under web/), and there is no GitHub Pages deployment: .github/workflows/ contains exactly two workflows (ci.yml and e2e.yml), neither of which deploys documentation. The markdown docs being extensive satisfies the 'first-class deliverable' spirit only partially; the two concrete named requirements (Starlight toolchain, GitHub Pages deployment) are unimplemented.",
        "evidence": [
          {
            "file": "docs/cli.md",
            "quote": "# Gyre CLI Reference",
            "detail": "User-facing documentation is plain Markdown files under docs/ (cli.md, api-reference.md, server-config.md, ui.md, development.md, agent-protocol.md). Inspected the whole repository for an Astro/Starlight docs site: no astro.config.* exists, and no package.json (root, web/, docker/gyre-agent) declares astro or @astrojs/starlight as a dependency — web/package.json is the Svelte dashboard, not a docs site. No Starlight docs build exists."
          },
          {
            "file": ".github/workflows/e2e.yml",
            "quote": "name: E2E Tests",
            "detail": "The .github/workflows/ directory contains exactly two files: ci.yml and this e2e.yml. Both were read in full; neither contains any GitHub Pages deployment step (no actions/deploy-pages, no peaceiris/actions-gh-pages, no pages-publish job). There is no documentation deployment pipeline of any kind."
          }
        ]
      },
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
      },
      {
        "id": "aa31b838c55b0b02a3bf3e50",
        "decision": "context",
        "requirement_quote": "## Versioning & Release",
        "detail": "Bare parent heading with no introductory text of its own; all normative content lives in its child sections (Conventional Commits: Forge-Enforced and its children, Automatic Version Computation, Changelog Generation, Release Process, Pre-Release Versions, Version in Code, Relationship to Existing Specs), each audited under its own heading ID.",
        "evidence": []
      },
      {
        "id": "27e35b4261dcf6bc873ac8bf",
        "decision": "context",
        "requirement_quote": "Conventional commit format is enforced at the **git protocol level** via pre-accept validation. Pushes with non-conforming commit messages are rejected before entering the repo.",
        "detail": "This parent heading states the enforcement contract that its child heading 'Pre-Accept Enforcement' elaborates in concrete rules (every commit validated, unknown types rejected, empty descriptions rejected, scope format, sideband error). To avoid double-counting, the enforcement requirement is evaluated once, under the Pre-Accept Enforcement heading (0707840bfbb865b285a8c0a7), where it is recorded as a gap: the implemented conventional-commit validation is an opt-in per-repo pre-accept gate, not an unconditional git-protocol-level enforcement, so pushes to repos that have not registered the gate are accepted with non-conforming messages.",
        "evidence": []
      },
      {
        "id": "f853f4635add08f3e7a8802b",
        "decision": "satisfied",
        "requirement_quote": "```\n<type>(<scope>): <description>\n\n[optional body]\n\n[optional footer(s)]\n```",
        "detail": "The format itself is implemented by two production parsers. The push-time gate parser (pre_accept.rs is_conventional) validates the subject line pattern type[(scope)]: description with non-empty description and allows arbitrary body lines (it inspects only the first line). The release-time parser (version_compute.rs parse_conventional) accepts the full message: subject line plus optional body, and detects BREAKING CHANGE: footers anywhere in the message body. Both run in production paths (git receive-pack gate evaluation and POST /api/v1/release/prepare). The format is parsed exactly as specced; the enforcement-scope question (whether every push is validated) belongs to the Pre-Accept Enforcement heading.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "fn is_conventional(msg: &str) -> bool {\n    let first_line = msg.lines().next().unwrap_or(\"\").trim();",
            "detail": "The push-time ConventionalCommitGate validates the subject line of each pushed commit message against the type(scope): description pattern; body and footers are permitted (only the first line is checked). This gate runs inside check_pre_accept_gates during git-receive-pack handling, so the format is enforced on real pushes for repos that register the gate."
          },
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "/// Accepts the full commit message (subject + body). The subject line is\n/// extracted from the first line; `BREAKING CHANGE:` footers are detected\n/// anywhere in the message body.",
            "detail": "parse_conventional implements the full specced format including optional body and optional footers, and is the production parser used by release prepare (api/release.rs) and by push-time breaking-change detection (git_http.rs detect_breaking_commits)."
          }
        ]
      },
      {
        "id": "b18ea381e9d8f5cb430d024e",
        "decision": "satisfied",
        "requirement_quote": "| `feat` | Minor bump | New functionality |\n| `fix` | Patch bump | Bug fix |",
        "detail": "The semver-effect mapping of the type table is implemented exactly in production code: type_bump maps feat to Minor, fix and perf to Patch, and every other type (docs, style, refactor, test, build, ci, chore) to None, which the release flow uses to decide has_release (no release when only no-bump types). The push-time gate additionally whitelists the ten spec types (plus 'revert', an extra type not in the spec table — noted under Pre-Accept Enforcement, where the type-rejection rule lives). The 'When to Use' column is guidance, not a mechanical requirement.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "    match commit_type {\n        \"feat\" => BumpLevel::Minor,\n        \"fix\" | \"perf\" => BumpLevel::Patch,\n        _ => BumpLevel::None,\n    }",
            "detail": "type_bump implements the spec's semver-effect table: feat -> minor, fix/perf -> patch, all other types (docs, style, refactor, test, build, ci, chore) -> no bump. This function is called from the production release-prepare handler (api/release.rs release_prepare) to compute the bump level, and has_release = bump != BumpLevel::None implements 'Only docs, style, refactor, test, build, ci, chore -> no release'."
          }
        ]
      },
      {
        "id": "bf9395564b122cc17b3e25d5",
        "decision": "satisfied",
        "requirement_quote": "A `BREAKING CHANGE:` footer or `!` after the type triggers a **major** version bump",
        "detail": "Both breaking-change markers are detected in production and both trigger a major bump. parse_conventional sets is_breaking when the type is suffixed with '!' (immediately before the colon) or when the message body contains a 'BREAKING CHANGE:' footer (full message %B is used so body footers are seen). The release flow escalates to BumpLevel::Major for any breaking commit (major wins over other bumps via BumpLevel ordering). Additionally, git_http.rs detect_breaking_commits uses the same parser at push time to record BreakingChange entries and feed merge gates for pushes carrying breaking markers.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "    let has_breaking_footer = message.contains(\"BREAKING CHANGE:\");",
            "detail": "parse_conventional detects the BREAKING CHANGE: footer anywhere in the full commit message; combined with the '!' suffix detection this implements both spec'd breaking markers."
          },
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "    let (type_scope_clean, bang) = if let Some(stripped) = type_scope.strip_suffix('!') {\n        (stripped, true)\n    } else {\n        (type_scope, false)\n    };",
            "detail": "The '!' after the type (before the colon) is detected and sets the breaking flag, matching the spec's 'feat(auth)!: ...' example."
          },
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "        if cc.is_breaking {\n            bump = BumpLevel::Major;\n        } else {",
            "detail": "In the production release-prepare handler, any breaking commit escalates the bump to Major; because BumpLevel is ordered Major > Minor > Patch > None and the loop keeps the maximum, 'major wins' as the spec requires."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    if let Some(parsed) = crate::version_compute::parse_conventional(sha, message) {\n            if parsed.is_breaking {\n                results.push((sha.to_string(), parsed.description));\n            }\n        }",
            "detail": "detect_breaking_commits runs at push time on the real receive-pack commit log (git log --format with full messages) and surfaces breaking commits into the forge's BreakingChange record and merge-gate flow, so breaking markers are acted on beyond version computation as well."
          }
        ]
      },
      {
        "id": "0707840bfbb865b285a8c0a7",
        "decision": "gap",
        "requirement_quote": "The forge validates commit messages at push time:\n- Every commit in the push must match the conventional format\n- Unknown types are rejected\n- Empty descriptions are rejected\n- Scope is optional but must be lowercase alphanumeric if present",
        "detail": "Four concrete deviations from the specced enforcement contract. (1) The enforcement is opt-in per repository, not forge-level: check_pre_accept_gates returns Ok with no evaluations when a repo has no gates configured, and gates are only registered via the push-gates administration API (set_for_repo); a newly created repo has no conventional-commit gate, so pushes with non-conforming messages to such repos are accepted — the spec's unconditional 'Pushes with non-conforming commit messages are rejected before entering the repo' does not hold. (2) The scope rule 'must be lowercase alphanumeric if present' is not enforced at all: is_conventional only matches parentheses and never validates the scope's content or case ('feat(AUTH): x' or 'feat(My Scope): x' pass the gate). (3) The error contract is not met: the spec requires a clear error in git sideband output naming the offending commit ('remote: [GYRE] Push rejected: commit abc123 has invalid message format ... Got: ...'); the implementation returns an HTTP 403 plain-text body 'push rejected by gate ...' with no commit SHA and no sideband pkt-line framing. (4) The gate's type whitelist accepts 'revert', a type not in the spec's Types table, so the 'unknown types are rejected' rule is broader than the contract. Additionally, the gate runs after receive-pack has processed the packfile and undoes the ref updates on rejection (undo_ref_updates), so rejected commits do briefly enter the object database even though the ref never moves — weaker than 'before entering the repo'. What does work: every commit in the push range is checked, unknown (non-whitelisted) types are rejected, and empty descriptions are rejected.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    if gate_names.is_empty() {\n        return Ok(Vec::new());\n    }",
            "detail": "check_pre_accept_gates skips all validation for repos with no configured gates. Gate names come from state.repo_push_gates.get_for_repo(repo_id); the only production setter is the push-gates administration API (api/push_gates.rs set_for_repo). Repo creation (api/repos.rs) registers no default gates, so a default repo enforces no commit-format validation — the spec's git-protocol-level unconditional enforcement is not implemented."
          },
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "    let types = [\n        \"feat\", \"fix\", \"docs\", \"style\", \"refactor\", \"perf\", \"test\", \"build\", \"ci\", \"chore\",\n        \"revert\",\n    ];",
            "detail": "The gate's allowed-type whitelist. It rejects types outside the list (unknown types rejected) but includes 'revert', which is not one of the spec's ten types, so the whitelist does not match the spec's Types table."
          },
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "    let description = rest.trim_start_matches(' ');\n    !description.is_empty()",
            "detail": "Empty descriptions are rejected (this part of the contract is met). However, the full is_conventional function was inspected: the only scope handling is locating '(' and ')' in the type part; the scope's content is never validated for lowercase-alphanumeric form, so 'feat(AUTH): x' and 'feat(Any Thing): x' pass — the 'Scope is optional but must be lowercase alphanumeric if present' rule is unenforced."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\n                        \"push rejected by gate '{}': {}\",\n                        rejection.gate_name, rejection.reason\n                    ),",
            "detail": "On gate rejection the handler returns an HTTP 403 with this plain-text body. The spec requires the error in git sideband output ('remote: [GYRE] Push rejected: commit abc123 has invalid message format / Expected: ... / Got: ...'), naming the offending commit SHA; the actual response carries no SHA and is not sideband-formatted (the sideband builder build_feedback_sideband is only used for the acceptance path)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    // Pre-accept gate checks: run after git accepts packfile, undo refs on failure.",
            "detail": "The gates run after run_git_stateless('receive-pack', ...) has executed; on rejection undo_ref_updates rolls the refs back. The ref never points at the rejected commits, but the packfile objects do enter the repository's object database before rejection — weaker than the spec's 'rejected before entering the repo'."
          }
        ]
      },
      {
        "id": "7871b6fde30347376b444a0e",
        "decision": "satisfied",
        "requirement_quote": "The forge computes the next version from the commit history between the latest semver tag and HEAD",
        "detail": "Implemented in the production release-prepare handler. latest_semver_tag finds the newest v-prefixed semver tag in the real bare repo (git tag --list with version sort); commits_since lists (sha, full message) pairs for tag..branch via git log; the handler folds parse_conventional/type_bump over the range with BumpLevel ordering, so any breaking commit yields Major ('major wins'), feat yields Minor, fix/perf yields Patch, and only no-bump types yield no release (has_release false, version unchanged). compute_next_version then derives v{major+1}.0.0 / v{major}.{minor+1}.0 / v{major}.{minor}.{patch+1} from the parsed tag. The analysis target is the repo's default branch (or an explicitly requested branch), which is the release-relevant interpretation of HEAD.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "    let tag = if let Some(f) = req.from {\n        Some(f)\n    } else {\n        latest_semver_tag(&repo_path).await\n    };",
            "detail": "The production handler resolves the 'from' ref as the latest semver tag of the real repository on disk (latest_semver_tag shells out to git tag --list 'v[0-9]*.*[0-9]' --sort=-version:refname), matching the spec's 'latest semver tag' anchor."
          },
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "    let raw_commits = commits_since(&repo_path, tag.as_deref(), &branch)\n        .await\n        .unwrap_or_default();",
            "detail": "commits_since runs git log with a tag..branch range over the real repository (full messages via %B so BREAKING CHANGE footers are visible), giving the commit history between the latest tag and the branch head that the bump rules fold over."
          },
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "                match bump {\n                    BumpLevel::Major => format!(\"v{}.0.0\", major + 1),\n                    BumpLevel::Minor => format!(\"v{major}.{}.0\", minor + 1),\n                    BumpLevel::Patch => format!(\"v{major}.{minor}.{}\", patch + 1),\n                    BumpLevel::None => tag.to_string(), // no change\n                }",
            "detail": "compute_next_version implements the semver arithmetic for each bump level on top of the parsed current tag, including the no-bump case that keeps the version unchanged; combined with the Major-wins fold in release_prepare this realizes the spec's example (v1.2.3 + breaking -> v2.0.0)."
          }
        ]
      },
      {
        "id": "5e69a034fc0a01c1f8222f5e",
        "decision": "gap",
        "requirement_quote": "Each entry includes:\n- **Agent attribution** - which agent produced the change\n- **Task reference** - which task it was part of\n- **Spec reference** - which spec it implements (from commit provenance)\n- **Conventional commit scope** - which area of the codebase",
        "detail": "Changelog generation exists and is real: release_prepare renders a markdown changelog grouped into BREAKING CHANGES / Features / Bug Fixes, each entry carrying the conventional commit scope, the producing agent (name or id, resolved from the agent-commit provenance store) and the task reference. But the fourth required attribution — the spec reference ('which spec it implements (from commit provenance)') — is absent: the provenance record the changelog is built from (AgentCommit) has no spec-reference field (only task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level), and ChangelogEntry has no spec-reference field. There is no lookup anywhere in the release path that would map a commit to the spec it implements, so the generated changelogs cannot contain the specced spec references.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "pub struct ChangelogEntry {\n    pub sha: String,\n    pub commit_type: String,\n    pub scope: Option<String>,\n    pub description: String,\n    pub is_breaking: bool,\n    pub agent_id: Option<String>,\n    pub agent_name: Option<String>,\n    pub task_id: Option<String>,\n}",
            "detail": "The changelog entry type carries scope (conventional commit scope), agent attribution (agent_id/agent_name) and task reference — but no spec-reference field. The spec explicitly requires each entry to include 'Spec reference - which spec it implements (from commit provenance)'; the rendered markdown in render_changelog_markdown correspondingly emits only scope, agent and task attribution."
          },
          {
            "file": "crates/gyre-domain/src/agent_tracking.rs",
            "quote": "pub struct AgentCommit {\n    pub id: Id,\n    pub agent_id: Id,\n    pub repository_id: Id,\n    pub commit_sha: String,\n    pub branch: String,\n    pub timestamp: u64,",
            "detail": "AgentCommit is the commit-provenance record the changelog attribution is resolved from (release_prepare builds prov_map from state.agent_commits.find_by_repo keyed by commit_sha). Its full field list was inspected: id, agent_id, repository_id, commit_sha, branch, timestamp, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level — there is no spec-reference field, so 'which spec it implements' cannot be derived from commit provenance as the spec requires."
          }
        ]
      },
      {
        "id": "c467a8f584b42b34991ff696",
        "decision": "gap",
        "requirement_quote": "1. **Human or workspace orchestrator triggers release:**\n   ```\n   gyre release prepare\n   ```",
        "detail": "The release flow is only partially implemented. Step 1: the human trigger is real (gyre release prepare CLI command -> client.release_prepare -> POST /api/v1/release/prepare), but the workspace-orchestrator trigger is not: the endpoint is Admin-role-only (NEW-41), agent-token authentication always assigns UserRole::Agent, so no agent persona — including the workspace orchestrator, which is an agent persona per specs/system/platform-model.md — can trigger a release, and there is no MCP tool or other orchestrator code path wrapping release prepare (the only production callers are the CLI command and the admin-only HTTP route). This is an intentional code-versus-spec deviation (NEW-41) with no spec amendment narrowing the spec's actor enumeration, so per the audit rules it is an implementation gap: either the orchestrator trigger must be implemented with scoped authorization, or the spec must be amended through the spec lifecycle. Steps 2-3 are implemented (version computation and changelog generation, see those headings). Step 4 is largely missing: the changelog is produced, and spec-registry manifest state (the AIBOM's spec provenance section) is embedded in the prepare response, but there is no SBOM generation, no signed git tag (no Sigstore signing of release tags anywhere in the release path), no Nix binary artifacts packaged into a release, and no Docker image build. Step 5 is partial: create_mr opens a merge request, but it contains no version bump in Cargo.toml, no updated changelog file, and no AIBOM snapshot — the code's own comment admits 'In a real release flow the agent would create a version-bump commit and push it first; here we open the MR against the current branch'. Step 6 is missing: no release-specific gate wiring exists (no security-agent vulnerability scan 'since last release' and no accountability-agent verification that spec-drift-review tasks are resolved, tied to release MRs). Step 7 is entirely absent: nothing tags the repo on merge, creates a GitHub Release, pushes a Docker image to a registry, or deploys docs to GitHub Pages — the release API surface consists solely of POST /api/v1/release/prepare.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {",
            "detail": "release_prepare rejects every principal without UserRole::Admin ('only Admin role may trigger release preparation'), with an NEW-41 regression test (agent_role_cannot_trigger_release_prepare) asserting an agent-role JWT gets 403. Agent-token authentication always assigns roles: vec![UserRole::Agent] (crates/gyre-server/src/auth.rs, both agent-token arms), and UserRole is exactly {Admin, Developer, Agent, ReadOnly} (crates/gyre-domain/src/user.rs), so no agent persona can hold Admin. The workspace orchestrator is an agent persona, so the spec's 'workspace orchestrator triggers release' actor cannot trigger it. Inspected all callers of release_prepare: only the CLI (crates/gyre-cli/src/main.rs Release variant) and this HTTP route; mcp.rs contains no release tool and no orchestrator path calls it. The Admin-only restriction contradicts the spec's actor enumeration without any spec amendment."
          },
          {
            "file": "specs/system/platform-model.md",
            "quote": "- **Manager Agent** (`development/manager-agent.md`): split into workspace orchestrator + repo orchestrator",
            "detail": "The workspace orchestrator is an agent persona (personas/workspace-orchestrator.md), i.e. it authenticates as an agent with Agent role — the class the NEW-41 admin gate excludes. The spec's step 1 names 'Human or workspace orchestrator' as trigger actors; only the human half is implemented."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/release/prepare\", post(release::release_prepare))",
            "detail": "The complete release API surface is this single route. There is no release finalize/publish/tag endpoint anywhere in the route table (inspected api/mod.rs in full): nothing creates a signed git tag on merge, publishes a GitHub Release with artifacts, pushes a Docker image to a registry, or deploys docs to GitHub Pages (step 7 entirely absent), and no endpoint produces SBOMs, Nix binary artifacts, or Docker images as release artifacts (step 4). The AIBOM itself is only reachable via the separate repos/:id/aibom endpoint; the prepare response embeds just the spec_registry manifest section."
          },
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "        // In a real release flow the agent would create a version-bump commit\n        // and push it first; here we open the MR against the current branch.",
            "detail": "The release MR (step 5) is hollow: create_mr stores a MergeRequest with a title but performs no version bump in Cargo.toml, no changelog file update, and no AIBOM snapshot — the three contents the spec requires the release MR to contain. The code comment itself concedes the flow is not the real one."
          }
        ]
      },
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
      },
      {
        "id": "4a0c06c902b41dd1a2000ebe",
        "decision": "gap",
        "requirement_quote": "The version is derived from git tags at build time, not hardcoded in `Cargo.toml`. The Nix flake or build script injects the version:\n- Tagged commit -> exact version (e.g., `1.2.3`)\n- Untagged commit -> dev version (e.g., `1.3.0-dev.42+abc123`)",
        "detail": "The version is in fact hardcoded in Cargo.toml and never derived from git tags. The workspace package version is the literal '0.1.0' and every crate inherits it; the gyre-server build script only builds the web assets and performs no version injection; the Nix flake builds the server and CLI with crane without any git-tag-derived version override; and the runtime version surfaces (CLI --version, admin version endpoint) report CARGO_PKG_VERSION. None of the specced injection behaviors (tagged commit -> exact version, untagged -> dev version with commit count and short SHA) exist. Note this section also assumes the Pre-Release Versions computation, which is itself unimplemented (see that heading).",
        "evidence": [
          {
            "file": "Cargo.toml",
            "quote": "[workspace.package]\nversion = \"0.1.0\"",
            "detail": "The version is a hardcoded literal in the workspace manifest, inherited by all crates — exactly what the spec forbids as the source of truth ('not hardcoded in Cargo.toml'). Since no release MR flow ever updates it either (see the Release Process heading), it is both the de facto source and never updated as a record."
          },
          {
            "file": "crates/gyre-server/build.rs",
            "quote": "fn main() {\n    // Watch web source files for changes",
            "detail": "The gyre-server build script was read in full: its only behavior is watching and building the Svelte web assets (npm ci / npm run build, skippable via SKIP_WEB_BUILD). It never reads git tags, never computes a version, and never emits cargo:rustc-env or cargo:rerun-if-changed inputs for version derivation — the 'build script injects the version' mechanism does not exist."
          },
          {
            "file": "flake.nix",
            "quote": "        gyreServer = craneLib.buildPackage (commonArgs // {\n          inherit cargoArtifacts;\n          cargoExtraArgs = \"-p gyre-server\";\n        });",
            "detail": "The Nix flake builds gyre-server and gyre-cli with crane using the manifest version; the flake was read in full and contains no version override, no git tag reading (no self.rev / self.dirtyRev usage), and no dev-version computation — the 'Nix flake injects the version' mechanism does not exist."
          },
          {
            "file": "crates/gyre-server/src/api/admin.rs",
            "quote": "        version: env!(\"CARGO_PKG_VERSION\"),",
            "detail": "The server's runtime version report (admin info endpoint) is the compile-time manifest constant, confirming that every version surface in the product traces to the hardcoded Cargo.toml value rather than to git tags at build time."
          }
        ]
      },
      {
        "id": "ac49c4bf0419ec38e56d5282",
        "decision": "context",
        "requirement_quote": "- **Supply Chain** (`supply-chain.md`): AIBOM is a release artifact. Release tags are Sigstore-signed.\n- **Agent Gates** (`agent-gates.md`): Release MRs can have gate agents (security scan, spec alignment).",
        "detail": "This section is a cross-reference map tying this spec's features to their sibling canonical specs; the substantive requirements live in, and are audited under, their owning sections. Specifically: 'AIBOM is a release artifact' and 'Release tags are Sigstore-signed' are evaluated under the Release Process heading (c467a8f584b42b34991ff696, gap: no signed release tags exist, AIBOM packaging into the release flow is partial); 'Pre-accept validation enforces commit format' is evaluated under Pre-Accept Enforcement (0707840bfbb865b285a8c0a7, gap: opt-in rather than forge-level); 'Version computation, changelog with provenance, and commit validation at push time are only possible because Gyre owns the forge' is rationale for the design, not an implementable requirement; the spec-lifecycle cross-reference ('release gates can verify all spec-drift tasks are resolved') is a conditional capability whose absence is recorded under Release Process step 6. No requirement is uniquely normative in this section, so it is classified context to avoid double-counting.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "cdd27aa428258fd587e513cd",
        "decision": "gap",
        "requirement_quote": "- **Starlight** (Astro) for user-facing documentation.\n- Deployed via **GitHub Pages**.\n- Docs are a first-class deliverable, not an afterthought.",
        "detail": "The spec requires user-facing documentation to be built with Starlight (Astro) and deployed via GitHub Pages. The repository's user-facing documentation is plain Markdown under docs/ (api-reference.md, cli.md, server-config.md, ui.md, etc.) served by nothing; there is no Astro/Starlight project anywhere in the repo (no astro.config.*, no astro/starlight dependency in any package manifest — the only web build is the Svelte dashboard under web/), and there is no GitHub Pages deployment: .github/workflows/ contains exactly two workflows (ci.yml and e2e.yml), neither of which deploys documentation. The markdown docs being extensive satisfies the 'first-class deliverable' spirit only partially; the two concrete named requirements (Starlight toolchain, GitHub Pages deployment) are unimplemented.",
        "evidence": [
          {
            "file": "docs/cli.md",
            "quote": "# Gyre CLI Reference",
            "detail": "User-facing documentation is plain Markdown files under docs/ (cli.md, api-reference.md, server-config.md, ui.md, development.md, agent-protocol.md). Inspected the whole repository for an Astro/Starlight docs site: no astro.config.* exists, and no package.json (root, web/, docker/gyre-agent) declares astro or @astrojs/starlight as a dependency — web/package.json is the Svelte dashboard, not a docs site. No Starlight docs build exists."
          },
          {
            "file": ".github/workflows/e2e.yml",
            "quote": "name: E2E Tests",
            "detail": "The .github/workflows/ directory contains exactly two files: ci.yml and this e2e.yml. Both were read in full; neither contains any GitHub Pages deployment step (no actions/deploy-pages, no peaceiris/actions-gh-pages, no pages-publish job). There is no documentation deployment pipeline of any kind."
          }
        ]
      },
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
      },
      {
        "id": "0707840bfbb865b285a8c0a7",
        "decision": "gap",
        "requirement_quote": "The forge validates commit messages at push time:\n- Every commit in the push must match the conventional format\n- Unknown types are rejected\n- Empty descriptions are rejected\n- Scope is optional but must be lowercase alphanumeric if present",
        "detail": "Four concrete deviations from the specced enforcement contract. (1) The enforcement is opt-in per repository, not forge-level: check_pre_accept_gates returns Ok with no evaluations when a repo has no gates configured, and gates are only registered via the push-gates administration API (set_for_repo); a newly created repo has no conventional-commit gate, so pushes with non-conforming messages to such repos are accepted — the spec's unconditional 'Pushes with non-conforming commit messages are rejected before entering the repo' does not hold. (2) The scope rule 'must be lowercase alphanumeric if present' is not enforced at all: is_conventional only matches parentheses and never validates the scope's content or case ('feat(AUTH): x' or 'feat(My Scope): x' pass the gate). (3) The error contract is not met: the spec requires a clear error in git sideband output naming the offending commit ('remote: [GYRE] Push rejected: commit abc123 has invalid message format ... Got: ...'); the implementation returns an HTTP 403 plain-text body 'push rejected by gate ...' with no commit SHA and no sideband pkt-line framing. (4) The gate's type whitelist accepts 'revert', a type not in the spec's Types table, so the 'unknown types are rejected' rule is broader than the contract. Additionally, the gate runs after receive-pack has processed the packfile and undoes the ref updates on rejection (undo_ref_updates), so rejected commits do briefly enter the object database even though the ref never moves — weaker than 'before entering the repo'. What does work: every commit in the push range is checked, unknown (non-whitelisted) types are rejected, and empty descriptions are rejected.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    if gate_names.is_empty() {\n        return Ok(Vec::new());\n    }",
            "detail": "check_pre_accept_gates skips all validation for repos with no configured gates. Gate names come from state.repo_push_gates.get_for_repo(repo_id); the only production setter is the push-gates administration API (api/push_gates.rs set_for_repo). Repo creation (api/repos.rs) registers no default gates, so a default repo enforces no commit-format validation — the spec's git-protocol-level unconditional enforcement is not implemented."
          },
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "    let types = [\n        \"feat\", \"fix\", \"docs\", \"style\", \"refactor\", \"perf\", \"test\", \"build\", \"ci\", \"chore\",\n        \"revert\",\n    ];",
            "detail": "The gate's allowed-type whitelist. It rejects types outside the list (unknown types rejected) but includes 'revert', which is not one of the spec's ten types, so the whitelist does not match the spec's Types table."
          },
          {
            "file": "crates/gyre-server/src/pre_accept.rs",
            "quote": "    let description = rest.trim_start_matches(' ');\n    !description.is_empty()",
            "detail": "Empty descriptions are rejected (this part of the contract is met). However, the full is_conventional function was inspected: the only scope handling is locating '(' and ')' in the type part; the scope's content is never validated for lowercase-alphanumeric form, so 'feat(AUTH): x' and 'feat(Any Thing): x' pass — the 'Scope is optional but must be lowercase alphanumeric if present' rule is unenforced."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\n                        \"push rejected by gate '{}': {}\",\n                        rejection.gate_name, rejection.reason\n                    ),",
            "detail": "On gate rejection the handler returns an HTTP 403 with this plain-text body. The spec requires the error in git sideband output ('remote: [GYRE] Push rejected: commit abc123 has invalid message format / Expected: ... / Got: ...'), naming the offending commit SHA; the actual response carries no SHA and is not sideband-formatted (the sideband builder build_feedback_sideband is only used for the acceptance path)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    // Pre-accept gate checks: run after git accepts packfile, undo refs on failure.",
            "detail": "The gates run after run_git_stateless('receive-pack', ...) has executed; on rejection undo_ref_updates rolls the refs back. The ref never points at the rejected commits, but the packfile objects do enter the repository's object database before rejection — weaker than the spec's 'rejected before entering the repo'."
          }
        ]
      },
      {
        "id": "5e69a034fc0a01c1f8222f5e",
        "decision": "gap",
        "requirement_quote": "Each entry includes:\n- **Agent attribution** - which agent produced the change\n- **Task reference** - which task it was part of\n- **Spec reference** - which spec it implements (from commit provenance)\n- **Conventional commit scope** - which area of the codebase",
        "detail": "Changelog generation exists and is real: release_prepare renders a markdown changelog grouped into BREAKING CHANGES / Features / Bug Fixes, each entry carrying the conventional commit scope, the producing agent (name or id, resolved from the agent-commit provenance store) and the task reference. But the fourth required attribution — the spec reference ('which spec it implements (from commit provenance)') — is absent: the provenance record the changelog is built from (AgentCommit) has no spec-reference field (only task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level), and ChangelogEntry has no spec-reference field. There is no lookup anywhere in the release path that would map a commit to the spec it implements, so the generated changelogs cannot contain the specced spec references.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "pub struct ChangelogEntry {\n    pub sha: String,\n    pub commit_type: String,\n    pub scope: Option<String>,\n    pub description: String,\n    pub is_breaking: bool,\n    pub agent_id: Option<String>,\n    pub agent_name: Option<String>,\n    pub task_id: Option<String>,\n}",
            "detail": "The changelog entry type carries scope (conventional commit scope), agent attribution (agent_id/agent_name) and task reference — but no spec-reference field. The spec explicitly requires each entry to include 'Spec reference - which spec it implements (from commit provenance)'; the rendered markdown in render_changelog_markdown correspondingly emits only scope, agent and task attribution."
          },
          {
            "file": "crates/gyre-domain/src/agent_tracking.rs",
            "quote": "pub struct AgentCommit {\n    pub id: Id,\n    pub agent_id: Id,\n    pub repository_id: Id,\n    pub commit_sha: String,\n    pub branch: String,\n    pub timestamp: u64,",
            "detail": "AgentCommit is the commit-provenance record the changelog attribution is resolved from (release_prepare builds prov_map from state.agent_commits.find_by_repo keyed by commit_sha). Its full field list was inspected: id, agent_id, repository_id, commit_sha, branch, timestamp, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation level — there is no spec-reference field, so 'which spec it implements' cannot be derived from commit provenance as the spec requires."
          }
        ]
      },
      {
        "id": "c467a8f584b42b34991ff696",
        "decision": "gap",
        "requirement_quote": "1. **Human or workspace orchestrator triggers release:**\n   ```\n   gyre release prepare\n   ```",
        "detail": "The release flow is only partially implemented. Step 1: the human trigger is real (gyre release prepare CLI command -> client.release_prepare -> POST /api/v1/release/prepare), but the workspace-orchestrator trigger is not: the endpoint is Admin-role-only (NEW-41), agent-token authentication always assigns UserRole::Agent, so no agent persona — including the workspace orchestrator, which is an agent persona per specs/system/platform-model.md — can trigger a release, and there is no MCP tool or other orchestrator code path wrapping release prepare (the only production callers are the CLI command and the admin-only HTTP route). This is an intentional code-versus-spec deviation (NEW-41) with no spec amendment narrowing the spec's actor enumeration, so per the audit rules it is an implementation gap: either the orchestrator trigger must be implemented with scoped authorization, or the spec must be amended through the spec lifecycle. Steps 2-3 are implemented (version computation and changelog generation, see those headings). Step 4 is largely missing: the changelog is produced, and spec-registry manifest state (the AIBOM's spec provenance section) is embedded in the prepare response, but there is no SBOM generation, no signed git tag (no Sigstore signing of release tags anywhere in the release path), no Nix binary artifacts packaged into a release, and no Docker image build. Step 5 is partial: create_mr opens a merge request, but it contains no version bump in Cargo.toml, no updated changelog file, and no AIBOM snapshot — the code's own comment admits 'In a real release flow the agent would create a version-bump commit and push it first; here we open the MR against the current branch'. Step 6 is missing: no release-specific gate wiring exists (no security-agent vulnerability scan 'since last release' and no accountability-agent verification that spec-drift-review tasks are resolved, tied to release MRs). Step 7 is entirely absent: nothing tags the repo on merge, creates a GitHub Release, pushes a Docker image to a registry, or deploys docs to GitHub Pages — the release API surface consists solely of POST /api/v1/release/prepare.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {",
            "detail": "release_prepare rejects every principal without UserRole::Admin ('only Admin role may trigger release preparation'), with an NEW-41 regression test (agent_role_cannot_trigger_release_prepare) asserting an agent-role JWT gets 403. Agent-token authentication always assigns roles: vec![UserRole::Agent] (crates/gyre-server/src/auth.rs, both agent-token arms), and UserRole is exactly {Admin, Developer, Agent, ReadOnly} (crates/gyre-domain/src/user.rs), so no agent persona can hold Admin. The workspace orchestrator is an agent persona, so the spec's 'workspace orchestrator triggers release' actor cannot trigger it. Inspected all callers of release_prepare: only the CLI (crates/gyre-cli/src/main.rs Release variant) and this HTTP route; mcp.rs contains no release tool and no orchestrator path calls it. The Admin-only restriction contradicts the spec's actor enumeration without any spec amendment."
          },
          {
            "file": "specs/system/platform-model.md",
            "quote": "- **Manager Agent** (`development/manager-agent.md`): split into workspace orchestrator + repo orchestrator",
            "detail": "The workspace orchestrator is an agent persona (personas/workspace-orchestrator.md), i.e. it authenticates as an agent with Agent role — the class the NEW-41 admin gate excludes. The spec's step 1 names 'Human or workspace orchestrator' as trigger actors; only the human half is implemented."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/release/prepare\", post(release::release_prepare))",
            "detail": "The complete release API surface is this single route. There is no release finalize/publish/tag endpoint anywhere in the route table (inspected api/mod.rs in full): nothing creates a signed git tag on merge, publishes a GitHub Release with artifacts, pushes a Docker image to a registry, or deploys docs to GitHub Pages (step 7 entirely absent), and no endpoint produces SBOMs, Nix binary artifacts, or Docker images as release artifacts (step 4). The AIBOM itself is only reachable via the separate repos/:id/aibom endpoint; the prepare response embeds just the spec_registry manifest section."
          },
          {
            "file": "crates/gyre-server/src/api/release.rs",
            "quote": "        // In a real release flow the agent would create a version-bump commit\n        // and push it first; here we open the MR against the current branch.",
            "detail": "The release MR (step 5) is hollow: create_mr stores a MergeRequest with a title but performs no version bump in Cargo.toml, no changelog file update, and no AIBOM snapshot — the three contents the spec requires the release MR to contain. The code comment itself concedes the flow is not the real one."
          }
        ]
      },
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
      },
      {
        "id": "4a0c06c902b41dd1a2000ebe",
        "decision": "gap",
        "requirement_quote": "The version is derived from git tags at build time, not hardcoded in `Cargo.toml`. The Nix flake or build script injects the version:\n- Tagged commit -> exact version (e.g., `1.2.3`)\n- Untagged commit -> dev version (e.g., `1.3.0-dev.42+abc123`)",
        "detail": "The version is in fact hardcoded in Cargo.toml and never derived from git tags. The workspace package version is the literal '0.1.0' and every crate inherits it; the gyre-server build script only builds the web assets and performs no version injection; the Nix flake builds the server and CLI with crane without any git-tag-derived version override; and the runtime version surfaces (CLI --version, admin version endpoint) report CARGO_PKG_VERSION. None of the specced injection behaviors (tagged commit -> exact version, untagged -> dev version with commit count and short SHA) exist. Note this section also assumes the Pre-Release Versions computation, which is itself unimplemented (see that heading).",
        "evidence": [
          {
            "file": "Cargo.toml",
            "quote": "[workspace.package]\nversion = \"0.1.0\"",
            "detail": "The version is a hardcoded literal in the workspace manifest, inherited by all crates — exactly what the spec forbids as the source of truth ('not hardcoded in Cargo.toml'). Since no release MR flow ever updates it either (see the Release Process heading), it is both the de facto source and never updated as a record."
          },
          {
            "file": "crates/gyre-server/build.rs",
            "quote": "fn main() {\n    // Watch web source files for changes",
            "detail": "The gyre-server build script was read in full: its only behavior is watching and building the Svelte web assets (npm ci / npm run build, skippable via SKIP_WEB_BUILD). It never reads git tags, never computes a version, and never emits cargo:rustc-env or cargo:rerun-if-changed inputs for version derivation — the 'build script injects the version' mechanism does not exist."
          },
          {
            "file": "flake.nix",
            "quote": "        gyreServer = craneLib.buildPackage (commonArgs // {\n          inherit cargoArtifacts;\n          cargoExtraArgs = \"-p gyre-server\";\n        });",
            "detail": "The Nix flake builds gyre-server and gyre-cli with crane using the manifest version; the flake was read in full and contains no version override, no git tag reading (no self.rev / self.dirtyRev usage), and no dev-version computation — the 'Nix flake injects the version' mechanism does not exist."
          },
          {
            "file": "crates/gyre-server/src/api/admin.rs",
            "quote": "        version: env!(\"CARGO_PKG_VERSION\"),",
            "detail": "The server's runtime version report (admin info endpoint) is the compile-time manifest constant, confirming that every version surface in the product traces to the hardcoded Cargo.toml value rather than to git tags at build time."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "docs/cli.md",
        "sha256": "987ace432f18cd1867c4c93180103bda0edc5bb6cb52f7d76940f9c739bcfa12"
      },
      {
        "file": ".github/workflows/e2e.yml",
        "sha256": "7f37678d597256fd14decb3096c4ccc93d875c5f34fdd13e95fe6d35507f941f"
      },
      {
        "file": ".github/workflows/ci.yml",
        "sha256": "a58652ba210d38bd13b8ae0171c0deb7fad593b916d9e2977078a9962404b97a"
      },
      {
        "file": "web/src/lib/i18n.js",
        "sha256": "d1a71f93f2c459f0f0f325fac5a986eca4d6131deceaad977229dfd09799f1f8"
      },
      {
        "file": "crates/gyre-server/src/pre_accept.rs",
        "sha256": "957d788e4e32cf25197f2665a72efe66349f5b3230eb9e93a22ef0bb4c92539c"
      },
      {
        "file": "crates/gyre-server/src/version_compute.rs",
        "sha256": "1c76a3818f9688d8a222125674a4166de3a997297f4e59d31065011ee0b2d262"
      },
      {
        "file": "crates/gyre-server/src/api/release.rs",
        "sha256": "37a3f824510ea79c2b2a6dd75ba19c70055e78a76689e51cf4199d18fe8511f9"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "fd5036bd1272469fc55a2b304ac10a7db63c95a6d5f546b37a3c2c6c86307149"
      },
      {
        "file": "crates/gyre-domain/src/agent_tracking.rs",
        "sha256": "90874758a93cff40da96b61e6723f3b52ad2ea5a6ebc42e1dd17c4489fab678b"
      },
      {
        "file": "specs/system/platform-model.md",
        "sha256": "78397e1267062ac7eb9f7063dc9eba224695ed6f94336f90a9c4494ebd9dfd49"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "f306cf67464f0902d11b3844f425b91a245e6f8726763b3e9360a026286d4c1d"
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "sha256": "b16446e777779fcd02857cd61e536961e6c3283b2443339bd35935e9f9ec5c4d"
      },
      {
        "file": "Cargo.toml",
        "sha256": "2298e83b544633e37c0ce6f857ee4879924d6f18efcf798ab1a8179e373b1a90"
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
        "file": "crates/gyre-server/src/api/admin.rs",
        "sha256": "4e9d39b02e035233018476d50af6543fff8f88aca74c5804114d1db37302225f"
      }
    ]
  },
  "plan": {
    "audit": "add168988284d6bd092d482d4def617beaa7b6b838c4506077411770fc1216a9",
    "tasks": [
      {
        "key": "docs-site-starlight-pages",
        "title": "User-facing docs as a Starlight (Astro) site deployed to GitHub Pages",
        "covers": [
          "cdd27aa428258fd587e513cd"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §User-Facing Docs: user-facing documentation is built with Starlight (Astro) and deployed via GitHub Pages; docs are a first-class deliverable. (1) Add a real Astro + Starlight project (e.g. docs-site/ with astro.config.mjs, package.json, committed package-lock.json, following the web/ dashboard's npm conventions) whose content source is the EXISTING markdown under docs/ (api-reference.md, cli.md, server-config.md, ui.md, development.md, agent-protocol.md) — the site renders those files as its pages via Starlight content collections (frontmatter supplied by the site layer or a build step), with no forked duplicate of the content: docs/*.md stays the single source, and a page removed from docs/ disappears from the built site. (2) `npm --prefix docs-site install && npm --prefix docs-site run build` succeeds and produces dist/ output containing a rendered HTML page per docs/*.md file. (3) GitHub Pages deployment: .github/workflows/docs.yml builds the site and deploys it via actions/configure-pages + actions/upload-pages-artifact + actions/deploy-pages, triggered by changes to docs/ and docs-site/ on the default branch, and reusable via workflow_dispatch so the release publish flow (release-flow-artifacts-publish) can trigger the docs deployment for a release. (4) The site build is added as a mandatory CI step in .github/workflows/ci.yml (no `|| true` escape) so it cannot silently rot; steps follow existing workflow conventions. Do not move or rewrite the docs/ markdown content itself (other documents and code link to it), and do not alter the spec.",
        "acceptance": [
          "`npm --prefix docs-site run build` exits 0 and the built output contains one rendered HTML page per file currently in docs/ (api-reference, cli, server-config, ui, development, agent-protocol), with the markdown body content present in the rendered HTML.",
          "Removing (or renaming) a file under docs/ and rebuilding changes the built site's page set accordingly — the site is generated from docs/, not from a copied fork of the content.",
          ".github/workflows/docs.yml exists, builds the site, and deploys via actions/deploy-pages (upload-pages-artifact + deploy-pages steps present), triggering on default-branch changes to docs/ and docs-site/ and via workflow_dispatch.",
          "ci.yml contains a mandatory docs-site build step that fails when the site build fails."
        ],
        "negative_probe": "Pre-task the defect is directly observable: no astro.config.* and no astro/starlight dependency exist anywhere in the repo, and no workflow in .github/workflows/ contains a Pages deployment step — so `npm --prefix docs-site run build` fails (no such project) and a search for deploy-pages in .github/workflows/ returns nothing. Probe: build the site and assert the rendered HTML for docs/cli.md exists in the output and the Pages workflow with a deploy-pages step exists; both assertions fail on the current tree. Content-fidelity control: temporarily delete docs/cli.md, rebuild, and assert the cli page is absent from the built site — an implementation that vendors a static copy of the docs content fails this control (the page would survive the deletion).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "ci-invariant-i18n-doc-freshness",
        "title": "CI enforcement for the two named missing invariants: web i18n coverage and doc freshness",
        "covers": [
          "b7c86bc0e72bd8b20c253c5c"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §CI, the pipeline must enforce the named mechanical invariants that are currently absent: i18n coverage and doc freshness. (1) scripts/check-i18n-coverage.sh — real, blocking enforcement of web i18n completeness: every svelte-i18n message key referenced in web/src (t('…') / $_('…') / format lookups) resolves in web/src/locales/en.json, and any additional locale file under web/src/locales/ has full key parity with en.json; violations print the component file and key and exit 1; the unmodified tree exits 0. This exact script and contract are cross-referenced by the speed-backpressure plan's i18n-coverage-web pre-commit hook (it reuses this script; do not fork a second copy — exactly one i18n coverage script exists in scripts/). (2) scripts/check-doc-freshness.sh — real doc-freshness enforcement with code as source of truth, two anchors: (a) every REST route registered in crates/gyre-server/src/api/mod.rs (.route(...) path strings) is documented in docs/api-reference.md; (b) every GYRE_* environment variable read in crates/ (std::env::var(\"GYRE_*\") / env! / option_env!) is documented in docs/server-config.md. Violations name the route/env var and the doc file that is missing it; exit 1. Where the current tree has genuinely undocumented routes or env vars, fix the docs (real content, derived from the code's actual behavior) so the check lands green — no exemption entries in any frozen scripts/*-exemptions.txt. (3) Wire both scripts as mandatory steps in .github/workflows/ci.yml following the existing check-step conventions (e.g. the arch-lint job), never advisory (`|| true` forbidden). Coordination: the architecture plan (task-311) and this task both add steps to ci.yml — additions only, no modification of their steps.",
        "acceptance": [
          "scripts/check-i18n-coverage.sh exits 0 on the unmodified tree and exits 1 naming the component file and missing key when a key referenced by a web/src component's t()/$_() call is removed from web/src/locales/en.json; exactly one i18n coverage script exists in scripts/.",
          "scripts/check-doc-freshness.sh exits 0 on the unmodified tree and exits 1 naming the route when a route registered in api/mod.rs is removed from docs/api-reference.md, and exits 1 naming the variable when a GYRE_* env read is added to crates/ without a docs/server-config.md entry.",
          "ci.yml runs both scripts as mandatory steps; inserting a deliberate violation locally makes the corresponding step fail (verifiable by running the scripts directly).",
          "No entry is added to any existing frozen scripts/*-exemptions.txt."
        ],
        "negative_probe": "Plant `t('definitely.missing.key')` in a web/src component and run scripts/check-i18n-coverage.sh: pre-task no such script exists (the CI pipeline has no i18n coverage enforcement — the only locale is en and nothing checks key usage), so the probe cannot pass; post-task it must exit 1 naming that component file and key, and exit 0 again once the key is defined. Doc-freshness arm: add `std::env::var(\"GYRE_PROBE_UNDOCUMENTED\")` to a crate and run scripts/check-doc-freshness.sh — it must exit 1 naming GYRE_PROBE_UNDOCUMENTED and docs/server-config.md, proving the check tracks code (the source of truth) rather than asserting a symbol exists.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "forge-level-conventional-push-enforcement",
        "title": "Forge-level conventional-commit push enforcement: unconditional validation, scope rule, spec type table, sideband rejection contract",
        "covers": [
          "0707840bfbb865b285a8c0a7"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §Conventional Commits: Forge-Enforced / §Pre-Accept Enforcement, commit-message validation is a git-protocol-level, unconditional pre-accept behavior of the forge, with a precise rejection contract. Four deviations to close, all in production push paths: (1) Forge-level default — the conventional-commit gate is evaluated for EVERY smart-HTTP receive-pack push regardless of repo_push_gates configuration (today check_pre_accept_gates returns Ok early when a repo has no gates configured, so a default repo accepts non-conforming pushes). Implement the default inside the shared gate-evaluation entry (check_pre_accept_gates in git_http.rs) so any other consumer of that entry (notably the source-control plan's jj-export parity task, whose contract predates this change) inherits the identical default and the two delivery routes cannot diverge; repos' configured named gates (task-ref, no-em-dash, etc.) continue to run in addition, and listing 'conventional-commit' via the push-gates admin API remains accepted (idempotent with the now-default behavior). (2) Scope rule — a present scope must be lowercase alphanumeric ([a-z0-9]+): 'feat(AUTH): x' and 'feat(My Scope): x' are rejected, 'feat(auth): x' and 'feat: x' are accepted. (3) Type whitelist matches the spec's ten-type table exactly (feat, fix, docs, style, refactor, perf, test, build, ci, chore) — drop 'revert', which is not in the spec's Types table, from both the whitelist and the error text ('unknown types are rejected' now means exactly the ten). (4) Error contract — the rejection is delivered in the git protocol so the pushing client sees it: per-offender detail (commit SHA + raw subject) must flow through the gate (extend PushContext / the PreAcceptGate port in gyre-ports to carry (sha, message) pairs, updating all gates and tests), and on rejection the receive-pack response reports the ref as rejected (report-status ng) with sideband 'remote:' lines (when the client negotiated side-band-64k) in the spec's exact shape: 'remote: [GYRE] Push rejected: commit {sha} has invalid message format', 'remote: [GYRE] Expected: <type>(<scope>): <description>', 'remote: [GYRE] Got: \"{subject}\"'; the HTTP error body carries the same message. Rejected pushes leave the ref at its pre-push position (existing undo_ref_updates path) and emit the GitPushRejected audit event. Every commit in the push range is still checked; empty descriptions still rejected. Do not change the gate's enforcement for the acceptance path, the other named gates, or the agent-workflow plan's separate agent-principal default-branch push rejection (a distinct check in the same handler — keep both).",
        "acceptance": [
          "A push of a commit with message 'fixed the thing' to a freshly created repo with NO gates configured is rejected: git exits nonzero, the client output contains 'remote: [GYRE] Push rejected: commit {sha} has invalid message format' plus the Expected/Got lines naming the offending commit, and the target ref is unchanged after the push; a GitPushRejected audit event records the rejection.",
          "Scope rule enforced on a default repo: 'feat(AUTH): x' is rejected with the same contract while 'feat(auth): x' on the same repo is accepted (and 'feat: x' without a scope is accepted).",
          "Type table enforced: 'revert: x' is rejected as an unknown type while 'chore: x' is accepted; the rejection text lists exactly the spec's ten types.",
          "Configured named gates (task-ref, no-em-dash) still evaluate as before on top of the forge-level default; pushes with conventional messages to default repos continue to succeed unchanged (no fail-closed regression).",
          "The PushContext/PreAcceptGate port change carries (sha, message) pairs and all existing gates and their tests are updated; no duplicated validation logic between the receive-pack path and the shared gate-evaluation entry."
        ],
        "negative_probe": "Create a fresh repo (no push gates configured), push a commit whose message is 'fixed the thing' via the git CLI over smart HTTP, and observe the push result: pre-task the push SUCCEEDS (the ref advances and the push is audited as accepted — the real defect: check_pre_accept_gates returns Ok early for unconfigured repos, so the spec's unconditional git-protocol-level enforcement does not exist), so the probe's assertion (nonzero exit + 'remote: [GYRE] Push rejected: commit {sha}...' in the output + unchanged ref) fails today. Controls that kill partial implementations: on the same fresh repo, 'feat(AUTH): x' must be rejected while 'feat(auth): x' succeeds (an implementation that rejects everything or skips the scope rule fails one arm), and 'revert: x' must be rejected while 'chore: x' succeeds (whitelist still broader than the spec's table fails this arm).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "changelog-spec-reference-provenance",
        "title": "Changelog entries carry the spec reference resolved from commit provenance",
        "covers": [
          "5e69a034fc0a01c1f8222f5e"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §Changelog Generation, each changelog entry must include the spec reference — 'which spec it implements (from commit provenance)' — alongside the already-implemented agent attribution, task reference, and conventional-commit scope. (1) ChangelogEntry (crates/gyre-server/src/version_compute.rs) gains a spec_ref: Option<String> field. (2) release_prepare (crates/gyre-server/src/api/release.rs) resolves it per commit from real provenance state, preferring the direct record and falling back to the task join: AgentCommit.spec_ref when that field exists and is set (the field is delivered by the agent-gates plan's auto-mr-pinned-spec-ref task, planned but not yet published — consume it if landed; do NOT fork a second agent_commits spec field if it is absent), otherwise resolve via the existing provenance chain already loaded in the handler: agent_commits row's task_id -> state.tasks lookup -> task.spec_ref (the pinned 'path@sha'). Commits without provenance (human pushes with no agent_commits row, or tasks without a pinned spec_ref) render no spec reference — never a fabricated value. (3) render_changelog_markdown emits the spec reference on each entry that has one, in the spec's example form ('spec: system/identity-security.md@abc123'), in the BREAKING CHANGES / Features / Bug Fixes sections alike; the ReleasePrepareResponse sections carry the field so API consumers see it too. The resolution is a real query against the provenance and task stores on every prepare — not a cached or hardcoded mapping.",
        "acceptance": [
          "On a repo whose commits have provenance (an agent_commits row with a task_id whose task carries spec_ref 'system/foo.md@abc123'), POST /api/v1/release/prepare produces a changelog whose markdown for that commit contains 'spec: system/foo.md@abc123', and the response's ChangelogEntry carries spec_ref = Some('system/foo.md@abc123').",
          "A commit in the same range pushed by a human (no agent_commits row) yields an entry with spec_ref = None and renders without a spec reference — no fabricated or defaulted value (mirrors the fabricated-scope discipline).",
          "The rendered markdown keeps the existing agent attribution, task reference and scope on every entry (no regression), and the spec reference appears in all three sections (breaking/features/fixes) for entries that have one."
        ],
        "negative_probe": "Seed a repo with a tag, push a conventional commit attributed to an agent session whose task pins spec_ref 'system/foo.md@abc123' (agent_commits row with that task_id), run release prepare, and assert the generated markdown contains 'spec: system/foo.md@abc123' on that entry: pre-task the probe fails because ChangelogEntry has no spec_ref field, AgentCommit has no spec-reference field, and render_changelog_markdown emits only scope/agent/task — grep for 'spec:' in the generated changelog returns nothing (the real defect: the fourth required attribution cannot be produced by any code path). Control: a human-pushed commit in the same range must render WITHOUT any spec reference — an implementation that fabricates a default spec string fails this control.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "release-flow-artifacts-publish",
        "title": "Complete the release process: workspace-orchestrator trigger, real release artifacts (AIBOM/SBOM/Nix binaries/Docker), version-bump release MR, release gates, and tag+publish on merge",
        "covers": [
          "c467a8f584b42b34991ff696"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §Release Process, close the remaining steps of the flow on top of the implemented steps 1(human trigger)/2(version)/3(changelog). Step 1, orchestrator half (review finding c467a8f584b42b34991ff696:release-trigger-orchestrator-path, disposition (a) — scoped mechanism, not a blanket Agent-role grant): extend release_prepare's authorization to accept EITHER UserRole::Admin OR a workspace-orchestrator principal — a caller whose JWT carries orchestrator_type=workspace_work orchestrator scope (workspace_orchestrator_scope pattern, mcp.rs) with a signed workspace_id AND tenant matching the target repo's actual workspace — evaluated per request against the loaded repo record, never a blanket Agent-role grant; a worker-agent JWT, an agent-role JWT without orchestrator scope, and a workspace orchestrator scoped to a different workspace all still get 403 (preserving NEW-41's intent that arbitrary agents cannot trigger releases). Add the MCP tool gyre_release_prepare (workspace-orchestrator tier, same scope validation as gyre_spawn_repo_orchestrator et al.) so the orchestrator persona has a real trigger path; it performs writes, so it MUST be added to the needs_write RBAC matches! list in mcp.rs (scripts/check-mcp-write-tools.sh derives membership from handler effects). Update the NEW-41 regression test agent_role_cannot_trigger_release_prepare to the narrowed contract (unscoped/differently-scoped agents still 403). Step 4 — release artifacts, durably persisted in a NEW port-backed release store (gyre-ports trait + adapter, next unused migration sequence number, portable SQL for both SQLite and PostgreSQL, mem-adapter contract parity; never an AppState Arc<Mutex<HashMap>>): (a) changelog markdown; (b) AIBOM wired into the release flow from build_release_aibom — PARTITION with the supply-chain plan's release-artifacts-aibom-sbom task (that task is accountable for the supply-chain.md acceptance criteria; this task owns the release-process wiring: implement the release-flow integration if it has not landed, consume it if it has; do not duplicate the artifact builders); (c) SBOM in SPDX format generated from the real dependency lockfiles at the release ref (Cargo.lock / package-lock.json read via read_git_file at the release SHA — no synthetic dependency lists; a ref with no lockfile yields an empty inventory with a stated reason); (d) binary artifacts from the Nix build: run the existing root flake outputs (gyre-server, gyre-cli) at the release ref and record output hashes durably; where Nix is unavailable in the environment, record an explicit not-built status with reason (observable, never silent success); (e) Docker image built from the existing Nix flake docker image output — share the existing flake build definitions (including docker/gyre-agent/flake.nix outputs where the supply-chain SLSA task shares the path), do not fork new build definitions. Step 5 — the release MR is real: prepare creates a release branch carrying an actual version-bump commit (workspace Cargo.toml version set to next_version, the changelog file updated with the generated changelog, and an AIBOM snapshot file committed — the three contents the spec requires), pushed server-side via the git_ops port, and opens the MR against the default branch linked to the release record (replacing today's hollow 'open the MR against the current branch' path). Step 6 — release gates when configured (workspace/repo release-gate configuration, port-backed): the release MR passes through gate evaluation on merge: (a) security gate — a real vulnerability scan of what changed since the last release: dependency delta computed from the SBOM at the previous tag vs the release ref, each changed dependency checked against advisory data (OSV API query with a bounded timeout, honoring the unbounded-external-http invariant; or a configured advisory source), and a finding fails the gate naming the dependency; (b) accountability gate — a real query for open spec-drift-review tasks bound to the repo (the spec-lifecycle machinery's task class), and any unresolved one fails the gate naming the task. Enforcement, not audit: a failed release gate blocks the release MR merge. Step 7 — on merge of the release MR (detected via the release-record link in the merge processor): the forge creates the tag v{major}.{minor}.{patch} on the merge commit via the git_ops port and signs it with Sigstore keyless signing (the existing sigstore.rs sign_commit_keyless chain) using the releasing principal's OIDC identity (the trigger principal recorded in the release record), with the signature recorded durably and verifiable via the existing verification path; publishes the release: GitHub Release creation with the artifacts via configured GitHub API credentials (real HTTP calls, bounded timeout; unconfigured targets produce an explicit 'skipped: not configured' status in the release record visible via the API — never a silent success or a fake publication), Docker image push to the configured registry (real push when configured, explicit status otherwise), and docs deployment by triggering the docs-site Pages deployment (docs.yml workflow_dispatch from the docs-site-starlight-pages task) for the release. Any new REST routes are registered in api/mod.rs with ABAC RouteResourceMapping entries in abac_middleware.rs (no exemption-file entries); all task-labeled product-surface commits are recorded in the task's commits: frontmatter. Do not alter the spec; the NEW-41 admin path keeps working unchanged alongside the new scoped orchestrator path.",
        "acceptance": [
          "A workspace-orchestrator JWT scoped to workspace W (signed workspace_id/tenant claims) triggers POST /api/v1/release/prepare for a repo in W successfully (and the new MCP tool gyre_release_prepare works for the same principal); a worker-agent JWT, an agent-role JWT without orchestrator scope, and a workspace-orchestrator JWT scoped to a different workspace all get 403; the Admin path still works.",
          "Preparing a release on a repo whose release-ref tree contains a Cargo.lock durably records release artifacts addressable from the release: the changelog, the AIBOM (same provenance content as the on-demand GET /repos/:id/aibom?format=release), and an SBOM whose package list matches the Cargo.lock at that ref (mutating the lockfile and re-preparing yields a changed SBOM); Nix binary/Docker build results are recorded with output hashes, or an explicit not-built status with reason when Nix is unavailable.",
          "The release MR's source branch HEAD is a real version-bump commit whose tree contains the workspace Cargo.toml at next_version, the updated changelog file, and the AIBOM snapshot — no longer an MR opened against the current branch with none of the three contents.",
          "With release gates configured and an open spec-drift-review task bound to the repo, the release MR's merge is blocked with an error naming the task; resolving (closing) the task unblocks the merge. With a dependency in the release delta carrying a known advisory (per the advisory source), the security gate fails the release MR naming the dependency.",
          "On merge of the release MR, the repo gains the tag v{major}.{minor}.{patch} on the merge commit, signed via Sigstore with the releasing principal's OIDC identity (signature verifiable through the existing verification path); with publish targets configured (e.g., a recorded endpoint), the forge performs the GitHub Release creation call with the release artifacts and the registry push; with no targets configured, the release record exposes an explicit 'publish skipped: not configured' status; the docs deployment is triggered for the release.",
          "Release records and artifacts persist through the port-backed store (restart-safe); every new route has an ABAC resolver entry; the new MCP tool is in the needs_write gate."
        ],
        "negative_probe": "Primary probe (the review finding's defect, observable today): mint a workspace-orchestrator JWT scoped to workspace W (the test mint helper exists in mcp.rs tests) and POST /api/v1/release/prepare for a repo in W — pre-task the call returns 403 'only Admin role may trigger release preparation' (release_prepare rejects every principal without Admin; agent tokens always carry Agent role; the NEW-41 regression test deliberately asserts this), so the probe's success arm fails today; after implementation the same call succeeds while the same JWT against a repo in a DIFFERENT workspace and a worker-agent JWT both still return 403 (an implementation that grants all Agent-role principals fails the control arms). Secondary probe (the step-5 hollowness, observable today): prepare with create_mr=true and inspect the MR — pre-task no version-bump commit exists (the code comment concedes the MR is opened against the current branch with no Cargo.toml bump, no changelog file, no AIBOM snapshot), so asserting that the MR branch HEAD's tree contains Cargo.toml at next_version plus the changelog file plus the AIBOM snapshot fails on the current tree.",
        "dependencies": [
          "docs-site-starlight-pages"
        ],
        "existing_task": null
      },
      {
        "key": "prerelease-dev-version",
        "title": "Pre-release dev versions: {next}-dev.{count} computed from commits since the last release, never tagged",
        "covers": [
          "3d8bcb425b5e801f88e7fb66"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §Pre-Release Versions, development builds between releases carry an automatically computed pre-release version: 'v1.3.0-dev.42 — 42 commits since last release; computed automatically, not tagged; available via gyre --version for debugging'. (1) Implement compute_dev_version in crates/gyre-server/src/version_compute.rs as a real function over real git state: given the repo path and branch, resolve the latest semver tag (existing latest_semver_tag) and the commit range (existing commits_since), compute the next version per the bump rules (parse_conventional/type_bump fold — a range with a feat yields the minor-next base, fix/perf the patch-next base, breaking the major-next, only no-bump types keep the current version as base), count the commits since the tag (no tag: base v0.0.0 with the full commit count), and produce '{next}-dev.{count}' matching the spec's example semantics (v1.2.3 + a feat + 41 other commits -> v1.3.0-dev.42). (2) The computation is surfaced as a real consumer: the release-prepare response gains the analyzed branch's current dev version (computed on every prepare from the actual repository — no caching, no hardcoding), giving operators the debugging affordance server-side; the build-time injection task (build-time-version-injection) consumes this same function so gyre --version reports it. (3) Never tagged: no code path creates a git tag for a dev version — the dev version is a computed string only (the release tag is created solely by the release flow's merge step); assert in tests that preparing/inspecting a dev version leaves the repo's tag list unchanged.",
        "acceptance": [
          "On a repo with tag v1.2.3 and a commit range since it containing one feat and 41 other commits, the dev version computes 'v1.3.0-dev.42' (minor-next base per the bump rules); with only fix commits it computes the patch-next base; with only docs/chore commits it keeps v1.2.3 as the base with the commit count.",
          "A repo with no semver tag computes a v0.0.0-dev.{total commits} dev version rather than erroring.",
          "POST /api/v1/release/prepare reports the branch's current dev version in the response, derived from the real repository state on each call.",
          "Computing the dev version creates no git tag: the repo's tag list is byte-identical before and after (dev versions are computed, never tagged)."
        ],
        "negative_probe": "Create a repo, tag it v1.2.3, then add exactly 42 commits since the tag of which one is a feat (e.g. 'feat: x') and the rest no-bump types, then request the dev version (release-prepare response / the computation over the real repo): pre-task the probe fails because no code path computes a dev pre-release version at all — version_compute has only tag-stripping logic for reading tags, the prepare response has no dev-version field, and gyre --version prints the manifest constant — so the assertion that the computed value equals 'v1.3.0-dev.42' (and not '0.1.0', not 'v1.2.3', not a tag) has nothing to observe. Control: run git tag --list after the computation and assert it is unchanged — an implementation that realizes the dev version by creating a -dev tag fails this control.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "build-time-version-injection",
        "title": "Version derived from git tags at build time (build script + Nix flake injection), Cargo.toml as record only",
        "covers": [
          "4a0c06c902b41dd1a2000ebe"
        ],
        "required_behavior": "Per specs/development/ci-docs-release.md §Version in Code, the version is derived from git tags at build time, not hardcoded in Cargo.toml; the Nix flake or build script injects it: tagged commit -> exact version (e.g. 1.2.3); untagged commit -> dev version (e.g. 1.3.0-dev.42+abc123). (1) Build-script injection: add version derivation to the gyre-cli and gyre-server build scripts (gyre-server's existing build.rs gains this alongside its web-asset build — additive, web build behavior and SKIP_WEB_BUILD unchanged; gyre-cli gains a build.rs): resolve the version from the source checkout's git state — exact semver tag on HEAD -> that version; otherwise the dev version computed by the prerelease-dev-version task's logic (next-version base + dev.{count}) suffixed with '+' + the short commit SHA — and emit cargo:rustc-env=GYRE_BUILD_VERSION={value} (with cargo:rerun-if-changed inputs so version changes rebuild). Precedence: an explicit GYRE_BUILD_VERSION environment override (the Nix path) wins over git-derived; when neither git metadata nor an override is available (e.g. a source tarball), fall back to the manifest version suffixed with an explicit marker (e.g. '+nogit') so the degradation is observable, never a silent hardcoded claim. (2) Runtime surfaces consume the injected value instead of CARGO_PKG_VERSION: the CLI's clap version (gyre --version), the admin info endpoint (api/admin.rs), and the /api/v1/version route (currently the literal \"0.1.0\", api/version.rs) all report GYRE_BUILD_VERSION. (3) Nix flake injection: flake.nix passes a git-derived version into the crane builds of gyre-server and gyre-cli (from the flake's self.rev / self.dirtyRev resolution, or by exporting the tag-derived version), so flake-built binaries carry the git-derived version. (4) Cargo.toml's version remains a record updated only by the release MR (release-flow-artifacts-publish step 5) — never the source of truth; after this task, the served/printed version of a build from an untagged checkout differs from the manifest literal. Coordination: the speed-backpressure plan (assigned) also edits gyre-server/build.rs and flake.nix — keep changes additive and independent. Do not alter the spec.",
        "acceptance": [
          "Building gyre-cli from a checkout whose HEAD carries tag v1.2.3 yields `gyre --version` printing 1.2.3; building from an untagged HEAD N commits past v1.2.3 with short SHA abc1234 prints the dev form 1.3.0-dev.N+abc1234 (per the bump rules for the range) — both differ from the manifest literal 0.1.0 that every build prints today.",
          "GYRE_BUILD_VERSION=9.9.9 in the environment at build time is honored by the build script (the Nix injection path is exercisable without Nix), and a build with no git metadata and no override produces a value carrying the explicit nogit marker rather than a plain manifest claim.",
          "The admin info endpoint and GET /api/v1/version report the injected build version (no longer the literal \"0.1.0\"), and their existing tests are updated to the injected-value contract.",
          "flake.nix feeds a git-derived version into the crane package builds of gyre-server and gyre-cli (self.rev/dirtyRev or tag-derived), so flake-built binaries carry the git-derived version; the web-asset build behavior of gyre-server's build.rs (incl. SKIP_WEB_BUILD) is unchanged."
        ],
        "negative_probe": "Tag a test checkout's HEAD as v2.5.1, cargo build -p gyre-cli, and run the built `gyre --version`: pre-task it prints 'gyre 0.1.0' (the clap version attribute resolves to the hardcoded workspace manifest constant regardless of any tag — the real defect: no build script or flake reads git tags, and every runtime version surface traces to CARGO_PKG_VERSION), so the assertion that it prints 2.5.1 fails today; then remove the tag, rebuild from the untagged HEAD, and assert the dev form {next}-dev.{count}+{shortsha} — an implementation that only special-cases tags (or that copies the manifest version) fails the second arm. Control: GYRE_BUILD_VERSION=9.9.9 cargo build must yield 9.9.9, proving the injection path the Nix flake uses is real rather than git-only.",
        "dependencies": [
          "prerelease-dev-version"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "docs-site-starlight-pages": "task-314",
    "ci-invariant-i18n-doc-freshness": "task-315",
    "forge-level-conventional-push-enforcement": "task-316",
    "changelog-spec-reference-provenance": "task-317",
    "release-flow-artifacts-publish": "task-318",
    "prerelease-dev-version": "task-319",
    "build-time-version-injection": "task-320",
    "_publication": "task-321",
    "_base": "a2bd3eb044f5418cc0b01b5a06f635b164c1a2bb"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
