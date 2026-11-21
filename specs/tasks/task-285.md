---
title: "Publish reviewed spec backlog for specs/system/dependency-graph.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/dependency-graph.md",
  "plan_id": "1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "ed9f34316476f290bbed88c9764c9ac6d14be0a46ea75ea0400818fef323fdcc",
    "sections": [
      {
        "id": "129c28d0ec208eaddbf6377f",
        "decision": "context",
        "requirement_quote": "# Cross-Repo Dependency Graph",
        "detail": "Document title heading. No implementable requirement; names the capability whose requirements live in the child sections.",
        "evidence": []
      },
      {
        "id": "875be00cf9159a038eeb3e33",
        "decision": "context",
        "requirement_quote": "At enterprise scale, repos don't exist in isolation. Repo A imports a crate from Repo B. Repo C calls an API defined by Repo D.",
        "detail": "Problem statement / rationale. Describes motivations (invisible dependencies, blast radius, version drift, cross-repo impact analysis, orchestrator prioritization data) that the rest of the spec turns into requirements; nothing here is independently implementable.",
        "evidence": []
      },
      {
        "id": "c725cb1207c5bdc61a152454",
        "decision": "satisfied",
        "requirement_quote": "Gyre maintains a **tenant-wide dependency graph** across all repos, detected automatically and enforced mechanically. The forge sees every push across every repo - it uses this visibility to build, maintain, and enforce the graph.",
        "detail": "The capability thesis is real at this base: DependencyEdge/BreakingChange/DependencyPolicy state is persisted through the store! macro (SQLite/Postgres adapters selected when a DB is configured, mem fallback otherwise), auto-detection and breaking-change detection run from the git post-receive push path, and enforcement is mechanical (Block policy fails the merge queue entry while breaking changes are unacknowledged). The specific detection-form shortfalls are accounted under their child sections (§1 Parse Dependency Files, §3 Parse API Contracts) and are not re-counted here.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "        dependencies: store!(\n            dyn DependencyRepository,\n            mem::MemDependencyRepository::default()\n        ),",
            "detail": "AppState wires the dependency graph through the store! macro, which selects the SQLite or Postgres DependencyRepository implementation when a database is configured (task-199 closed the earlier in-memory-only hollow wiring noted in the coverage matrix)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            // Dependency graph: auto-detect Cargo.toml path deps (M22.4).\n            detect_dependencies_on_push(\n                &state_clone,\n                &repo_id_clone,\n                &repo_path_clone,\n                &update.new_sha,\n            )\n            .await;",
            "detail": "Push visibility is real: the post-receive processing loop invokes dependency auto-detection (and breaking-change detection) on pushes to the repo's default branch."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                gyre_domain::BreakingChangeBehavior::Block => {",
            "detail": "Mechanical enforcement exists: when the workspace dependency policy is Block and the repo has unacknowledged breaking changes, the merge queue entry is failed with reason 'breaking_change_policy' instead of merging."
          }
        ]
      },
      {
        "id": "ab77890c60e6cba135046761",
        "decision": "satisfied",
        "requirement_quote": "| `manual` | Explicitly declared via API/CLI | Dependencies that can't be auto-detected |",
        "detail": "All five dependency types (code/spec/api/schema/manual) exist in gyre-domain and every type has a real detection or declaration path wired into the graph: code via Cargo.toml/package.json/go.mod/pyproject.toml parsers, spec via specs/manifest.yaml links, api via OpenAPI/mcp.json detection, schema via proto imports, manual via POST /api/v1/repos/{id}/dependencies and `gyre deps add`. The DetectionMethod enum covers all eight specced methods plus PyprojectToml, matching the spec's own dep-type table which lists pyproject.toml as a code source.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/sqlite/dependency.rs",
            "quote": "        DependencyType::Manual => \"manual\",",
            "detail": "The persistence layer round-trips all five specced DependencyType variants (Code/Spec/Api/Schema/Manual), and the adjacent detection-method mapping covers CargoToml, PackageJson, GoMod, PyprojectToml, ManifestLink, OpenApiRef, ProtoImport, McpToolRef, Manual."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "/// POST /api/v1/repos/{id}/dependencies\n/// Manually declare a dependency from this repo to another.",
            "detail": "Manual dependencies are declarable via the API handler add_dependency, which validates both repos exist, rejects self-dependency, and persists an edge with DetectionMethod::Manual."
          },
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "    /// Add a manual dependency from this repo to another",
            "detail": "Manual dependencies are also declarable via the CLI (`gyre deps add --target ... --type ...`), satisfying the 'via API/CLI' row of the type table."
          }
        ]
      },
      {
        "id": "4390a1b94a03e318854cbbec",
        "decision": "satisfied",
        "requirement_quote": "pub struct DependencyEdge {\n    pub id: Id,\n    pub source_repo_id: Id,\n    pub target_repo_id: Id,",
        "detail": "The production DependencyEdge entity carries every specced field (id, source/target repo ids, dependency_type, source_artifact, target_artifact, version_pinned, target_version_current, version_drift, detection_method, detected_at, last_verified_at, status), the DependencyType/DetectionMethod/DependencyStatus enums match the spec variants (Active/Stale/Breaking/Orphaned), and the entity is durably persisted: the store! macro selects the Diesel SQLite/Postgres DependencyRepository implementations, with migrations creating dependency_edges, breaking_changes, and dependency_policies tables. This closes the hollow in-memory wiring the 2026-09-29 coverage audit recorded for this section (task-199).",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/dependency.rs",
            "quote": "pub struct DependencyEdge {\n    pub id: Id,\n    /// The repo that depends on the target.\n    pub source_repo_id: Id,",
            "detail": "The domain entity struct contains all specced fields including version_pinned, target_version_current, version_drift, detection_method, detected_at, last_verified_at, and status."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "        dependencies: store!(\n            dyn DependencyRepository,\n            mem::MemDependencyRepository::default()\n        ),",
            "detail": "AppState resolves the DependencyRepository through the store! macro: Postgres or SQLite storage when a DB is configured, mem only as the pure-in-memory fallback — the graph survives restart in DB-backed deployments."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/dependency.rs",
            "quote": "use gyre_ports::DependencyRepository;",
            "detail": "A real Diesel-backed SQLite adapter implements the DependencyRepository port against the dependency_edges table (schema.rs:589)."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-10-08-000061_dep_breaking_policies/up.sql",
            "quote": "CREATE TABLE IF NOT EXISTS breaking_changes (",
            "detail": "Durable migrations exist for the graph's durable state: dependency_edges (2026-03-23-000007 / 2026-04-09-000049), breaking_changes and dependency_policies (this migration)."
          }
        ]
      },
      {
        "id": "12ff1c4b10f99712ceaf0c8e",
        "decision": "gap",
        "requirement_quote": "On every push to any repo, the forge:",
        "detail": "Umbrella section whose four numbered steps are accounted under their child headings (§1 gap: Cargo.toml git-reference extraction missing; §2 satisfied; §3 gap: OpenAPI/mcp.json detection is a substring scan and only root-level contract files are read; §4 satisfied) — the child gaps are not re-counted here. The parent's own content is the push-time trigger: detection is genuinely wired into the git post-receive path, but only for ref updates on the repo's default branch (filtered by `u.refname == default_ref`) and only when a dependency-related file changed in the push. Pushes to non-default branches do not touch the graph (the graph does update when such branches later land on the default branch), and the extraction forms named by the children are only partially implemented, so the parent's statement is not fully true at this base.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            // Dependency graph: auto-detect Cargo.toml path deps (M22.4).\n            detect_dependencies_on_push(\n                &state_clone,\n                &repo_id_clone,\n                &repo_path_clone,\n                &update.new_sha,\n            )\n            .await;",
            "detail": "The trigger wiring is real: this call site sits inside `for update in applied_updates.iter().filter(|u| u.refname == default_ref)` — the post-receive loop scoped to default-branch ref updates — and detect_dependencies_on_push early-returns unless a dependency file changed in the push."
          }
        ]
      },
      {
        "id": "e07d32e2ebc10c9a36f6343c",
        "decision": "gap",
        "requirement_quote": "Cargo.toml       -> extract [dependencies] with git/path references to other Gyre repos",
        "detail": "Per the cycle-3 review correction (cargo-git-references-unparsed), this section is a gap: the spec names BOTH git and path reference forms as the Cargo.toml extraction contract, and the implementation parses only `path = \"...\"` inline-table values (detect_cargo_path_deps / extract_path_value). A `[dependencies]` entry like `repo-b = { git = \"https://gyre.example.com/<ws>/repo-b.git\" }` — exactly the Gyre-to-Gyre reference the spec names — produces no DependencyEdge; no code in the Cargo detection path handles `git =` (or `rev =`) keys. On the other three files, whose spec wordings are generic ('dependencies pointing to Gyre-hosted packages', 'require directives referencing Gyre modules', 'dependencies referencing Gyre packages'): go.mod is faithful under the generic wording because module paths are matched by last path segment, which covers both bare names and Gyre-URL module paths; package.json and pyproject.toml handle only `file:`/`workspace:` and path/`file:` forms respectively — a Gyre-hosted package referenced by git URL (e.g. `\"repo-b\": \"git+https://...\"` or `pkg @ git+https://...`) is a natural reading of the generic wording and is not detected. Secondary: only root-level Cargo.toml is read (`read_git_file(..., \"Cargo.toml\")`) although the changed-file trigger matches nested `*/Cargo.toml` paths, so path deps declared in workspace-member manifests are never detected. Matching against known tenant repos (state.repos.list()) and ignoring external registry deps are both correctly implemented for the forms that are parsed.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "fn extract_path_value(line: &str) -> Option<String> {\n    let path_idx = line.find(\"path\")?;",
            "detail": "The only Cargo.toml dependency-reference extractor: it locates the literal `path` key in a dependency line and reads its quoted value. There is no corresponding git= handling — grep for git-reference parsing in the Cargo detection path finds nothing, so `repo-b = { git = \"https://gyre.example.com/ws/repo-b.git\" }` yields no edge."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let path_deps = detect_cargo_path_deps(&content);\n            for path_dep in &path_deps {",
            "detail": "The Cargo.toml branch of detect_dependencies_on_push feeds only detect_cargo_path_deps output into DetectedEdge (DependencyType::Code, DetectionMethod::CargoToml); git-reference values never enter the flow."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    if let Some(path) = ver_str.strip_prefix(\"file:\") {",
            "detail": "package.json detection handles only `file:` and `workspace:` reference forms; a Gyre-hosted package referenced by git URL is not extracted, though the spec's wording ('dependencies pointing to Gyre-hosted packages') naturally covers it."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        // Single-line: `require module/path v1.0.0`",
            "detail": "go.mod require (and supplementary local replace) directives are parsed and matched by last path segment, which covers URL-form Gyre module paths — this file's reading is faithful to its generic spec wording."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        // PEP 621: [project] dependencies = [...] with `pkg @ file:///path`",
            "detail": "pyproject.toml detection handles Poetry `{path = ...}` and PEP 621 `@ file:` forms only; PEP 508 direct git references (`pkg @ git+https://...`) to Gyre packages are not extracted."
          }
        ]
      },
      {
        "id": "25c591a75522545b22e1c001",
        "decision": "satisfied",
        "requirement_quote": "From `specs/manifest.yaml`, extract all cross-repo spec links (already covered in `spec-links.md`). These become `spec` type edges in the dependency graph.",
        "detail": "detect_manifest_spec_links parses specs/manifest.yaml with serde_yaml into the SpecManifest struct, keeps links whose target starts with `@`, resolves cross-repo and cross-workspace targets to repo names (same-repo targets skipped), and the push pipeline turns each matched target into a DependencyType::Spec / DetectionMethod::ManifestLink edge after matching the repo name against known tenant repos.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            if !link.target.starts_with('@') {\n                continue;\n            }",
            "detail": "Cross-repo spec-link extraction filters manifest links to cross-repo/cross-workspace `@`-prefixed targets and resolves their repo_name via parse_cross_workspace_target."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                            dep_type: DependencyType::Spec,\n                            method: DetectionMethod::ManifestLink,",
            "detail": "The manifest branch of detect_dependencies_on_push creates spec-type edges for each detected cross-repo link whose target repo exists in the tenant."
          }
        ]
      },
      {
        "id": "8244b7d9d6d350af456c6492",
        "decision": "gap",
        "requirement_quote": "openapi.yaml / swagger.json  -> extract server URLs matching other Gyre repos",
        "detail": "Only one of the three contract forms is implemented as specified. Proto import detection is real structured parsing (`import \"...\"` directives, first path segment matched against known repo names, all .proto files in the tree scanned via git ls-tree). But OpenAPI/Swagger and mcp.json detection are coarse whole-document substring scans — `content.contains(repo_name)` — not the specced extraction of server URLs from openapi.yaml/swagger.json or tool references from mcp.json; any mention of a repo name anywhere in the document (e.g. in a description) creates an Api edge, which both over-matches beyond server URLs and never inspects the servers[] structure the spec names. Additionally, only the three root-level file names are read (`openapi.yaml`, `openapi.yml`, `swagger.json` at the repo root) even though the change trigger matches nested paths — the spec's own entity example uses `api/openapi.yaml` as a source_artifact — so nested API contracts are never parsed. The code's own comment ('Best-effort: many API contracts don't explicitly reference repo names') acknowledges the divergence; a code comment is not a spec amendment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    for repo_name in known_repos {\n        // Match repo names in server URLs, $ref paths, and other string values.\n        if content.contains(repo_name) {",
            "detail": "detect_openapi_refs: instead of parsing servers[].url (or $ref values) from the YAML/JSON document, it scans the raw document text for each known repo name — a substring stand-in for the specced server-URL extraction."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        for api_file in &[\"openapi.yaml\", \"openapi.yml\", \"swagger.json\"] {",
            "detail": "Only the three root-level contract files are read via read_git_file, while the has_openapi trigger also matches nested `*/openapi.yaml` etc. — nested contracts (the spec's own `api/openapi.yaml` example artifact) are never parsed."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        if let Some(rest) = trimmed.strip_prefix(\"import \") {",
            "detail": "Proto detection is the satisfied third of the section: structured import-directive parsing with first-segment matching against known repos, scanned across all .proto files at the pushed SHA."
          }
        ]
      },
      {
        "id": "26e66ab12d5a4bd37d82f499",
        "decision": "satisfied",
        "requirement_quote": "- New dependency detected -> create edge, log activity event",
        "detail": "reconcile_dependencies implements all three specced transitions against the real store: new edges are created with Active status and a `dependency_detected` domain event is broadcast to the source repo's workspace via emit_event (activity event); existing edges not re-detected are marked Orphaned rather than deleted, guarded by the ran_methods set so methods that did not run in this push cannot falsely orphan their edges; version changes update version_pinned and un-orphan re-detected edges; Manual edges are never modified. Post-reconcile, the push path re-reads edges and applies version drift/staleness updates.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        if !detected_keys.contains(&key) && edge.status != DependencyStatus::Orphaned {\n            let mut updated = edge.clone();\n            updated.status = DependencyStatus::Orphaned;",
            "detail": "Orphaning: existing non-Manual edges from methods that ran but no longer detect the dependency are marked Orphaned (kept, not deleted) — matching 'mark as orphaned (don't delete)'."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                        \"event\": \"dependency_detected\",",
            "detail": "Activity event: on creating a new edge, a dependency_detected event with source/target/artifacts/method/type is emitted to the workspace destination on the unified message bus, not merely logged."
          }
        ]
      },
      {
        "id": "c8dab08785afd0c6458ae06b",
        "decision": "gap",
        "requirement_quote": "When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):",
        "detail": "The conventional-commit route and the entire downstream cascade are real and code-verified: detect_breaking_commits parses the full push range with %B so both `feat!:` subjects and `BREAKING CHANGE:` footers are caught; process_breaking_changes queries list_dependents, persists BreakingChange records per dependent edge, sets edge status Breaking, creates High-priority tasks titled 'Breaking change in {repo}: {description}' with exactly the specced labels, notifies the dependent workspace's members, and broadcasts breaking_change_detected to the dependent workspace; the Block policy mechanically fails the merge until acknowledgment. Two normative shortfalls keep this section open: (1) the parenthetical names a second detection route — 'or API contract change' — which is not implemented anywhere: detect_breaking_changes_on_push's only detection input is detect_breaking_commits over conventional-commit markers; a push that removes an OpenAPI endpoint without a `!` marker triggers no cascade. The spec's own §Relationship (Versioning) bullet says conventional commits + semver 'are how the forge detects breaking changes', which designates the implemented route as the mechanism but does not amend away the 'or API contract change' arm — surfacing this tension rather than silently resolving it. (2) Step 3b says 'Notify the dependent repo's orchestrator via MCP'; the implementation notifies via user notifications (notify_rich) and a workspace-destination message-bus broadcast, with no MCP-tool notification path (grep of mcp.rs for breaking-change notify finds nothing).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        if let Some(parsed) = crate::version_compute::parse_conventional(sha, message) {\n            if parsed.is_breaking {",
            "detail": "detect_breaking_commits is the sole breaking-change detection input: conventional-commit parsing of `!` markers and BREAKING CHANGE: footers. No API-contract-diff route feeds process_breaking_changes."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    let commit_log = String::from_utf8_lossy(&log_out);\n    let breaking_commits = detect_breaking_commits(&commit_log);",
            "detail": "detect_breaking_changes_on_push derives breaking commits exclusively from the git log message text; OpenAPI/proto contract changes pushed without conventional-commit markers produce no BreakingChange records."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
            "detail": "The specced per-dependent task creation is real: High-priority tasks with the exact title format and labels [dependency-update, breaking-change, auto-created], plus BreakingChange records, edge status updates, member notifications, and workspace broadcasts."
          }
        ]
      },
      {
        "id": "f57b4d5f840e0b485f9fe50c",
        "decision": "satisfied",
        "requirement_quote": "  breaking_change_behavior: block | warn | notify",
        "detail": "The per-workspace DependencyPolicy carries all five specced knobs (breaking_change_behavior, max_version_drift, stale_dependency_alert_days, require_cascade_tests, auto_create_update_tasks), is configurable via GET/PUT /api/v1/workspaces/:id/dependency-policy, and is persisted by real Diesel adapters for both SQLite and Postgres (task-163). Every knob has a production effect: Block fails the merge queue entry while unacknowledged breaking changes exist, Warn logs and proceeds, Notify proceeds silently; max_version_drift and stale_dependency_alert_days drive Stale classification in the push-time pass and the daily dep_staleness job; require_cascade_tests gates trigger_cascade_tests (both merged-repo and dependent-workspace policy respected); auto_create_update_tasks gates the breaking-change and stale-dependency auto-tasks.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/dependency.rs",
            "quote": "            breaking_change_behavior: BreakingChangeBehavior::Warn,",
            "detail": "DependencyPolicy default (Warn) — the struct carries breaking_change_behavior, max_version_drift, stale_dependency_alert_days (default 30), require_cascade_tests (default true), and auto_create_update_tasks."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/dependency_policy.rs",
            "quote": "impl DependencyPolicyRepository for PgStorage {",
            "detail": "Real persistent adapters exist for the policy port (Postgres here, SQLite in sqlite/dependency_policy.rs, migration 2026-10-08-000061) — policies survive restart in DB-backed deployments."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                gyre_domain::BreakingChangeBehavior::Block => {",
            "detail": "Block is mechanically enforced at merge time: the queue entry is failed with 'breaking_change_policy' until all breaking changes from the repo are acknowledged; Warn and Notify arms proceed with logging/silence respectively."
          },
          {
            "file": "crates/gyre-server/src/dep_staleness.rs",
            "quote": "            let threshold_secs = policy.stale_dependency_alert_days as u64 * 86400;",
            "detail": "stale_dependency_alert_days has a real effect in the daily job: edges not re-verified within the threshold are marked Stale (alongside the max_version_drift threshold check in the same pass)."
          }
        ]
      },
      {
        "id": "7014f11632cdabb887a52f2b",
        "decision": "gap",
        "requirement_quote": "2. For each dependent repo, the forge triggers a test run against Repo B's new version",
        "detail": "The identification, task-filing, event, notification, failure follow-up, and orchestrator-notification links are real: on merge, trigger_cascade_tests (gated on require_cascade_tests in both the merged and dependent workspaces) lists dependents from the graph, creates a High-priority 'cascade-test' task per dependent, emits cascade_test_triggered to the dependent workspace, and notifies members; when an agent running a cascade-test task completes or fails, report_cascade_test_result emits cascade_test_passed/failed, auto-creates a failure follow-up task, and notifies the workspace. The missing link is execution: the cascade-test task is created with task_type unset, and the worker scheduler explicitly never spawns untyped tasks — nothing in the forge dispatches an agent to actually run the dependent repo's tests, and there is no API endpoint to trigger or query cascade tests (the UI's trigger call is a dead reference, see §Impact Analysis/§UI). The spec's claim 'the forge knows the graph and runs the tests' is therefore only partially true: the forge files paperwork for a test run that nothing automatically executes.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "            format!(\"Cascade test: {dep_repo_name} against {merged_repo_name}@{sha_short}\"),",
            "detail": "trigger_cascade_tests creates the per-dependent cascade-test task (High priority, labels [cascade-test, auto-created]) — but the task is created with no task_type set."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "            continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The worker-spawn cycle skips every task whose task_type is not Implementation (and the orchestrator claim path only takes Delegation/Coordination), so cascade-test tasks sit in the Backlog unless a human manually spawns an agent — the forge never runs the tests itself."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "                crate::merge_processor::report_cascade_test_result(&state, &task, true, None).await;",
            "detail": "Result reporting is wired to agent completion (and to the fail path with passed=false), proving the intended execution model — but only an agent that happens to be manually spawned on the cascade-test task ever reaches this code."
          }
        ]
      },
      {
        "id": "551185c9c05657eddf50405e",
        "decision": "gap",
        "requirement_quote": "- The AIBOM flags stale dependencies in release reports",
        "detail": "The drift computation, continuous monitoring, and auto-tasking are real: compute_version_drift derives semver minor-delta drift (major bumps weighted x10), latest_semver_tag resolves the target repo's current version from git tags, drift is applied both at push time and by the daily dep_staleness_check job (registered in jobs.rs at an 86400s interval), edges exceeding max_version_drift or the alert-days threshold are marked Stale, and 'Update {repo} dependency from {pinned} to {current}' auto-tasks are created when auto_create_update_tasks is set. The section stays open on its third bullet: the AIBOM does not flag stale dependencies. Both AIBOM shapes (working summary and format=release artifact) were inspected end-to-end — AibomResponse and AibomReleaseResponse contain agents, commits, attestation chains, stack policy, spec registry, and summary sections, with no dependency-graph snapshot and no stale-dependency flag; grep for stale/dependency content in api/aibom.rs and api/release.rs returns nothing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "pub fn compute_version_drift(pinned: &str, current: &str) -> Option<u32> {",
            "detail": "Real drift computation over parsed semver triples, used by both the push-time pass (git_http.rs) and the daily dep_staleness job."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "                name: \"dep_staleness_check\".to_string(),",
            "detail": "The daily background job (86400s interval) re-checks every edge for version drift and time-based staleness — the 'continuously monitors' requirement."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub struct AibomReleaseResponse {",
            "detail": "Absence evidence: the release-artifact AIBOM struct's complete field list (aibom_version, release, generated_at, repo_id, agents, commits, stack_policy, spec_registry, summary) was inspected — no dependency-graph snapshot and no stale-dependency flag anywhere in the release report; the same holds for the summary format and api/release.rs."
          }
        ]
      },
      {
        "id": "74607a9069c32b20ef245765",
        "decision": "gap",
        "requirement_quote": "    Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown",
        "detail": "The core blast-radius query is real: GET /api/v1/repos/:id/blast-radius BFS-walks the reverse graph (direct at depth 0, transitive deeper, deduplicated, total returned), `gyre deps impact` exposes it from the CLI, and the UI modal composes it with per-dependent version_pinned/target_version_current/version_drift from the dependents endpoint plus breaking-change badges. The section stays open on the remaining specced response fields: 'Cascade test results' and per-dependent test status ('tests passing'/'tests unknown') are not served anywhere — BlastRadiusResponse carries only repo_id/direct/transitive/total, no server endpoint returns cascade test results or test status for dependents (cascade results exist only as one-shot workspace events), and the UI modal's attempt to fetch them is dead code: it calls api.cascadeTestResults?.(repoId) and api.triggerCascadeTests?.(repoId), but neither method exists in web/src/lib/api.js, so the optional chaining silently resolves to null and the test-status column is hardcoded '--'.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "    let mut direct: Vec<String> = Vec::new();\n    let mut transitive: Vec<String> = Vec::new();",
            "detail": "The blast_radius handler BFS-walks all edges to produce direct and transitive dependents plus a total — the core of the specced query."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "pub struct BlastRadiusResponse {\n    pub repo_id: String,\n    pub direct_dependents: Vec<String>,\n    pub transitive_dependents: Vec<String>,\n    pub total: usize,\n}",
            "detail": "The complete response contract: no cascade-test-result field, no per-dependent health (test status) field — the specced response lines 'Cascade test results: not yet run' and 'tests passing/tests unknown' have no server-side source."
          },
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
            "detail": "The modal's cascade-results fetch is a dead reference: api.cascadeTestResults does not exist in web/src/lib/api.js (grep for cascade in that file returns nothing), so cascadeTestResults is always null and the cascade/test-status columns can never populate."
          }
        ]
      },
      {
        "id": "79be3a435216d41c4ebe0660",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/deps/graph` | GET | Full tenant-wide dependency graph |",
        "detail": "Per the cycle-3 review correction (spec-endpoint-paths-not-served), this section is a gap. Every row's PURPOSE is implemented by a real handler — tenant graph, workspace-scoped graph, per-repo dependents/dependencies, blast radius, stale list, unacknowledged breaking list, manual add, manual-only delete, acknowledge — but NONE of the spec's literal paths is served: grep for 'api/v1/deps' across crates/ and web/src returns zero matches. The routes are registered as /api/v1/dependencies/graph, /api/v1/workspaces/:id/dependency-graph, /api/v1/repos/:id/dependencies(+/:dependency_id), /api/v1/repos/:id/dependents, /api/v1/repos/:id/blast-radius, /api/v1/dependencies/stale, /api/v1/dependencies/breaking(+:id/acknowledge). Workspace scoping is a dedicated path rather than the spec's `?workspace={id}` query parameter on the graph endpoint (the stale endpoint does accept ?workspace_id=). A consumer written against the canonical spec gets 404 on every row; docs/api-reference.md documenting the implemented shapes is not a spec-lifecycle amendment renaming the endpoints, and nothing in the spec makes the literal paths unsatisfiable.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/dependencies/graph\", get(dependencies::get_graph))",
            "detail": "The tenant-wide graph is served at /api/v1/dependencies/graph, not the spec's /api/v1/deps/graph; grep for the literal 'api/v1/deps' prefix across the entire codebase (crates/ and web/src) returns 0 matches."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "            \"/api/v1/workspaces/:id/dependency-graph\",",
            "detail": "Workspace scoping is a dedicated route rather than the spec's `GET /api/v1/deps/graph?workspace={id}` query-parameter form."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\n            \"/api/v1/repos/:id/dependencies\",\n            get(dependencies::list_dependencies).post(dependencies::add_dependency),\n        )",
            "detail": "Dependents/dependencies/manual-add/delete live under /api/v1/repos/:id/* instead of /api/v1/deps/repo/{id}/* and /api/v1/deps/* — the handlers are real (dependencies.rs), the paths are not the spec's."
          }
        ]
      },
      {
        "id": "7df3052372738ce81043bfb9",
        "decision": "satisfied",
        "requirement_quote": "gyre deps add --target repo-b --type api    # Manual dependency declaration",
        "detail": "All eight specced commands exist with the specced flags and real implementations: `deps show` (this repo's dependencies+dependents table with pinned/current/drift/status/method, resolved from the git remote), `deps show --workspace` (workspace-scoped graph, filtered from the tenant graph by the workspace's repo set), `deps show --tenant` (tenant-wide graph), `deps graph --format dot` (DOT output, other formats rejected), `deps impact <repo>` (blast radius), `deps stale`, `deps breaking`, `deps add --target --type`, and `deps acknowledge <id>` — each hitting the real server endpoints.",
        "evidence": [
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "enum DepsCommands {",
            "detail": "The DepsCommands enum defines Show{repo, workspace, tenant}, Graph{format}, Impact{repo}, Stale, Breaking, Add{target, type}, Acknowledge{id} — the full specced command set."
          },
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "                    if tenant {\n                        // Tenant-wide graph\n                        let graph = api.get_dependency_graph().await?;\n                        print_dependency_graph(&graph);",
            "detail": "The Show command's bodies are real: tenant mode fetches and renders the tenant graph, workspace mode filters the graph by the git-remote-inferred workspace's repos, and the default mode prints the dependencies/dependents tables from the live API."
          }
        ]
      },
      {
        "id": "810eac26a99174ce812d04a3",
        "decision": "gap",
        "requirement_quote": "- **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.",
        "detail": "Three of the four bullets are fully real: DependencyGraph.svelte renders the interactive DAG at workspace and tenant scope with edges colored by type, stale/breaking node coloring, and node-click to impact analysis; RepoDependencies.svelte is mounted in RepoMode (repo detail) showing dependencies/dependents with drift indicators and stale/breaking badges; WorkspaceHome's sidebar mounts DependencyHealthCard fed by loadDepHealth, which computes 'repos with stale dependencies' and 'unacknowledged breaking changes' counts from the real workspace graph, stale, and breaking endpoints. The section stays open on the fourth bullet's tail: the impact analysis modal can never show cascade test results — its fetch (api.cascadeTestResults?.(repoId)) and its trigger button (api.triggerCascadeTests?.(repoId)) reference client methods that do not exist in web/src/lib/api.js, so the optional chaining silently no-ops and the cascade-results section and test-status column can never populate. (Same root cause as the §Impact Analysis gap — no server endpoint serves cascade test results — noted there without double-counting; the client-side dead wiring is this section's own missing piece.)",
        "evidence": [
          {
            "file": "web/src/components/DependencyGraph.svelte",
            "quote": "   * yellow, breaking in red. Click a node to see impact analysis.\"",
            "detail": "The DAG visualization bullet is real: workspace/tenant-scoped interactive graph, edges colored by dependency type, stale edges yellow / breaking red, node click wired to impact analysis."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "        api.staleDependencies(workspace.id).catch(() => []),",
            "detail": "The workspace-dashboard aggregate health bullet is real: loadDepHealth fetches the workspace graph, stale edges, and breaking changes from live endpoints and feeds staleCount/breakingCount into DependencyHealthCard."
          },
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
            "detail": "The modal's cascade-test-results display is dead code: api.cascadeTestResults (and api.triggerCascadeTests, used by the trigger button) do not exist in web/src/lib/api.js — grep for 'cascade' in that file returns nothing — so 'show the full blast radius with cascade test results' cannot be met for the cascade-results part."
          }
        ]
      },
      {
        "id": "589b5beaab7b22bb95d9b7ee",
        "decision": "gap",
        "requirement_quote": "- **Merge ordering:** cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically.",
        "detail": "Two of the four bullets are real: priority decisions and cross-repo task creation are implemented through the breaking-change cascade — process_breaking_changes queries the graph's dependents and files High-priority 'Breaking change in ...' tasks in each dependent repo, and cascade test tasks are similarly per-dependent. The section stays open on the other two bullets. Merge ordering: no code links DependencyEdge to the MR-dependency system — the only writer of MergeRequestDependency records is the manual set_dependencies API (plus its cycle check); inspected call paths in git_http.rs, merge_processor.rs, and merge_deps.rs contain no automatic creation of an MR dependency from a graph edge, so 'the forge creates the MR dependency automatically' never happens. Budget allocation: nothing in the budget system (api/budget.rs) considers dependent counts — grep for dependents-based budget logic finds nothing; repos with many dependents get no budget treatment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/merge_deps.rs",
            "quote": "pub async fn set_dependencies(",
            "detail": "The MR-dependency system's only entry point is the explicit set_dependencies handler (caller-declared depends_on IDs with cycle detection); no automatic path derives MR dependencies from DependencyEdge records."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
            "detail": "The satisfied bullets: breaking-change handling uses the graph to file High-priority tasks in every dependent repo (priority decisions + cross-repo task creation), which is the real portion of this section."
          }
        ]
      },
      {
        "id": "605ecacdbe3cbbfa544aee46",
        "decision": "context",
        "requirement_quote": "- **Versioning** (`ci-docs-release.md`): conventional commits + semver are how the forge detects breaking changes. The `feat!:` or `BREAKING CHANGE:` footer triggers the cascade.",
        "detail": "Cross-reference section mapping this spec's relationships to six sibling specs; the actionable requirements live in those specs' own audits (supply-chain.md, merge-dependencies.md, spec-lifecycle.md, platform-model.md, ci-docs-release.md — supply-chain and merge-dependencies have separate staged plans in the active index). For orientation: the Versioning bullet's designated detection mechanism (conventional commits + semver via feat!:/BREAKING CHANGE:) is the implemented route and is fully real; the Supply Chain bullet's dependency-graph snapshot in the AIBOM and the Merge Dependencies bullet's auto-created cross-repo MR dependencies are absent in this repo and are flagged as gaps in their home sections of this audit (§Version Drift Tracking and §Workspace Orchestrator Integration respectively) rather than re-counted here.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "12ff1c4b10f99712ceaf0c8e",
        "decision": "gap",
        "requirement_quote": "On every push to any repo, the forge:",
        "detail": "Umbrella section whose four numbered steps are accounted under their child headings (§1 gap: Cargo.toml git-reference extraction missing; §2 satisfied; §3 gap: OpenAPI/mcp.json detection is a substring scan and only root-level contract files are read; §4 satisfied) — the child gaps are not re-counted here. The parent's own content is the push-time trigger: detection is genuinely wired into the git post-receive path, but only for ref updates on the repo's default branch (filtered by `u.refname == default_ref`) and only when a dependency-related file changed in the push. Pushes to non-default branches do not touch the graph (the graph does update when such branches later land on the default branch), and the extraction forms named by the children are only partially implemented, so the parent's statement is not fully true at this base.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            // Dependency graph: auto-detect Cargo.toml path deps (M22.4).\n            detect_dependencies_on_push(\n                &state_clone,\n                &repo_id_clone,\n                &repo_path_clone,\n                &update.new_sha,\n            )\n            .await;",
            "detail": "The trigger wiring is real: this call site sits inside `for update in applied_updates.iter().filter(|u| u.refname == default_ref)` — the post-receive loop scoped to default-branch ref updates — and detect_dependencies_on_push early-returns unless a dependency file changed in the push."
          }
        ]
      },
      {
        "id": "e07d32e2ebc10c9a36f6343c",
        "decision": "gap",
        "requirement_quote": "Cargo.toml       -> extract [dependencies] with git/path references to other Gyre repos",
        "detail": "Per the cycle-3 review correction (cargo-git-references-unparsed), this section is a gap: the spec names BOTH git and path reference forms as the Cargo.toml extraction contract, and the implementation parses only `path = \"...\"` inline-table values (detect_cargo_path_deps / extract_path_value). A `[dependencies]` entry like `repo-b = { git = \"https://gyre.example.com/<ws>/repo-b.git\" }` — exactly the Gyre-to-Gyre reference the spec names — produces no DependencyEdge; no code in the Cargo detection path handles `git =` (or `rev =`) keys. On the other three files, whose spec wordings are generic ('dependencies pointing to Gyre-hosted packages', 'require directives referencing Gyre modules', 'dependencies referencing Gyre packages'): go.mod is faithful under the generic wording because module paths are matched by last path segment, which covers both bare names and Gyre-URL module paths; package.json and pyproject.toml handle only `file:`/`workspace:` and path/`file:` forms respectively — a Gyre-hosted package referenced by git URL (e.g. `\"repo-b\": \"git+https://...\"` or `pkg @ git+https://...`) is a natural reading of the generic wording and is not detected. Secondary: only root-level Cargo.toml is read (`read_git_file(..., \"Cargo.toml\")`) although the changed-file trigger matches nested `*/Cargo.toml` paths, so path deps declared in workspace-member manifests are never detected. Matching against known tenant repos (state.repos.list()) and ignoring external registry deps are both correctly implemented for the forms that are parsed.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "fn extract_path_value(line: &str) -> Option<String> {\n    let path_idx = line.find(\"path\")?;",
            "detail": "The only Cargo.toml dependency-reference extractor: it locates the literal `path` key in a dependency line and reads its quoted value. There is no corresponding git= handling — grep for git-reference parsing in the Cargo detection path finds nothing, so `repo-b = { git = \"https://gyre.example.com/ws/repo-b.git\" }` yields no edge."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let path_deps = detect_cargo_path_deps(&content);\n            for path_dep in &path_deps {",
            "detail": "The Cargo.toml branch of detect_dependencies_on_push feeds only detect_cargo_path_deps output into DetectedEdge (DependencyType::Code, DetectionMethod::CargoToml); git-reference values never enter the flow."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    if let Some(path) = ver_str.strip_prefix(\"file:\") {",
            "detail": "package.json detection handles only `file:` and `workspace:` reference forms; a Gyre-hosted package referenced by git URL is not extracted, though the spec's wording ('dependencies pointing to Gyre-hosted packages') naturally covers it."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        // Single-line: `require module/path v1.0.0`",
            "detail": "go.mod require (and supplementary local replace) directives are parsed and matched by last path segment, which covers URL-form Gyre module paths — this file's reading is faithful to its generic spec wording."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        // PEP 621: [project] dependencies = [...] with `pkg @ file:///path`",
            "detail": "pyproject.toml detection handles Poetry `{path = ...}` and PEP 621 `@ file:` forms only; PEP 508 direct git references (`pkg @ git+https://...`) to Gyre packages are not extracted."
          }
        ]
      },
      {
        "id": "8244b7d9d6d350af456c6492",
        "decision": "gap",
        "requirement_quote": "openapi.yaml / swagger.json  -> extract server URLs matching other Gyre repos",
        "detail": "Only one of the three contract forms is implemented as specified. Proto import detection is real structured parsing (`import \"...\"` directives, first path segment matched against known repo names, all .proto files in the tree scanned via git ls-tree). But OpenAPI/Swagger and mcp.json detection are coarse whole-document substring scans — `content.contains(repo_name)` — not the specced extraction of server URLs from openapi.yaml/swagger.json or tool references from mcp.json; any mention of a repo name anywhere in the document (e.g. in a description) creates an Api edge, which both over-matches beyond server URLs and never inspects the servers[] structure the spec names. Additionally, only the three root-level file names are read (`openapi.yaml`, `openapi.yml`, `swagger.json` at the repo root) even though the change trigger matches nested paths — the spec's own entity example uses `api/openapi.yaml` as a source_artifact — so nested API contracts are never parsed. The code's own comment ('Best-effort: many API contracts don't explicitly reference repo names') acknowledges the divergence; a code comment is not a spec amendment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    for repo_name in known_repos {\n        // Match repo names in server URLs, $ref paths, and other string values.\n        if content.contains(repo_name) {",
            "detail": "detect_openapi_refs: instead of parsing servers[].url (or $ref values) from the YAML/JSON document, it scans the raw document text for each known repo name — a substring stand-in for the specced server-URL extraction."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        for api_file in &[\"openapi.yaml\", \"openapi.yml\", \"swagger.json\"] {",
            "detail": "Only the three root-level contract files are read via read_git_file, while the has_openapi trigger also matches nested `*/openapi.yaml` etc. — nested contracts (the spec's own `api/openapi.yaml` example artifact) are never parsed."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        if let Some(rest) = trimmed.strip_prefix(\"import \") {",
            "detail": "Proto detection is the satisfied third of the section: structured import-directive parsing with first-segment matching against known repos, scanned across all .proto files at the pushed SHA."
          }
        ]
      },
      {
        "id": "c8dab08785afd0c6458ae06b",
        "decision": "gap",
        "requirement_quote": "When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):",
        "detail": "The conventional-commit route and the entire downstream cascade are real and code-verified: detect_breaking_commits parses the full push range with %B so both `feat!:` subjects and `BREAKING CHANGE:` footers are caught; process_breaking_changes queries list_dependents, persists BreakingChange records per dependent edge, sets edge status Breaking, creates High-priority tasks titled 'Breaking change in {repo}: {description}' with exactly the specced labels, notifies the dependent workspace's members, and broadcasts breaking_change_detected to the dependent workspace; the Block policy mechanically fails the merge until acknowledgment. Two normative shortfalls keep this section open: (1) the parenthetical names a second detection route — 'or API contract change' — which is not implemented anywhere: detect_breaking_changes_on_push's only detection input is detect_breaking_commits over conventional-commit markers; a push that removes an OpenAPI endpoint without a `!` marker triggers no cascade. The spec's own §Relationship (Versioning) bullet says conventional commits + semver 'are how the forge detects breaking changes', which designates the implemented route as the mechanism but does not amend away the 'or API contract change' arm — surfacing this tension rather than silently resolving it. (2) Step 3b says 'Notify the dependent repo's orchestrator via MCP'; the implementation notifies via user notifications (notify_rich) and a workspace-destination message-bus broadcast, with no MCP-tool notification path (grep of mcp.rs for breaking-change notify finds nothing).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        if let Some(parsed) = crate::version_compute::parse_conventional(sha, message) {\n            if parsed.is_breaking {",
            "detail": "detect_breaking_commits is the sole breaking-change detection input: conventional-commit parsing of `!` markers and BREAKING CHANGE: footers. No API-contract-diff route feeds process_breaking_changes."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    let commit_log = String::from_utf8_lossy(&log_out);\n    let breaking_commits = detect_breaking_commits(&commit_log);",
            "detail": "detect_breaking_changes_on_push derives breaking commits exclusively from the git log message text; OpenAPI/proto contract changes pushed without conventional-commit markers produce no BreakingChange records."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
            "detail": "The specced per-dependent task creation is real: High-priority tasks with the exact title format and labels [dependency-update, breaking-change, auto-created], plus BreakingChange records, edge status updates, member notifications, and workspace broadcasts."
          }
        ]
      },
      {
        "id": "7014f11632cdabb887a52f2b",
        "decision": "gap",
        "requirement_quote": "2. For each dependent repo, the forge triggers a test run against Repo B's new version",
        "detail": "The identification, task-filing, event, notification, failure follow-up, and orchestrator-notification links are real: on merge, trigger_cascade_tests (gated on require_cascade_tests in both the merged and dependent workspaces) lists dependents from the graph, creates a High-priority 'cascade-test' task per dependent, emits cascade_test_triggered to the dependent workspace, and notifies members; when an agent running a cascade-test task completes or fails, report_cascade_test_result emits cascade_test_passed/failed, auto-creates a failure follow-up task, and notifies the workspace. The missing link is execution: the cascade-test task is created with task_type unset, and the worker scheduler explicitly never spawns untyped tasks — nothing in the forge dispatches an agent to actually run the dependent repo's tests, and there is no API endpoint to trigger or query cascade tests (the UI's trigger call is a dead reference, see §Impact Analysis/§UI). The spec's claim 'the forge knows the graph and runs the tests' is therefore only partially true: the forge files paperwork for a test run that nothing automatically executes.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "            format!(\"Cascade test: {dep_repo_name} against {merged_repo_name}@{sha_short}\"),",
            "detail": "trigger_cascade_tests creates the per-dependent cascade-test task (High priority, labels [cascade-test, auto-created]) — but the task is created with no task_type set."
          },
          {
            "file": "crates/gyre-server/src/signal_chain.rs",
            "quote": "            continue; // Delegation/Coordination → orchestrators; untyped → never spawned",
            "detail": "The worker-spawn cycle skips every task whose task_type is not Implementation (and the orchestrator claim path only takes Delegation/Coordination), so cascade-test tasks sit in the Backlog unless a human manually spawns an agent — the forge never runs the tests itself."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "                crate::merge_processor::report_cascade_test_result(&state, &task, true, None).await;",
            "detail": "Result reporting is wired to agent completion (and to the fail path with passed=false), proving the intended execution model — but only an agent that happens to be manually spawned on the cascade-test task ever reaches this code."
          }
        ]
      },
      {
        "id": "551185c9c05657eddf50405e",
        "decision": "gap",
        "requirement_quote": "- The AIBOM flags stale dependencies in release reports",
        "detail": "The drift computation, continuous monitoring, and auto-tasking are real: compute_version_drift derives semver minor-delta drift (major bumps weighted x10), latest_semver_tag resolves the target repo's current version from git tags, drift is applied both at push time and by the daily dep_staleness_check job (registered in jobs.rs at an 86400s interval), edges exceeding max_version_drift or the alert-days threshold are marked Stale, and 'Update {repo} dependency from {pinned} to {current}' auto-tasks are created when auto_create_update_tasks is set. The section stays open on its third bullet: the AIBOM does not flag stale dependencies. Both AIBOM shapes (working summary and format=release artifact) were inspected end-to-end — AibomResponse and AibomReleaseResponse contain agents, commits, attestation chains, stack policy, spec registry, and summary sections, with no dependency-graph snapshot and no stale-dependency flag; grep for stale/dependency content in api/aibom.rs and api/release.rs returns nothing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/version_compute.rs",
            "quote": "pub fn compute_version_drift(pinned: &str, current: &str) -> Option<u32> {",
            "detail": "Real drift computation over parsed semver triples, used by both the push-time pass (git_http.rs) and the daily dep_staleness job."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "                name: \"dep_staleness_check\".to_string(),",
            "detail": "The daily background job (86400s interval) re-checks every edge for version drift and time-based staleness — the 'continuously monitors' requirement."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub struct AibomReleaseResponse {",
            "detail": "Absence evidence: the release-artifact AIBOM struct's complete field list (aibom_version, release, generated_at, repo_id, agents, commits, stack_policy, spec_registry, summary) was inspected — no dependency-graph snapshot and no stale-dependency flag anywhere in the release report; the same holds for the summary format and api/release.rs."
          }
        ]
      },
      {
        "id": "74607a9069c32b20ef245765",
        "decision": "gap",
        "requirement_quote": "    Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown",
        "detail": "The core blast-radius query is real: GET /api/v1/repos/:id/blast-radius BFS-walks the reverse graph (direct at depth 0, transitive deeper, deduplicated, total returned), `gyre deps impact` exposes it from the CLI, and the UI modal composes it with per-dependent version_pinned/target_version_current/version_drift from the dependents endpoint plus breaking-change badges. The section stays open on the remaining specced response fields: 'Cascade test results' and per-dependent test status ('tests passing'/'tests unknown') are not served anywhere — BlastRadiusResponse carries only repo_id/direct/transitive/total, no server endpoint returns cascade test results or test status for dependents (cascade results exist only as one-shot workspace events), and the UI modal's attempt to fetch them is dead code: it calls api.cascadeTestResults?.(repoId) and api.triggerCascadeTests?.(repoId), but neither method exists in web/src/lib/api.js, so the optional chaining silently resolves to null and the test-status column is hardcoded '--'.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "    let mut direct: Vec<String> = Vec::new();\n    let mut transitive: Vec<String> = Vec::new();",
            "detail": "The blast_radius handler BFS-walks all edges to produce direct and transitive dependents plus a total — the core of the specced query."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "pub struct BlastRadiusResponse {\n    pub repo_id: String,\n    pub direct_dependents: Vec<String>,\n    pub transitive_dependents: Vec<String>,\n    pub total: usize,\n}",
            "detail": "The complete response contract: no cascade-test-result field, no per-dependent health (test status) field — the specced response lines 'Cascade test results: not yet run' and 'tests passing/tests unknown' have no server-side source."
          },
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
            "detail": "The modal's cascade-results fetch is a dead reference: api.cascadeTestResults does not exist in web/src/lib/api.js (grep for cascade in that file returns nothing), so cascadeTestResults is always null and the cascade/test-status columns can never populate."
          }
        ]
      },
      {
        "id": "79be3a435216d41c4ebe0660",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/deps/graph` | GET | Full tenant-wide dependency graph |",
        "detail": "Per the cycle-3 review correction (spec-endpoint-paths-not-served), this section is a gap. Every row's PURPOSE is implemented by a real handler — tenant graph, workspace-scoped graph, per-repo dependents/dependencies, blast radius, stale list, unacknowledged breaking list, manual add, manual-only delete, acknowledge — but NONE of the spec's literal paths is served: grep for 'api/v1/deps' across crates/ and web/src returns zero matches. The routes are registered as /api/v1/dependencies/graph, /api/v1/workspaces/:id/dependency-graph, /api/v1/repos/:id/dependencies(+/:dependency_id), /api/v1/repos/:id/dependents, /api/v1/repos/:id/blast-radius, /api/v1/dependencies/stale, /api/v1/dependencies/breaking(+:id/acknowledge). Workspace scoping is a dedicated path rather than the spec's `?workspace={id}` query parameter on the graph endpoint (the stale endpoint does accept ?workspace_id=). A consumer written against the canonical spec gets 404 on every row; docs/api-reference.md documenting the implemented shapes is not a spec-lifecycle amendment renaming the endpoints, and nothing in the spec makes the literal paths unsatisfiable.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/dependencies/graph\", get(dependencies::get_graph))",
            "detail": "The tenant-wide graph is served at /api/v1/dependencies/graph, not the spec's /api/v1/deps/graph; grep for the literal 'api/v1/deps' prefix across the entire codebase (crates/ and web/src) returns 0 matches."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "            \"/api/v1/workspaces/:id/dependency-graph\",",
            "detail": "Workspace scoping is a dedicated route rather than the spec's `GET /api/v1/deps/graph?workspace={id}` query-parameter form."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\n            \"/api/v1/repos/:id/dependencies\",\n            get(dependencies::list_dependencies).post(dependencies::add_dependency),\n        )",
            "detail": "Dependents/dependencies/manual-add/delete live under /api/v1/repos/:id/* instead of /api/v1/deps/repo/{id}/* and /api/v1/deps/* — the handlers are real (dependencies.rs), the paths are not the spec's."
          }
        ]
      },
      {
        "id": "810eac26a99174ce812d04a3",
        "decision": "gap",
        "requirement_quote": "- **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.",
        "detail": "Three of the four bullets are fully real: DependencyGraph.svelte renders the interactive DAG at workspace and tenant scope with edges colored by type, stale/breaking node coloring, and node-click to impact analysis; RepoDependencies.svelte is mounted in RepoMode (repo detail) showing dependencies/dependents with drift indicators and stale/breaking badges; WorkspaceHome's sidebar mounts DependencyHealthCard fed by loadDepHealth, which computes 'repos with stale dependencies' and 'unacknowledged breaking changes' counts from the real workspace graph, stale, and breaking endpoints. The section stays open on the fourth bullet's tail: the impact analysis modal can never show cascade test results — its fetch (api.cascadeTestResults?.(repoId)) and its trigger button (api.triggerCascadeTests?.(repoId)) reference client methods that do not exist in web/src/lib/api.js, so the optional chaining silently no-ops and the cascade-results section and test-status column can never populate. (Same root cause as the §Impact Analysis gap — no server endpoint serves cascade test results — noted there without double-counting; the client-side dead wiring is this section's own missing piece.)",
        "evidence": [
          {
            "file": "web/src/components/DependencyGraph.svelte",
            "quote": "   * yellow, breaking in red. Click a node to see impact analysis.\"",
            "detail": "The DAG visualization bullet is real: workspace/tenant-scoped interactive graph, edges colored by dependency type, stale edges yellow / breaking red, node click wired to impact analysis."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "        api.staleDependencies(workspace.id).catch(() => []),",
            "detail": "The workspace-dashboard aggregate health bullet is real: loadDepHealth fetches the workspace graph, stale edges, and breaking changes from live endpoints and feeds staleCount/breakingCount into DependencyHealthCard."
          },
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),",
            "detail": "The modal's cascade-test-results display is dead code: api.cascadeTestResults (and api.triggerCascadeTests, used by the trigger button) do not exist in web/src/lib/api.js — grep for 'cascade' in that file returns nothing — so 'show the full blast radius with cascade test results' cannot be met for the cascade-results part."
          }
        ]
      },
      {
        "id": "589b5beaab7b22bb95d9b7ee",
        "decision": "gap",
        "requirement_quote": "- **Merge ordering:** cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically.",
        "detail": "Two of the four bullets are real: priority decisions and cross-repo task creation are implemented through the breaking-change cascade — process_breaking_changes queries the graph's dependents and files High-priority 'Breaking change in ...' tasks in each dependent repo, and cascade test tasks are similarly per-dependent. The section stays open on the other two bullets. Merge ordering: no code links DependencyEdge to the MR-dependency system — the only writer of MergeRequestDependency records is the manual set_dependencies API (plus its cycle check); inspected call paths in git_http.rs, merge_processor.rs, and merge_deps.rs contain no automatic creation of an MR dependency from a graph edge, so 'the forge creates the MR dependency automatically' never happens. Budget allocation: nothing in the budget system (api/budget.rs) considers dependent counts — grep for dependents-based budget logic finds nothing; repos with many dependents get no budget treatment.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/merge_deps.rs",
            "quote": "pub async fn set_dependencies(",
            "detail": "The MR-dependency system's only entry point is the explicit set_dependencies handler (caller-declared depends_on IDs with cycle detection); no automatic path derives MR dependencies from DependencyEdge records."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    format!(\"Breaking change in {repo_name}: {description}\"),",
            "detail": "The satisfied bullets: breaking-change handling uses the graph to file High-priority tasks in every dependent repo (priority decisions + cross-repo task creation), which is the real portion of this section."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "a23a71630b3302002b50d4dbc1f256cf95c2deeed11f69df4a02b083410f2771"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "a03dd115a9635ca3ae50d45c69931926effafa7ddec1cfa8f317f601a53ec27c"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "42ad3821e16bc5e58a55a8edc2dfd8714b901fe397466b79b78a4213980dbbce"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/dependency.rs",
        "sha256": "d19f722e26bd022a1fc3e2f874cbed9ef3c986f873dee7904df3890d9c9b3236"
      },
      {
        "file": "crates/gyre-server/src/api/dependencies.rs",
        "sha256": "9b32eb65f4108a7b768a7654701b4b6916c15cc1a0b20adba8e5a393a61b909e"
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "sha256": "b16446e777779fcd02857cd61e536961e6c3283b2443339bd35935e9f9ec5c4d"
      },
      {
        "file": "crates/gyre-domain/src/dependency.rs",
        "sha256": "feee888b2faf0530953fdf6bc75685b1c7a678ccff79d14cd7ec87f2a74efcd7"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-10-08-000061_dep_breaking_policies/up.sql",
        "sha256": "4220fb4673b48bcb057ae8cf2d9c2bfba4ff126ac07f6513d69ed7a73a6c15e6"
      },
      {
        "file": "crates/gyre-adapters/src/postgres/dependency_policy.rs",
        "sha256": "9339186263918ea6b1b98b708f802fc7d96d4bf708a1803078f7156f63a0dded"
      },
      {
        "file": "crates/gyre-server/src/dep_staleness.rs",
        "sha256": "4583a169950276e541b131a94d86b2702aa7098de988c1be425c3aa73e710fb1"
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "sha256": "fe55371b80328824dee46be8e931fdd1b4a10a2e48da686b19557f81ea810a82"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "80fad25139b5d3c24ea43fd4cfced0c896cf9fa92b65a4fd8f10bd35b197b783"
      },
      {
        "file": "crates/gyre-server/src/version_compute.rs",
        "sha256": "1c76a3818f9688d8a222125674a4166de3a997297f4e59d31065011ee0b2d262"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "sha256": "23906a2abaf2d63aeb69bb55436a422aca9f6a5d56494c9dde13f64b6cff4b3d"
      },
      {
        "file": "web/src/components/ImpactAnalysisModal.svelte",
        "sha256": "75bc3322f30e6f8889e3b867e989d91dcdcffcc28b7e97db4908a08ac98f47f7"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "036b32975f4110d6aa35cef408441c0be5ecb4cef4a9a7504eee9caf08a8bc35"
      },
      {
        "file": "web/src/components/DependencyGraph.svelte",
        "sha256": "c1beed3d3c56f90827ca25152680848b189a8f745a63a437419538749aca2ee3"
      },
      {
        "file": "web/src/components/WorkspaceHome.svelte",
        "sha256": "b589439a0c7061a509163fe09fa3e3e6a8b4b33e57b5aefcd997f6560f4cf0c3"
      },
      {
        "file": "crates/gyre-server/src/api/merge_deps.rs",
        "sha256": "dfce35c50ddf903f2e2818a10fb71dbcc40edb4a6dd3811b47df0a2b31b9ceae"
      }
    ]
  },
  "plan": {
    "audit": "6f80727640440717602f4e2ff583f13babcdbd547c2550dea0fcf984b1270570",
    "tasks": [
      {
        "key": "dep-detection-extraction-forms",
        "title": "Dependency auto-detection: full extraction forms (git refs, structured contracts, nested files) and every-push trigger",
        "covers": [
          "12ff1c4b10f99712ceaf0c8e",
          "e07d32e2ebc10c9a36f6343c",
          "8244b7d9d6d350af456c6492"
        ],
        "required_behavior": "Narrow delta on the existing push-time detection pipeline (task-048's detect_dependencies_on_push and its helper parsers in crates/gyre-server/src/git_http.rs) — the reconciliation store logic (§4, already real) is unchanged. (A) TRIGGER (§Automatic Detection parent, gap 12ff1c4b): detection runs on EVERY push to any repo — move the detect_dependencies_on_push call out of the `applied_updates.iter().filter(|u| u.refname == default_ref)` loop so it runs for every applied non-delete ref update on ANY branch, and remove the changed-dependency-file early return: all detectors run on every push, parsing the manifests/contracts present at the pushed tip. Reconcile is idempotent for no-change pushes; ran_methods must cover all methods that ran (with all detectors running every push, orphaning stays correct: an edge from a branch whose declarations vanish on a later push is marked Orphaned, never deleted; Manual edges are never touched). This also fixes the real failure where a repo created after a source repo's push is never matched until another dependency-file-touching push happens. (B) CODE MANIFESTS (§1, gap e07d32e2): Cargo.toml — parse [dependencies] entries with BOTH `path = \"...\"` (existing) and `git = \"<url>\"` inline-table keys: extract the Gyre repo name from the git URL (last path segment, `.git` stripped, handling `<workspace>/<repo>.git` forms), match against known tenant repos (state.repos.list()), version_pinned from `version`/`tag`/`rev` keys when present; enumerate ALL Cargo.toml files in the tree at the pushed SHA (git ls-tree — the list_files_by_extension pattern already used for .proto), not only the root manifest, with source_artifact carrying the manifest's tree path. package.json — in addition to `file:`/`workspace:` forms, extract dependency values that are git URLs (`\"repo-b\": \"git+https://gyre.example.com/<ws>/repo-b.git#v1.2\"`, git+ssh forms), matched by URL last segment. pyproject.toml — in addition to `file:` forms, extract PEP 508 direct references `pkg @ git+https://...`. go.mod stays as-is (already faithful under the generic wording). External registry deps (crates.io/npm/PyPI) remain ignored — only Gyre-to-Gyre dependencies tracked. (C) API CONTRACTS (§3, gap 8244b7d9): replace the whole-document `content.contains(repo_name)` scans with structured extraction: OpenAPI/Swagger — parse the contract document (YAML or JSON) and extract server URLs only (OpenAPI 3.x `servers[].url`; Swagger 2.0 `host` + `basePath`), matching known repo names within the extracted URLs; a repo name appearing only in a description, title, or arbitrary string creates NO edge. mcp.json — parse the JSON structure and extract server URL fields (e.g. `mcpServers.<name>.url`), not arbitrary document text. Enumerate contract files tree-wide (any openapi.yaml/openapi.yml/swagger.json/mcp.json path, e.g. api/openapi.yaml — the spec's own entity example), not only root-level names. Proto import detection (already real, structured) is unchanged. All parsers must use find-based slicing that cannot panic on multibyte UTF-8 (scripts/check-byte-slice-truncation). COORDINATION: the staged spec-lifecycle and agent-workflow plans also edit git_http.rs but in unrelated functions (process_spec_lifecycle, gates); keep changes scoped to the detection helpers and the post-receive call site.",
        "acceptance": [
          "Pushing `[dependencies] repo-b = { git = \"https://gyre.example.com/<ws>/repo-b.git\" }` in a repo where repo-b is a known tenant repo creates a Code/CargoToml DependencyEdge to repo-b (GET /api/v1/repos/:id/dependencies shows it, with version_pinned when a version/tag/rev key is present).",
          "A push to a NON-default branch that adds a dependency declaration updates the graph for that repo; a later push whose tip no longer contains the declaration marks the edge Orphaned (not deleted).",
          "An api/openapi.yaml (nested) whose servers[].url references repo-b creates an Api edge with source_artifact \"api/openapi.yaml\"; the same document mentioning repo-b only in a description creates no edge.",
          "An mcp.json whose mcpServers entry url references repo-b's server creates an Api/McpToolRef edge; a repo name appearing only in an unrelated string field creates no edge.",
          "package.json `\"repo-b\": \"git+https://gyre.example.com/<ws>/repo-b.git\"` and pyproject.toml `repo-b @ git+https://...` produce Code edges matched to the known repo.",
          "After a new repo named repo-c is created, the next push to a repo whose committed manifest already referenced repo-c by name creates the edge (no dependency-file-change short-circuit).",
          "A Cargo.toml in a workspace-member subdirectory (not the root) declaring a path/git dependency on a known repo is detected with the manifest path as source_artifact."
        ],
        "negative_probe": "Push a Cargo.toml whose only repo-b reference is `repo-b = { git = \"https://gyre.example.com/<ws>/repo-b.git\" }` (no `path` key) on a feature branch, then GET the repo's dependencies: pre-fix this fails twice over — detect_cargo_path_deps/extract_path_value parse only `path = \"...\"` values so a git reference produces no edge, and detection only runs for `u.refname == default_ref` updates so a feature-branch push never runs detection at all — the probe must fail until both the git-reference extraction and the every-push trigger land. Companion over-match probe: an openapi.yaml whose description text contains \"repo-b\" but whose servers[].url does not must produce NO Api edge — pre-fix `content.contains(repo_name)` creates one, so the probe fails until server-URL-structured extraction replaces the substring scan.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "breaking-change-contract-route",
        "title": "Breaking change detection: API-contract-change route and MCP notification of the dependent repo's orchestrator",
        "covers": [
          "c8dab08785afd0c6458ae06b"
        ],
        "required_behavior": "The conventional-commit route and the entire downstream cascade (BreakingChange records, per-dependent High-priority tasks with the specced title/labels, Block policy failing the merge queue, member notifications, workspace events) are real and unchanged. This task closes the two normative shortfalls. (1) API-CONTRACT-CHANGE ROUTE (spec: \"or API contract change\"): in detect_breaking_changes_on_push, in addition to detect_breaking_commits, detect incompatible API contract changes: when a push modifies any openapi.yaml/openapi.yml/swagger.json file in the tree, parse the contract at old_sha and at new_sha (read_git_file at both) and diff the operation set — an operation (method + path) present in the old contract and absent in the new one (removed or renamed endpoint) is an API contract change; it feeds the SAME process_breaking_changes cascade with a description naming the concrete change (e.g. \"API contract change: removed DELETE /widgets from api/openapi.yaml\"), gated on existing dependents exactly like the conventional-commit route. Additive-only contract changes (new operations, no removals) are NOT breaking. YAML and JSON contract forms must both parse; an unparseable contract on either side logs a warning and skips that route for the push (no panic, no fail-open cascade). (2) MCP ORCHESTRATOR NOTIFICATION (spec step 3b: \"Notify the dependent repo's orchestrator via MCP\"): alongside the existing notify_rich member notifications and the workspace-bus broadcast, the forge delivers the notification via MCP: resolve the live repo orchestrator agent for the dependent repo (the same agent source gyre_list_repo_orchestrators / list_repo_orchestrators_core reads) and send a Directed, persisted-until-acked message on the message bus addressed Destination::Agent(orchestrator_agent_id) carrying the breaking-change payload (repo, description, breaking-change id, dependent repo) — the bus the gyre_message_poll MCP tool serves, so the orchestrator receives it through its MCP polling loop. When no repo orchestrator agent exists for the dependent repo, the existing channels remain and a warning names the dependent repo and the missing orchestrator (no silent drop of the MCP arm). COORDINATION: shares the git_http.rs push path with dep-detection-extraction-forms (different functions; reuse tree-enumeration helpers rather than duplicating them).",
        "acceptance": [
          "Push to repo B (with dependent repo A) that removes an operation from B's openapi.yaml in a plain `fix:` commit (no `!` marker, no BREAKING CHANGE footer) creates a BreakingChange record for the A→B edge, sets the edge status Breaking, and files the High-priority task titled \"Breaking change in {B}: API contract change: ...\" with labels [\"dependency-update\", \"breaking-change\", \"auto-created\"].",
          "A Directed message addressed to A's live repo orchestrator agent is persisted on the message bus and is returned by gyre_message_poll for that orchestrator, carrying the breaking-change payload.",
          "With the workspace policy set to block, the merge queue entry for the change fails while the breaking change is unacknowledged (existing Block mechanism, now also reachable via the contract route); POST acknowledge clears it.",
          "A push that only ADDS an endpoint to the contract triggers no cascade.",
          "When the dependent repo has no live orchestrator agent, the server logs a warning naming the dependent repo, and member notifications still fire."
        ],
        "negative_probe": "Push to repo B a commit titled `fix: tidy contract` that removes `DELETE /widgets` from openapi.yaml, with repo A depending on B via a DependencyEdge: pre-fix detect_breaking_changes_on_push's ONLY detection input is detect_breaking_commits over conventional-commit markers, so an unmarked contract removal returns early — no BreakingChange record, no task, no edge status change — and the notify path has no MCP delivery at all (only notify_rich user notifications and a workspace-destination bus broadcast; grep of mcp.rs/message-bus writers for a breaking-change orchestrator notification finds nothing). The probe must fail until the contract-diff route feeds the cascade and the Directed MCP message to the dependent repo's orchestrator is delivered.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "cascade-test-execution",
        "title": "Cascade testing: real agent-executed test runs against the merged version, with durable result records",
        "covers": [
          "7014f11632cdabb887a52f2b"
        ],
        "required_behavior": "The identification, task-filing, event, notification, and failure follow-up links are real and unchanged; this task closes the missing EXECUTION link so \"the forge knows the graph and runs the tests\" is true. (1) EXECUTION: the cascade-test task created by trigger_cascade_tests must actually run without human or orchestrator intervention beyond the require_cascade_tests policy gates: set task_type = Implementation (the discriminator the worker scheduler spawns — signal_chain.rs currently skips untyped tasks: \"untyped → never spawned\") or an equivalent dedicated spawn path in the scheduler; the spawned agent's prompt carries the full context — dependent repo, merged repo, merge SHA — and instructs it to run the dependent repo's test suite against Repo B's new version (update the dependency reference to the merged commit/tag, run the repo's tests), so completion/failure is a real test outcome flowing through the existing report_cascade_test_result path (agent complete → passed; agent failed → failed with details). (2) DURABLE RESULTS: report_cascade_test_result persists a CascadeTestResult record (dependent repo id, merged repo id, merge SHA, cascade task id, outcome pass/fail, failure details, timestamp) through a new gyre-ports trait implemented by the mem, Diesel SQLite, AND Postgres adapters (contract enforced identically in all three — check-mem-port-contracts), with a migration using the next unused 6-digit sequence number and SQL portable across both backends (check-migration-sql-portability), wired via the store! macro — NOT an Arc<Mutex<HashMap>> in AppState (check-in-memory-state-stores; a restart must not orphan results). The existing one-shot workspace events and notifications remain.",
        "acceptance": [
          "With require_cascade_tests enabled in both the merged and dependent workspaces, merging a change in repo B that repo A depends on results in an agent actually spawned for the cascade-test task (an agent record tied to repo A and the task exists) with no human action, and the task carries a spawnable task_type at creation.",
          "On agent completion, a durable CascadeTestResult (passed) is retrievable from the store after a server restart; on agent failure, the record carries the failure details and the existing follow-up task and cascade_test_failed event fire.",
          "The new port has mem, SQLite, and Postgres implementations with the failure constraints enforced identically, and the migration runs on both storage backends."
        ],
        "negative_probe": "Enable require_cascade_tests in both workspaces and merge a change in repo B on which repo A depends: pre-fix the created cascade-test task has task_type unset and signal_chain's scheduler loop explicitly skips untyped tasks (\"untyped → never spawned\"), so no agent is ever spawned, no test of the dependent repo ever runs, and no durable result record exists anywhere — cascade outcomes live only as one-shot workspace events. The probe (an agent spawned for the task plus a CascadeTestResult that survives a server restart) must fail until execution and persistence land.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "impact-cascade-results-ui",
        "title": "Impact analysis: cascade test results and per-dependent test status served by the API and shown in the UI",
        "covers": [
          "74607a9069c32b20ef245765",
          "810eac26a99174ce812d04a3"
        ],
        "required_behavior": "The blast-radius BFS query, CLI impact command, and the UI modal's composition of pinned/current/drift and breaking badges are real and unchanged. This task closes the remaining specced response fields and the dead client wiring. (1) SERVER: extend the impact/blast-radius surface (BlastRadiusResponse in crates/gyre-server/src/api/dependencies.rs and/or the per-repo dependents listing the modal composes) so each dependent repo reports its test status — \"passing\"/\"failing\" from the dependent's most recent durable CascadeTestResult (cascade-test-execution's store), \"unknown\" when none exists — and the response includes the cascade test results themselves (per-dependent outcome, merged repo, SHA, timestamp), matching the spec's example (\"Cascade test results: not yet run\" when none exist). Add the two server endpoints the UI's dead calls reference: GET cascade test results for a repo (from the durable store, not one-shot events) and POST to trigger cascade tests for a repo's dependents (same require_cascade_tests policy gating as the merge path, both merged-repo and dependent-workspace policy respected; per-handler authorization loads the scoped entity and compares tenant/workspace against the caller). (2) UI (web): implement the missing client methods api.cascadeTestResults(repoId) and api.triggerCascadeTests(repoId) in web/src/lib/api.js against those endpoints, and make ImpactAnalysisModal.svelte populate the cascade-results section and the per-dependent test-status column from the real responses — removing the silent optional-chaining no-ops (`api.cascadeTestResults?.(repoId) ?? Promise.resolve(null)`) and the hardcoded testStatus '--'; the trigger button invokes the real endpoint and refreshes the results. New routes get ABAC RouteResourceMapping entries (no exemption-file growth).",
        "acceptance": [
          "After a cascade test ran for dependent A against merged repo B, GET the blast-radius/impact endpoint for B: A's entry carries test status \"passing\" (or \"failing\") and the cascade test results are included in the response; a dependent with no result shows \"unknown\" / not-yet-run.",
          "GET cascade test results for a repo returns the durable records (surviving restart); POST trigger with require_cascade_tests enabled creates cascade-test tasks for the repo's dependents and is refused/skipped when policy disables it.",
          "In web/src/lib/api.js, cascadeTestResults and triggerCascadeTests are defined (grep finds real implementations, not optional-chained absent methods); the impact analysis modal's cascade-results section and per-repo test-status column populate from the API, and the trigger button performs a real call.",
          "scripts/check-abac-route-registry.sh and check-abac-exempt-handlers.sh pass with the new endpoints (no new exemptions)."
        ],
        "negative_probe": "Run a cascade test for repo A against merged repo B (via cascade-test-execution), then GET B's blast-radius endpoint and open the impact analysis modal for B in the web UI: pre-fix BlastRadiusResponse carries only repo_id/direct/transitive/total — no endpoint anywhere serves cascade test results (they exist only as one-shot workspace events) — and the modal's api.cascadeTestResults?.(repoId) silently resolves to null because the method does not exist in web/src/lib/api.js, leaving the test-status column hardcoded '--'. The probe (real cascade results visible in BOTH the API response and the modal) must fail until the endpoints and the client methods exist.",
        "dependencies": [
          "cascade-test-execution"
        ],
        "existing_task": null
      },
      {
        "key": "deps-spec-route-paths",
        "title": "Serve the spec's literal /api/v1/deps/* API surface (all ten rows, ?workspace= graph scoping)",
        "covers": [
          "79be3a435216d41c4ebe0660"
        ],
        "required_behavior": "Every row's PURPOSE is implemented by a real handler today, but none of the spec's literal paths is served. Register the spec's literal routes in crates/gyre-server/src/api/mod.rs, backed by the real handler cores: GET /api/v1/deps/graph (full tenant-wide graph) with the SAME path accepting ?workspace={id} for the workspace-scoped graph (query-parameter scoping on the graph endpoint — not only the dedicated /workspaces/:id/dependency-graph path); GET /api/v1/deps/repo/{id}/dependents; GET /api/v1/deps/repo/{id}/dependencies; GET /api/v1/deps/repo/{id}/impact (full blast-radius analysis — including the cascade test results and test status delivered by impact-cascade-results-ui); GET /api/v1/deps/stale; GET /api/v1/deps/breaking; POST /api/v1/deps/manual (declare a manual edge — the source repo is derived from or validated against the caller's scope; no unvalidated caller-supplied tenant/workspace/repo scoping fields per check-forged-scope-fields); DELETE /api/v1/deps/{id} (manual-only removal — refuses non-manual edges, same semantics as the existing delete handler); POST /api/v1/deps/breaking/{id}/acknowledge. The existing routes (/api/v1/dependencies/*, /api/v1/repos/:id/*, /api/v1/workspaces/:id/dependency-graph) keep working — the satisfied CLI and UI consume them; the spec paths are additions, not a breaking rename. Every new route gets an ABAC RouteResourceMapping entry in the abac_middleware resolver (scripts/check-abac-route-registry.sh — no exemption-file growth, no duplicate resolver entries), and any route that runs without middleware ABAC enforces per-handler authorization (load the scoped entity, compare tenant/workspace against the caller, Forbidden on mismatch). Update docs/api-reference.md to document the spec-literal paths.",
        "acceptance": [
          "Each of the ten spec rows answers on its literal path with the specced purpose: tenant-wide graph; workspace-scoped graph via GET /api/v1/deps/graph?workspace={id}; per-repo dependents; per-repo dependencies; full blast-radius impact (with cascade results and test status); stale list; unacknowledged breaking list; manual edge declaration; manual-only delete (a non-manual/auto-detected edge is refused with an error); breaking-change acknowledge.",
          "A consumer written against the canonical spec table gets zero 404s across all ten rows.",
          "The existing CLI commands (gyre deps show/graph/impact/stale/breaking/add/acknowledge) still work unchanged against the existing routes.",
          "scripts/check-abac-route-registry.sh and check-abac-exempt-handlers.sh pass with all new routes registered in the resolver (no new exemption entries), and docs/api-reference.md lists the /api/v1/deps/* paths."
        ],
        "negative_probe": "curl GET /api/v1/deps/graph with a valid token: pre-fix the route does not exist — grep for 'api/v1/deps' across crates/ and web/src returns zero matches, and every row of the spec's API table 404s because the implemented handlers live only under /api/v1/dependencies/*, /api/v1/repos/:id/*, and /api/v1/workspaces/:id/dependency-graph — so a spec-conforming consumer gets 404 on every row and the probe must fail until the literal paths are registered with ABAC mappings. Second probe: DELETE /api/v1/deps/{id} against an auto-detected (non-manual) edge must be refused — proving manual-only deletion semantics survived the new route.",
        "dependencies": [
          "impact-cascade-results-ui"
        ],
        "existing_task": null
      },
      {
        "key": "orchestrator-merge-ordering-budget",
        "title": "Orchestrator integration: graph-driven auto-created MR dependencies (merge ordering) and dependent-count budget weighting",
        "covers": [
          "589b5beaab7b22bb95d9b7ee"
        ],
        "required_behavior": "The priority-decisions and cross-repo-task-creation bullets are real (breaking-change cascade) and unchanged. This task closes the other two bullets. (1) MERGE ORDERING — \"cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically\": add a graph-driven detector alongside detect_lineage_deps, integrated into the same reevaluate_lineage_deps reconciliation (called from the MR create/update paths): for an open MR in repo A, list A's DependencyEdges (state.dependencies.list_by_repo); when the MR's diff (source branch vs default branch, git diff --name-only) touches an edge's source_artifact — i.e. the MR updates A's dependency declaration on repo B (adding or changing the reference/pinned version) — and repo B has a pending (Open) MR, automatically create a MergeRequestDependency from A's MR to B's MR, recorded with a NEW DependencySource variant (e.g. DependencyGraph) so it is distinguishable from Explicit/BranchLineage/AgentDeclared edges; reconciliation drops graph-sourced deps that are no longer detected (stale), never drops explicit ones, and skips duplicates — exactly the lineage pattern. When repo B has multiple open MRs, link the most recently updated one (deterministic, documented at the detector). The existing merge-queue dependency semantics then order the merges (A's MR waits for B's), and the existing dependency cycle check guards creation. (2) BUDGET ALLOCATION — \"repos with many dependents may warrant higher agent budgets\": in the repo-budget inheritance path (check_repo_spawn_budget in crates/gyre-server/src/api/budget.rs, when repo.budget is None and the repo inherits the workspace limit), weight the enforced caps by the repo's dependent count read from the live dependency graph (state.dependencies.list_dependents): a defined, monotonic, bounded boost (e.g. +25% of the workspace limit per dependent, capped at 2x the workspace limit) applied to the enforced token/cost/concurrent-agent caps at spawn time. Explicit repo overrides always win unchanged; the weighting never reduces a repo below the unweighted inherited limit; the rule is documented at the enforcement site.",
        "acceptance": [
          "Repo A has a DependencyEdge on repo B; an MR opened in A whose diff changes A's repo-b dependency declaration (e.g. the repo-b line in Cargo.toml), while B has an open MR, automatically carries a MergeRequestDependency on B's MR with the new DependencySource; A's MR merge is ordered after B's by the existing merge-queue dependency semantics.",
          "Closing B's MR, or updating A's MR so its diff no longer touches the dependency declaration, drops the graph-sourced MR dependency on re-evaluation; explicitly-set MR dependencies are never dropped and are not duplicated.",
          "Two repos in a workspace, neither with an explicit budget override, one with N>0 dependents and one with none: the dependent-heavy repo's enforced spawn budget is measurably higher — e.g. with workspace max_concurrent_agents=2 and the boost cap at 2x, a repo with 5 dependents may run 4 concurrent agents while the 0-dependent repo is capped at 2 — enforced at spawn by check_repo_spawn_budget with the dependent count read from the graph.",
          "A repo with an explicit budget override keeps exactly that override regardless of dependent count."
        ],
        "negative_probe": "Create the A→B DependencyEdge, open an MR in B, then open an MR in A that updates the repo-b line in Cargo.toml: pre-fix the ONLY writer of MergeRequestDependency records is the manual set_dependencies API — no call path in git_http.rs, merge_processor.rs, or merge_requests.rs creates one from a graph edge — so A's MR carries no dependency and can merge before B's MR; the probe must fail until the graph-driven auto-creation exists. Companion budget probe: with workspace max_concurrent_agents=2, spawn a 3rd agent on a repo with 5 dependents and no override — pre-fix nothing in api/budget.rs reads dependent counts, so the spawn is rejected exactly as for a 0-dependent repo; post-fix the dependent-weighted cap admits it (and the same spawn on the 0-dependent repo is still rejected).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "aibom-stale-dependency-flags",
        "title": "AIBOM: dependency-graph snapshot and stale-dependency flags in release reports",
        "covers": [
          "551185c9c05657eddf50405e"
        ],
        "required_behavior": "The drift computation, daily dep_staleness_check job, Stale classification, and update auto-tasks are real and unchanged. This task closes the third bullet — the AIBOM flags stale dependencies in release reports — and the dependency-graph snapshot the audit homes in this section. build_release_aibom (crates/gyre-server/src/api/aibom.rs) gains a dependencies section built from the REAL dependency store at AIBOM build time: a snapshot of the released repo's outgoing DependencyEdges (target repo, dependency type, detection method, pinned version, target current version, version drift, status) plus its dependent count, with stale edges EXPLICITLY flagged as supply-chain risk (a stale/risk list naming each stale dependency with its drift and versions). The data comes from state.dependencies (list_by_repo) using the status/drift values the push-time pass and the daily job already maintain — no re-derivation drift between the graph and the AIBOM. The working-summary shape (AibomResponse) carries the stale-dependency flags (list + count) as well. The section is always present when the repo has edges (an all-current dependency set shows the edges with no stale flags — an omitted section would hide the snapshot), and a repo with no edges shows an empty snapshot with that stated. COORDINATION/PARTITION with active work: the staged supply-chain plan (plan_id b2576f0bb5cd13421ed650b13f3912dad0c8bb4f739145bc108b9e2fc2a0a571, task release-artifacts-aibom-sbom, status planned) wires AIBOM/SBOM generation into the release flow and also edits api/aibom.rs; THAT task owns per-release artifact generation, THIS task owns the dependency/stale CONTENT of the AIBOM shapes. The two compose: whichever lands second must preserve the other's fields; do not implement release-MR/tag/publish machinery here.",
        "acceptance": [
          "A repo with an edge whose status is Stale (drift exceeding the workspace max_version_drift): GET /api/v1/repos/:id/aibom?format=release&release={id} includes the dependencies section listing that edge flagged stale / as supply-chain risk, with its pinned version, current version, and drift.",
          "A repo whose edges are all current shows the dependency snapshot with no stale flags (section present, flags empty — not omitted).",
          "The flags track the live store: marking an edge stale (or resolving its drift) changes the next AIBOM build's output for the same release inputs.",
          "The working-summary AIBOM (default format) carries the stale-dependency list/count for the repo."
        ],
        "negative_probe": "Make repo A's dependency on repo B stale (A pins 1.2.0, B's latest tag is 1.5.0, drift over the workspace policy so the dep_staleness pass marks the edge Stale), then GET A's release AIBOM: pre-fix AibomReleaseResponse contains agents, commits, attestation chains, stack policy, spec registry, and summary — no dependency-graph snapshot and no stale-dependency flag anywhere (grep for stale/dependency content in api/aibom.rs returns nothing) — so the probe must fail until the dependencies section with the stale flag is built from the real DependencyRepository store.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "dep-detection-extraction-forms": "task-278",
    "breaking-change-contract-route": "task-279",
    "cascade-test-execution": "task-280",
    "impact-cascade-results-ui": "task-281",
    "deps-spec-route-paths": "task-282",
    "orchestrator-merge-ordering-budget": "task-283",
    "aibom-stale-dependency-flags": "task-284",
    "_publication": "task-285",
    "_base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
