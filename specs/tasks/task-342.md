---
title: "Publish reviewed spec backlog for specs/system/dependency-graph.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/dependency-graph.md",
  "plan_id": "4baa14e92efd107253381d13cd29b4038d0f21d30c35d5f0da35d4eaf776b808",
  "audit": {
    "base": "87377fe145ebddf27ef975fba7829fb10f4bb136",
    "requirements": "ed9f34316476f290bbed88c9764c9ac6d14be0a46ea75ea0400818fef323fdcc",
    "sections": [
      {
        "id": "129c28d0ec208eaddbf6377f",
        "decision": "context",
        "requirement_quote": "# Cross-Repo Dependency Graph",
        "detail": "Document title heading. No implementable requirement; it names the capability whose requirements live in the child sections below.",
        "evidence": []
      },
      {
        "id": "875be00cf9159a038eeb3e33",
        "decision": "context",
        "requirement_quote": "At enterprise scale, repos don't exist in isolation. Repo A imports a crate from Repo B. Repo C calls an API defined by Repo D.",
        "detail": "Problem statement / rationale: motivates the graph (invisible cross-repo dependencies, blast radius, version drift, manual impact analysis, orchestrator prioritization data). Describes why the capability is needed, not what the forge must do; the implementable requirements live in the child sections. The concerns named here are each addressed by concrete machinery audited under the child headings (detection, breaking-change cascade, drift tracking, impact analysis, orchestrator integration).",
        "evidence": []
      },
      {
        "id": "c725cb1207c5bdc61a152454",
        "decision": "satisfied",
        "requirement_quote": "Gyre maintains a **tenant-wide dependency graph** across all repos, detected automatically and enforced mechanically. The forge sees every push across every repo - it uses this visibility to build, maintain, and enforce the graph.",
        "detail": "The capability thesis is real at this base. Detected automatically: detect_dependencies_on_push runs in the git post-receive path for EVERY applied non-delete ref update on ANY branch (task-278 widened the trigger from default-branch-only). Maintained: edges persist through the DependencyRepository store (SQLite/PG adapters, mem stand-in) and are reconciled idempotently per push. Enforced mechanically: a workspace with breaking_change_behavior=block has its merge-queue entries failed with reason \"breaking_change_policy\" until every dependent acknowledges the breaking change (merge_processor). Tenant-wide: the SQLite/PostgreSQL RepoRepository::list filters by the storage handle's tenant scope, so detection matches identifiers against the caller-tenant's repos. The child sections account for the individual requirements; this parent's own claim was verified against the trigger, storage, and enforcement wiring.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "for update in applied_updates.iter() {\n            detect_dependencies_on_push(",
            "detail": "The push-time trigger: this loop sits in the post-receive path over applied_updates (non-delete ref updates on any branch) and invokes detection for every one of them — the \"sees every push across every repo\" visibility the section claims."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "gyre_domain::BreakingChangeBehavior::Block => {",
            "detail": "Mechanical enforcement: when the MR's repo has unacknowledged breaking changes and the workspace policy is block, the merge-queue entry is failed with reason \"breaking_change_policy\" — the merge cannot proceed until dependents acknowledge."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/repository.rs",
            "quote": ".filter(repositories::tenant_id.eq(&tenant))",
            "detail": "Tenant scoping of the repo set detection matches against: RepoRepository::list in the SQLite adapter (and the PostgreSQL sibling) filters rows by the storage handle's tenant_id, so \"known repos in the tenant\" is the production matching set, not the whole deployment."
          }
        ]
      },
      {
        "id": "ab77890c60e6cba135046761",
        "decision": "satisfied",
        "requirement_quote": "| `manual` | Explicitly declared via API/CLI | Dependencies that can't be auto-detected |",
        "detail": "All five dependency types exist in gyre-domain and every type has a real production path into the graph: code via Cargo.toml/package.json/go.mod/pyproject.toml parsing (DependencyType::Code edges in detect_dependencies_on_push), spec via specs/manifest.yaml cross-repo links (DependencyType::Spec), api via OpenAPI/Swagger server URLs and mcp.json tool references (DependencyType::Api), schema via protobuf import directives (DependencyType::Schema), manual via POST /api/v1/deps/manual and `gyre deps add` (DetectionMethod::Manual edges). No type is a dead enum variant.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/dependency.rs",
            "quote": "    /// Manually declared via API — cannot be auto-detected.\n    Manual,",
            "detail": "The DependencyType enum carries all five specced variants (Code, Spec, Api, Schema, Manual)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "dep_type: DependencyType::Code,",
            "detail": "Code-type edges are created by the manifest parsers (this line is in the Cargo.toml branch; the Spec, Api, and Schema branches of the same detector create the other auto-detected types)."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "DetectionMethod::Manual,",
            "detail": "Manual-type edges are declarable through the real API handler add_manual_dependency (POST /api/v1/deps/manual, also used by `gyre deps add`), with per-handler tenant containment on both ends of the edge."
          }
        ]
      },
      {
        "id": "4390a1b94a03e318854cbbec",
        "decision": "gap",
        "requirement_quote": "    Orphaned,     // Target repo was deleted or archived",
        "detail": "The entity shape is complete: the production DependencyEdge carries every specced field (id, source/target repo ids, dependency_type, source_artifact, target_artifact, version_pinned, target_version_current, version_drift, detection_method, detected_at, last_verified_at, status) and all four statuses exist and are set by real transitions (Active on create, Stale by drift/staleness, Breaking by the breaking-change cascade). The gap is the documented semantics of Orphaned itself: \"Target repo was deleted or archived\". No production path marks a dependents' edge Orphaned when the target repo is archived or deleted. archive_repo cancels the repo's tasks and updates the repo row but never touches dependency edges; delete_repo cleans up spec links and spec assertion results but leaves dependency edges untouched; the daily dep_staleness_check job explicitly skips edges already Orphaned and never orphans anything itself; the only Orphaned transition is reconcile_dependencies' no-longer-detected check, which runs only when the SOURCE (dependent) repo is pushed and only for detection methods that ran in that push. Consequences: a deleted target's edges keep their prior status indefinitely until the dependent happens to push again (lazy, best-effort coverage of the \"deleted\" half), and an archived target is never orphaned at all — archived repos remain in RepoRepository::list, so detection keeps matching them and reconcile re-confirms the edge as detected. The spec's own comment names both causes; neither is wired.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    // Clean up the spec-link graph rows sourced from this repo (spec-links.md",
            "detail": "delete_repo's cleanup list, inspected in full: it deletes the repo row, spec-link graph rows, spec assertion results, and the on-disk git directory — dependency edges pointing at the deleted repo are absent from the handler; nothing marks them Orphaned."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    repo.archive();\n    repo.updated_at = now_secs();",
            "detail": "archive_repo transitions the repo row to archived and proceeds to cancel active tasks — the inspected handler contains no dependency-edge handling, so edges whose target is this repo remain Active/Stale despite the spec's Orphaned semantics for archived targets."
          },
          {
            "file": "crates/gyre-server/src/dep_staleness.rs",
            "quote": "if edge.status == DependencyStatus::Breaking || edge.status == DependencyStatus::Orphaned {",
            "detail": "The daily dep_staleness_check job (the continuous monitor) treats Orphaned as \"managed by other systems\" and skips it — it never orphans edges itself, including edges whose target repo no longer resolves (target_repo None yields target_version None, which produces no status change to Orphaned)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "if !detected_keys.contains(&key) && edge.status != DependencyStatus::Orphaned {",
            "detail": "The only Orphaned transition in production: reconcile_dependencies marks an edge Orphaned when its declaration is no longer detected — but this runs only on a push to the SOURCE repo, and only for edges whose detection_method ran in that push, so target deletion is covered lazily at best and target archival never (the archived target still matches a known repo and is re-detected as present)."
          }
        ]
      },
      {
        "id": "12ff1c4b10f99712ceaf0c8e",
        "decision": "satisfied",
        "requirement_quote": "On every push to any repo, the forge:",
        "detail": "Umbrella section whose four numbered steps are accounted under their child headings (all four satisfied at this base: full extraction forms including git references, spec links, structured API contracts, and reconcile). The parent's own content is the push-time trigger, which is genuinely every-push now: the post-receive loop invokes detect_dependencies_on_push for every applied non-delete ref update on ANY branch — the cycle-3 default-branch-only restriction was removed by task-278 — and the detectors parse the manifests and contracts present at the pushed tip with idempotent reconciliation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "// non-delete ref update on ANY branch, not only the default ref —",
            "detail": "The trigger's scope comment, matched by the code below it: the detection loop iterates applied_updates without the default_ref filter that gates the ledger-sync/breaking-change block, so pushes to feature branches also maintain the graph."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "for update in applied_updates.iter() {\n            detect_dependencies_on_push(",
            "detail": "The unfiltered loop over all applied ref updates calling the detector — the concrete \"on every push to any repo\" wiring."
          }
        ]
      },
      {
        "id": "e07d32e2ebc10c9a36f6343c",
        "decision": "satisfied",
        "requirement_quote": "Cargo.toml       -> extract [dependencies] with git/path references to other Gyre repos",
        "detail": "The cycle-3 gap (git-reference extraction missing) is closed by task-278. Cargo.toml: detect_cargo_deps extracts BOTH `path = \"...\"` and `git = \"url\"` inline-table values (word-boundary key matching so `path` cannot match inside a crate name or URL), with version taken from version/tag/rev; the repo-name candidate comes from repo_name_from_reference (last path segment, `#fragment`/`?query` stripped, `.git` suffix removed, `<workspace>/<repo>` forms handled); every Cargo.toml in the tree is enumerated (workspace members' manifests carry their tree path as source_artifact). package.json: file:, workspace:, and git-URL references (is_git_reference covers git+/git:/git@/ssh forms, `#fragment` carries the pinned version). go.mod: require directives (single-line and block) plus local replace directives, matched by last path segment. pyproject.toml: PEP 621 `pkg @ file:`/`pkg @ git+https` direct references and Poetry `{ path = ... }` entries. External dependencies are ignored — only references whose resolved name matches a known repo create an edge, and the known-repo set is tenant-scoped by the storage adapter (SQLite/PG list filters tenant_id).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        let reference = if let Some(path_val) = extract_toml_string_value(trimmed, \"path\") {\n            path_val\n        } else if let Some(git_val) = extract_toml_string_value(trimmed, \"git\") {\n            git_val\n        } else {",
            "detail": "The Cargo.toml extraction contract: both specced reference forms are parsed — `path =` and `git =` inline-table values — closing the cycle-3 cargo-git-references-unparsed finding; a git-URL dependency entry now produces a DependencyEdge."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let cargo_files = list_files_by_names(&git_bin, repo_path, new_sha, &[\"Cargo.toml\"]).await;",
            "detail": "Every Cargo.toml in the pushed tree is enumerated (root + workspace members), each read at the pushed tip and parsed — not just the root manifest."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "} else if is_git_reference(ver_str) {",
            "detail": "package.json detection handles git-URL references to Gyre-hosted packages (git+https/git+ssh/git@/ssh forms) alongside file: and workspace: refs — the \"dependencies pointing to Gyre-hosted packages\" form the spec names."
          }
        ]
      },
      {
        "id": "25c591a75522545b22e1c001",
        "decision": "satisfied",
        "requirement_quote": "From `specs/manifest.yaml`, extract all cross-repo spec links (already covered in `spec-links.md`). These become `spec` type edges in the dependency graph.",
        "detail": "detect_manifest_spec_links parses specs/manifest.yaml with serde_yaml into the SpecManifest struct, keeps links whose target starts with `@`, resolves cross-repo and cross-workspace target forms to the target repo name (same-repo links skipped), and detect_dependencies_on_push turns each into a DependencyType::Spec edge with ManifestLink detection method and the link target as target_artifact, matched against known repos.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "ran_methods.insert(DetectionMethod::ManifestLink);",
            "detail": "The manifest-link detector is registered as a ran method in the push pipeline (the branch reads specs/manifest.yaml at the pushed tip and calls detect_manifest_spec_links)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "dep_type: DependencyType::Spec,",
            "detail": "Spec links become `spec` type edges in the graph — exactly the type mapping this section requires."
          }
        ]
      },
      {
        "id": "8244b7d9d6d350af456c6492",
        "decision": "satisfied",
        "requirement_quote": "openapi.yaml / swagger.json  -> extract server URLs matching other Gyre repos",
        "detail": "All three contract forms are implemented with structured parsing — the cycle-3 substring-scan and root-only-file gaps are closed by task-278. OpenAPI/Swagger: detect_openapi_refs parses the document (serde_yaml; JSON is valid YAML) and extracts ONLY servers[].url (OpenAPI 3.x) and host+basePath (Swagger 2.0), matching known repo names as whole URL labels (repo_name_in_url splits on URL separators, so a name in a description/title creates no edge). Proto: detect_proto_imports parses `import \"...\"` directives and matches the first path segment against known repos across ALL .proto files in the tree (multi-file method scanned whole-tree to avoid false orphaning). mcp.json: detect_mcp_refs parses mcpServers.<name>.url/server and tools[].server/url fields and matches names within those URLs only. Contract files (openapi.yaml/.yml, swagger.json, mcp.json) are enumerated across the whole tree, nested contracts carrying their tree path as source_artifact.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "if let Some(servers) = doc.get(\"servers\").and_then(|v| v.as_sequence()) {",
            "detail": "Structured OpenAPI extraction: server URLs come from the parsed document's servers[].url (and Swagger 2.0 host/basePath below it), not from scanning raw text for repo names."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let all_proto_files = list_files_by_extension(&git_bin, repo_path, new_sha, \".proto\").await;",
            "detail": "Proto import detection enumerates every .proto file in the pushed tree, not only root-level or changed files."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "if let Some(servers) = parsed.get(\"mcpServers\").and_then(|v| v.as_object()) {",
            "detail": "mcp.json detection parses the canonical MCP client config shape (mcpServers.<name>.url/server, plus tools[].server/url) — tool references to other repos' MCP servers, matched only within URL fields."
          }
        ]
      },
      {
        "id": "26e66ab12d5a4bd37d82f499",
        "decision": "satisfied",
        "requirement_quote": "- New dependency detected -> create edge, log activity event",
        "detail": "All three specced transitions are implemented against the real store in reconcile_dependencies: new edges are created with Active status and a `dependency_detected` domain event broadcast to the source repo's workspace (activity event via the unified message bus, not just infrastructure logging); edges no longer detected — only those whose detection_method ran in this push, preventing false orphaning when a subset of dependency files changed — are marked Orphaned, never deleted; version changes update version_pinned/target_version_current/version_drift on the existing edge. Manual edges are never modified by reconciliation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "\"event\": \"dependency_detected\",",
            "detail": "The activity event for a newly created edge: a dependency_detected payload broadcast to the source repo's workspace destination through emit_event (WebSocket consumers / orchestrators / dashboard), satisfying \"create edge, log activity event\"."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "updated.status = DependencyStatus::Orphaned;",
            "detail": "The no-longer-detected transition marks the edge Orphaned (the edge row is saved with the new status — not deleted), guarded by ran_methods so edges from detection methods that did not run in this push are left untouched."
          }
        ]
      },
      {
        "id": "c8dab08785afd0c6458ae06b",
        "decision": "satisfied",
        "requirement_quote": "When a push to Repo B is detected as a **breaking change** (semver major bump via conventional commit, or API contract change):",
        "detail": "Both detection routes named by the spec are real. Conventional commit: detect_breaking_commits parses the full push range with `--format=%H%x00%B%x00` so both `feat!:` subjects and `BREAKING CHANGE:` footers in interior commits are caught. API contract change: detect_api_contract_changes diffs the push's contract files (openapi/swagger) between old and new SHAs and flags removed operations as breaking (renames read old side at the old path; added contracts are additive; deleted contracts remove every old operation). Both routes feed the same cascade in process_breaking_changes: dependents queried via list_dependents, a BreakingChange record created per dependent edge, edge status set to Breaking, a High-priority task filed in the DEPENDENT repo's workspace with the specced title/labels (gated on auto_create_update_tasks), member notifications with the BreakingChangeDetected taxonomy type, a workspace broadcast — and the MCP arm closed by task-279: the dependent repo's live repo orchestrator is resolved (find_live_repo_orchestrator, the same source gyre_list_repo_orchestrators reads) and the breaking-change payload is delivered to its agent inbox (Destination::Agent), the inbox the gyre_message_poll MCP tool serves; when no live orchestrator exists a warning names the dependent repo (no silent drop). Step 3c's BLOCK is enforced at merge time under the Enforcement Policies section.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let contract_changes = detect_api_contract_changes(&git_bin, repo_path, old_sha, new_sha).await;",
            "detail": "The \"or API contract change\" detection route: contract-file removals between the push's old and new SHAs are appended to the conventional-commit breaking changes and feed the same dependent cascade."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "match crate::signal_chain::find_live_repo_orchestrator(state, dep_repo).await {",
            "detail": "Step 3b \"Notify the dependent repo's orchestrator via MCP\": the dependent repo's live repo orchestrator is resolved and the breaking-change payload is emitted to its agent-destined inbox (Destination::Agent), which the gyre_message_poll MCP tool serves; resolution failure or absence logs a warning naming the repo."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                    \"dependency-update\".to_string(),\n                    \"breaking-change\".to_string(),\n                    \"auto-created\".to_string(),",
            "detail": "Step 3a's task shape: High-priority tasks titled \"Breaking change in {repo_b}: {change_description}\" carry exactly the specced labels, filed in the dependent repo's workspace (task.workspace_id = the dependent's workspace, not the pushed repo's)."
          }
        ]
      },
      {
        "id": "f57b4d5f840e0b485f9fe50c",
        "decision": "satisfied",
        "requirement_quote": "  breaking_change_behavior: block | warn | notify",
        "detail": "The per-workspace DependencyPolicy carries all five specced knobs (breaking_change_behavior, max_version_drift, stale_dependency_alert_days, require_cascade_tests, auto_create_update_tasks) and is configurable via GET/PUT /api/v1/workspaces/:id/dependency-policy. block fails the merge-queue entry with reason \"breaking_change_policy\" until all breaking changes from the repo are acknowledged; warn logs and proceeds; notify proceeds silently. max_version_drift and stale_dependency_alert_days are enforced both push-time (drift applied on detection) and by the daily dep_staleness_check job (drift threshold + days-since-verified threshold, Stale marking, revert to Active when no longer stale). require_cascade_tests gates trigger_cascade_tests in both the merged and dependent workspaces (opt-out respected). auto_create_update_tasks gates the breaking-change and stale-dependency auto-tasks.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/dependency.rs",
            "quote": "pub breaking_change_behavior: BreakingChangeBehavior,",
            "detail": "The policy entity carries breaking_change_behavior alongside max_version_drift, stale_dependency_alert_days, require_cascade_tests, and auto_create_update_tasks — all five specced knobs in one per-workspace policy."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "gyre_domain::BreakingChangeBehavior::Block => {",
            "detail": "The `block` behavior is real enforcement: the merge-queue entry is failed (fail_queue_entry with \"breaking_change_policy\") when the repo has unacknowledged breaking changes; Warn logs and proceeds, Notify is a no-op — matching the specced effect table."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"dep_staleness_check\".to_string(),",
            "detail": "The stale_dependency_alert_days / max_version_drift knobs are exercised by a registered daily background job running dep_staleness::run_once, so the policy is continuously evaluated rather than only on push."
          }
        ]
      },
      {
        "id": "7014f11632cdabb887a52f2b",
        "decision": "satisfied",
        "requirement_quote": "2. For each dependent repo, the forge triggers a test run against Repo B's new version",
        "detail": "The cycle-3 execution gap (\"the forge knows the graph and runs the tests\" — tasks sat unspawned) is closed by task-280. On merge (and via the manual trigger POST /api/v1/repos/:id/cascade-tests/trigger, which reuses the same production path), trigger_cascade_tests creates one cascade-test task per dependent (gated on require_cascade_tests in both the merged and dependent workspaces, opt-outs skipped and counted) typed TaskType::Implementation — the discriminator the worker scheduler spawns — with the description rendered into the spawned agent's context prompt as the test-run procedure (pin the merged commit, run the suite, complete on pass / fail with output). Trigger-time context (merged repo id/name, merge SHA, dependent) is durably handed off through the kv store so results carry full merge identity. Outcomes are recorded as durable CascadeTestResult records (CascadeTestResultRepository port, mem/SQLite/PG); agent completion reports pass and agent failure reports failure with details (spawn.rs), which auto-creates a High-priority follow-up task with the failure details, emits cascade_test_failed to the workspace (orchestrator notification), notifies members, and pass/fail results are reported in the activity feed (cascade_test_passed/cascade_test_failed workspace events).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "task.task_type = Some(gyre_domain::TaskType::Implementation);",
            "detail": "The execution link: cascade-test tasks are typed Implementation so the signal-chain worker scheduler spawns an agent for them (untyped tasks are never spawned) — the forge itself runs the dependents' test suites, with the task description as the agent's instruction set."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "crate::merge_processor::report_cascade_test_result(&state, &task, true, None).await;",
            "detail": "The completion side of the loop: when an agent whose task is labeled cascade-test completes, the result is reported through the real path (the agent-fail endpoint reports failures with details at the sibling call site)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "if let Err(e) = state.cascade_test_results.record(&record).await {",
            "detail": "Durable result records: each outcome is persisted through the CascadeTestResultRepository port (surviving restarts), carrying dependent repo, merged repo, merge SHA, pass/fail, and failure details."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "// TASK-022: Cascade testing — trigger test tasks in dependent repos.",
            "detail": "The merge path itself invokes trigger_cascade_tests after a merge lands, so cascade runs are triggered by Repo B's merge as the spec requires (not only by manual API calls)."
          }
        ]
      },
      {
        "id": "551185c9c05657eddf50405e",
        "decision": "satisfied",
        "requirement_quote": "- The AIBOM flags stale dependencies in release reports",
        "detail": "The drift machinery was already real and is unchanged: compute_version_drift derives semver minor-delta drift, latest_semver_tag resolves the target's current version from its git tags, drift is applied push-time and by the daily dep_staleness_check job, Stale marking and \"Update {target} dependency from {pinned} to {current}\" auto-tasks are gated on auto_create_update_tasks. The cycle-3 gap — the AIBOM bullet — is closed by task-284: build_release_aibom now embeds a dependencies section built once from the REAL dependency store and shared by both the AIBOM and release-artifact responses: every outgoing edge with type/method/pinned/current/drift/status, the repo's dependent count, and the stale-dependency supply-chain risk list — one entry per Stale edge with a \"stale dependency: pinned ..., target current ..., drift ...\" risk string plus stale_count.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "async fn build_dependencies_section(",
            "detail": "The AIBOM's dependency-graph snapshot builder: reads the repo's outgoing edges and dependents from the real dependency store and is embedded in both the AIBOM and release-artifact responses so the two can never disagree about dependency/stale state."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "supply_chain_risk: format!(",
            "detail": "Stale dependencies are flagged as supply-chain risk in release reports — one AibomStaleDependency entry per Stale edge with pinned/current/drift and an explicit risk string, plus the stale count."
          },
          {
            "file": "crates/gyre-server/src/dep_staleness.rs",
            "quote": "if policy.auto_create_update_tasks {",
            "detail": "The drift auto-task: when a stale dependency is detected past the policy thresholds, an \"Update {target} dependency from {pinned} to {current}\" task is auto-filed in the source repo (the task the Repo Orchestrator dispatches an agent against)."
          },
          {
            "file": "crates/gyre-server/src/dep_staleness.rs",
            "quote": "crate::version_compute::compute_version_drift(pinned, current)",
            "detail": "Continuous drift monitoring: the daily job recomputes drift for every edge from the pinned version against the target's current version."
          }
        ]
      },
      {
        "id": "74607a9069c32b20ef245765",
        "decision": "satisfied",
        "requirement_quote": "    Repo C: pinned at v1.2.0 (3 versions behind, STALE), tests unknown",
        "detail": "The core blast-radius query is real: BFS over the reverse graph yields direct dependents (depth 0), transitive dependents (deeper, deduplicated), and the total. The cycle-3 response-field gaps are closed by task-281: BlastRadiusResponse now carries per-dependent health entries (BlastRadiusDependent with direct flag and test_status — \"passing\"/\"failing\" from the dependent's most recent durable CascadeTestResult against THIS repo, \"unknown\" when none) and the cascade_test_results list newest-first (\"not yet run\" when empty). Per-dependent pinned/current/drift health is served by the dependents edge listing (DependencyEdgeResponse carries version_pinned/target_version_current/version_drift/status) and the UI composes the two; breaking status is served by the breaking-changes list. Exposed via GET /api/v1/repos/:id/blast-radius, the spec-literal GET /api/v1/deps/repo/:id/impact, `gyre deps impact`, and the impact-analysis modal.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "let mut queue: VecDeque<(String, usize)> = VecDeque::new();",
            "detail": "The blast-radius BFS: direct dependents at depth 0, transitive beyond, deduplicated via the visited set, total returned — the direct/transitive/total shape of the specced response."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "let test_status = |rid: &str| -> &str {",
            "detail": "Per-dependent test status in the impact response: each dependent's most recent durable cascade result against this repo maps to passing/failing, unknown when none — the \"tests passing / tests unknown\" health fields of the specced response."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "pub cascade_test_results: Vec<CascadeTestResultResponse>,",
            "detail": "\"Cascade test results: not yet run\" is a real response field: the durable cascade results for merges in this repo are included newest-first in the blast-radius response."
          }
        ]
      },
      {
        "id": "79be3a435216d41c4ebe0660",
        "decision": "satisfied",
        "requirement_quote": "| `GET /api/v1/deps/graph` | GET | Full tenant-wide dependency graph |",
        "detail": "The cycle-3 gap (none of the spec's literal paths served — blocking review finding spec-endpoint-paths-not-served) is closed by task-282. All ten rows are registered at the spec-literal /api/v1/deps/* paths backed by the real handler cores that also serve the older /api/v1/dependencies/* and /api/v1/repos/:id/* routes: GET /api/v1/deps/graph (full graph) with the SAME path accepting ?workspace={id} for the workspace-scoped graph — query-parameter scoping on the graph endpoint as the spec's second row requires, with per-handler tenant containment (require_workspace_tenant_containment) on that query; per-repo dependents/dependencies/impact; stale; breaking; POST manual; DELETE :id (manual-only refusal for auto-detected edges, containment before any refusal detail); POST breaking/:id/acknowledge. The per-repo routes resolve the repo-to-workspace-to-tenant scope chain in the ABAC resolver; the flat write routes enforce per-handler tenant containment. matchit static-over-param priority keeps the flat DELETE distinct from the static segments.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/deps/graph\", get(dependencies::get_graph))",
            "detail": "The spec's literal graph path is served by the real graph handler — the route block registers all ten spec-literal rows (/deps/repo/:repo_id/dependents, /dependencies, /impact, /deps/stale, /deps/breaking, /deps/manual, /deps/:id, /deps/breaking/:id/acknowledge) alongside the implemented routes."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "if let Some(ws_id) = query.workspace.filter(|w| !w.is_empty()) {",
            "detail": "The ?workspace={id} query-parameter scoping on the graph endpoint: the same /api/v1/deps/graph path serves the workspace-scoped graph when the query param is present, the full graph otherwise."
          },
          {
            "file": "crates/gyre-server/src/api/dependencies.rs",
            "quote": "require_workspace_tenant_containment(&state, &auth, &ws_id).await?;",
            "detail": "The workspace query is tenant-contained per handler: the caller's tenant is compared against the workspace's tenant before the scoped graph is served (the flat path carries no repo segment for the middleware scope chain to resolve)."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/deps/breaking/:id/acknowledge\",",
            "detail": "The tenth row's literal path is registered (POST acknowledge), sharing the acknowledge core with the older route and adding per-handler tenant containment against the breaking change's source repo."
          }
        ]
      },
      {
        "id": "7df3052372738ce81043bfb9",
        "decision": "satisfied",
        "requirement_quote": "gyre deps add --target repo-b --type api    # Manual dependency declaration",
        "detail": "All eight specced commands exist with the specced flags and real implementations backed by the API client: `deps show` (this repo's or a named repo's dependencies and dependents, resolved from the git remote, with pinned/current/drift/status/method columns), `deps show --workspace` (workspace-scoped graph, workspace inferred from the git remote), `deps show --tenant` (tenant-wide graph), `deps graph --format dot` (DOT output, other formats refused), `deps impact <repo>` (blast radius with direct/transitive/total), `deps stale`, `deps breaking`, `deps add --target --type` (manual declaration), `deps acknowledge <id>`.",
        "evidence": [
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "DepsCommands::Acknowledge { id } => {",
            "detail": "The acknowledge subcommand is dispatched with a real implementation (acknowledge_breaking_change client call) — all eight subcommands in the DepsCommands enum have dispatch arms hitting the API client."
          },
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "print_dot_graph(&graph);",
            "detail": "`deps graph --format dot` fetches the real graph and renders DOT for visualization; non-dot formats are rejected rather than silently ignored."
          },
          {
            "file": "crates/gyre-cli/src/client.rs",
            "quote": "pub async fn acknowledge_breaking_change(&self, id: &str) -> Result<()> {",
            "detail": "The CLI client method hitting the server's acknowledge endpoint — the commands are real API consumers, not local stubs."
          }
        ]
      },
      {
        "id": "810eac26a99174ce812d04a3",
        "decision": "satisfied",
        "requirement_quote": "- **Impact analysis modal:** before merging a breaking change, show the full blast radius with cascade test results.",
        "detail": "The cycle-3 gap (the modal showed blast radius and per-repo health but not cascade test results) is closed by task-281. ImpactAnalysisModal now fetches blast radius, breaking changes, dependent edges, the workspace dependency policy, and the durable cascade test results in parallel; the per-dependent health table carries a Tests column (test status from the blast-radius response's dependents[].test_status, falling back to the cascade-test-results records); a Cascade Tests section lists each result with dependent repo, merge SHA, timestamp, pass/fail badge, and failure details, plus a Trigger Cascade Tests button that surfaces refusals (e.g. cascade testing disabled by policy) and reloads. The other three bullets remain real: DependencyGraph.svelte renders the interactive DAG at workspace and tenant scope with edges colored by type, stale/breaking coloring, and node-click impact; RepoDependencies.svelte is the repo detail dependency section with drift indicators and breaking alerts; DependencyHealthCard.svelte is the workspace dashboard aggregate (\"N repos with stale dependencies\", unacknowledged breaking count).",
        "evidence": [
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "const testStatusByRepo = $derived.by(() => {",
            "detail": "Per-dependent test status in the modal's health table: derived from the blast-radius response's dependents[].test_status, falling back to the cascade-test-results records — the same underlying durable store."
          },
          {
            "file": "web/src/components/ImpactAnalysisModal.svelte",
            "quote": "await api.triggerCascadeTests(repoId);",
            "detail": "The modal can trigger cascade tests through the real trigger endpoint (with the refusal surfaced to the user), and reloads to pick up the new results — the cascade results are live data, not decoration."
          },
          {
            "file": "web/src/components/DependencyGraph.svelte",
            "quote": "level. Nodes = repos, edges = dependencies colored by type. Stale edges in",
            "detail": "The graph visualization bullet: the DAG component documents and implements edges colored by dependency type with stale/breaking coloring at workspace and tenant scope, node click opening impact analysis."
          }
        ]
      },
      {
        "id": "589b5beaab7b22bb95d9b7ee",
        "decision": "satisfied",
        "requirement_quote": "- **Merge ordering:** cross-repo MR dependencies can be auto-created from the dependency graph. If Repo A's MR updates its dependency on Repo B, and Repo B has a pending MR with the new version, the forge creates the MR dependency automatically.",
        "detail": "All four bullets are real at this base. The cycle-3 gap bullets were closed by task-283. Merge ordering: at MR creation, detect_graph_deps diffs the MR's branch against the default branch and, for each changed file that is a dependency declaration (a source_artifact of an existing edge), links the MR to the most recently updated open MR in the target repo as a DependencyGraph-sourced depends_on entry (one per target repo; fresh MR id makes cycles impossible by construction); reevaluate_graph_deps_of_dependent_repos re-evaluates dependents' open MRs when a target repo's MR set changes (creation and close), and closed target MRs drop the stale edge rather than blocking forever. Budget allocation: check_spawn_budget_for_repo weights the inherited workspace spawn caps by the repo's dependent count read from the live graph (repo_dependent_count, +25% per dependent) — repos with many dependents get proportionally higher effective agent budgets. Priority decisions and cross-repo task creation were already real: the breaking-change cascade files High-priority tasks in dependent repos, giving the orchestrator graph-driven prioritization and task creation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/merge_requests.rs",
            "quote": "let graph_deps = detect_graph_deps(&state, &repo, &req.source_branch).await;",
            "detail": "Merge ordering: MR creation runs graph-driven dependency detection, and the produced DependencyGraph-sourced edges are merged into the MR's depends_on (cycle-guarded, re-evaluated on pushes and target-MR lifecycle changes)."
          },
          {
            "file": "crates/gyre-server/src/api/budget.rs",
            "quote": "let weighted = weight_inherited_caps(&config, dependent_boost_factor(dependents));",
            "detail": "Budget allocation: when a repo inherits the workspace budget (no override), its spawn caps are weighted by the dependent count read from the live dependency graph — repos with many dependents warrant and receive higher effective budgets."
          },
          {
            "file": "crates/gyre-server/src/api/merge_requests.rs",
            "quote": "\"auto-detected graph-driven MR dependency\"",
            "detail": "The auto-created MR dependency is an observable, logged production artifact (DependencySource::DependencyGraph edge with a reason naming the artifact that ties the repos), not a silent heuristic."
          }
        ]
      },
      {
        "id": "605ecacdbe3cbbfa544aee46",
        "decision": "context",
        "requirement_quote": "- **Versioning** (`ci-docs-release.md`): conventional commits + semver are how the forge detects breaking changes. The `feat!:` or `BREAKING CHANGE:` footer triggers the cascade.",
        "detail": "Cross-reference section mapping this spec's relationships to six sibling specs (merge-dependencies, spec-links, platform-model, spec-lifecycle, supply-chain, ci-docs-release). It records how the dependency graph relates to those systems rather than imposing requirements of its own; the actionable requirements live in those specs' own audits (e.g. the AIBOM/supply-chain integration this section points to is verified under Version Drift Tracking above, and the conventional-commit trigger under Breaking Change Detection).",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "4390a1b94a03e318854cbbec",
        "decision": "gap",
        "requirement_quote": "    Orphaned,     // Target repo was deleted or archived",
        "detail": "The entity shape is complete: the production DependencyEdge carries every specced field (id, source/target repo ids, dependency_type, source_artifact, target_artifact, version_pinned, target_version_current, version_drift, detection_method, detected_at, last_verified_at, status) and all four statuses exist and are set by real transitions (Active on create, Stale by drift/staleness, Breaking by the breaking-change cascade). The gap is the documented semantics of Orphaned itself: \"Target repo was deleted or archived\". No production path marks a dependents' edge Orphaned when the target repo is archived or deleted. archive_repo cancels the repo's tasks and updates the repo row but never touches dependency edges; delete_repo cleans up spec links and spec assertion results but leaves dependency edges untouched; the daily dep_staleness_check job explicitly skips edges already Orphaned and never orphans anything itself; the only Orphaned transition is reconcile_dependencies' no-longer-detected check, which runs only when the SOURCE (dependent) repo is pushed and only for detection methods that ran in that push. Consequences: a deleted target's edges keep their prior status indefinitely until the dependent happens to push again (lazy, best-effort coverage of the \"deleted\" half), and an archived target is never orphaned at all — archived repos remain in RepoRepository::list, so detection keeps matching them and reconcile re-confirms the edge as detected. The spec's own comment names both causes; neither is wired.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    // Clean up the spec-link graph rows sourced from this repo (spec-links.md",
            "detail": "delete_repo's cleanup list, inspected in full: it deletes the repo row, spec-link graph rows, spec assertion results, and the on-disk git directory — dependency edges pointing at the deleted repo are absent from the handler; nothing marks them Orphaned."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    repo.archive();\n    repo.updated_at = now_secs();",
            "detail": "archive_repo transitions the repo row to archived and proceeds to cancel active tasks — the inspected handler contains no dependency-edge handling, so edges whose target is this repo remain Active/Stale despite the spec's Orphaned semantics for archived targets."
          },
          {
            "file": "crates/gyre-server/src/dep_staleness.rs",
            "quote": "if edge.status == DependencyStatus::Breaking || edge.status == DependencyStatus::Orphaned {",
            "detail": "The daily dep_staleness_check job (the continuous monitor) treats Orphaned as \"managed by other systems\" and skips it — it never orphans edges itself, including edges whose target repo no longer resolves (target_repo None yields target_version None, which produces no status change to Orphaned)."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "if !detected_keys.contains(&key) && edge.status != DependencyStatus::Orphaned {",
            "detail": "The only Orphaned transition in production: reconcile_dependencies marks an edge Orphaned when its declaration is no longer detected — but this runs only on a push to the SOURCE repo, and only for edges whose detection_method ran in that push, so target deletion is covered lazily at best and target archival never (the archived target still matches a known repo and is re-detected as present)."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "fd5036bd1272469fc55a2b304ac10a7db63c95a6d5f546b37a3c2c6c86307149"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "5a5676ba50a366b0ed75b1297e3eb95f1490d18d5983758b869da2201e53e34d"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/repository.rs",
        "sha256": "489e60b23373824c1030c15dbf7a47218e3843a1dc85584335c6aa345c5f52a6"
      },
      {
        "file": "crates/gyre-domain/src/dependency.rs",
        "sha256": "6be7ab1232011f6770ae0ca82faedd1bf35d9b6d237a9d1dd7895ea828300c86"
      },
      {
        "file": "crates/gyre-server/src/api/dependencies.rs",
        "sha256": "ae898b1547072b27c709ffd6fefd14de3dc578f4dc228ba4e00d2cafbd26be05"
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "sha256": "29546768ffe40cb08f036e665ffc0e93ab784f8791d89aa2bb7ae4102fd7e6bf"
      },
      {
        "file": "crates/gyre-server/src/dep_staleness.rs",
        "sha256": "4583a169950276e541b131a94d86b2702aa7098de988c1be425c3aa73e710fb1"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "4dc88c1516f2135573d94cf255c246ef1b73ce7b3849683669a6543b21823e60"
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "sha256": "cac01a8dd660c75389f91f5de01f6dce9c4c0029bd27b3c5ae24560e6e889bb8"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "012f5b3fc89e4f82182168f7c32bb966544d4cfd90201147deb0b6a7971d6d40"
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "sha256": "b16446e777779fcd02857cd61e536961e6c3283b2443339bd35935e9f9ec5c4d"
      },
      {
        "file": "crates/gyre-cli/src/client.rs",
        "sha256": "45c068f9ab1e34bbf3b43d962d35f773dd997cb8ca6910ecb2b66decd98a4a84"
      },
      {
        "file": "web/src/components/ImpactAnalysisModal.svelte",
        "sha256": "8376d265384432a545bd67c11199e94609606b7ff32169b2648b834e24b732f8"
      },
      {
        "file": "web/src/components/DependencyGraph.svelte",
        "sha256": "c1beed3d3c56f90827ca25152680848b189a8f745a63a437419538749aca2ee3"
      },
      {
        "file": "crates/gyre-server/src/api/merge_requests.rs",
        "sha256": "d9491896cdec38612188ad4b8d3adf06685b7613f3b911eeb964e866d6a98d88"
      },
      {
        "file": "crates/gyre-server/src/api/budget.rs",
        "sha256": "427482dce0027b0410e0607cfac5d77294b5be097c27880cd32c1f92282237cf"
      }
    ]
  },
  "plan": {
    "audit": "df68fabd30ceb4d068ee76dc8a76d6bb809de80a59eb3538d01dceddaf614d76",
    "tasks": [
      {
        "key": "dep-edge-orphaned-on-target-archive-delete",
        "title": "Dependency graph: transition dependents' edges to Orphaned when the target repo is archived or deleted",
        "covers": [
          "4390a1b94a03e318854cbbec"
        ],
        "required_behavior": "Per dependency-graph.md §Dependency Entity, DependencyStatus::Orphaned means \"Target repo was deleted or archived\" — that transition must actually happen in production code, on all four surfaces where it is currently missing: (1) archive_repo (crates/gyre-server/src/api/repos.rs): after the repo row transitions to archived, every stored edge with target_repo_id == repo.id that is not already Orphaned is marked Orphaned and persisted through the existing DependencyRepository port (state.dependencies.list_dependents + save); edges are never deleted; a per-edge save failure logs a warning without aborting the archive, matching the handler's existing best-effort cleanup discipline. (2) delete_repo (same file): edges whose target is the deleted repo are marked Orphaned, not removed — the edge rows survive the repo row deletion, consistent with the spec's reconcile philosophy (\"don't delete - might be a detection gap\"). (3) detect_dependencies_on_push (crates/gyre-server/src/git_http.rs): archived repos are excluded from the known-repo match set on every detection path (Cargo.toml path/git refs, package.json, go.mod, pyproject.toml, manifest spec links, openapi/proto/mcp contract matching), so no new edge is created targeting an archived repo and reconcile does not resurrect an Orphaned edge whose target is archived back to Active (git_http.rs:4446 currently un-orphans on re-detection); unarchive makes the target matchable again and the next push to the source re-activates the edge through the existing reconcile un-orphan path. (4) dep_staleness_check (crates/gyre-server/src/dep_staleness.rs): the continuous monitor transitions edges whose target repo no longer resolves in the repo list (deleted, including edges stranded by pre-fix deletions) or is archived to Orphaned, evaluated before the existing Breaking/Orphaned skip so the skip only ever sees already-orphaned edges. No port-trait changes (list_dependents already exists on DependencyRepository with SQLite and PostgreSQL adapters), no spec changes, no new routes — the spec contract is implemented exactly as written.",
        "acceptance": [
          "Storage-backed test (real SQLite DependencyRepository, not a mem stand-in): repos A and B with an edge A→B; archiving B through the archive_repo handler transitions the edge to status Orphaned, visible via the dependency port / GET /api/v1/deps/repo/{B}/dependents, and the edge row still exists afterwards.",
          "Pushing to A while B is archived (dependency declaration still present in A's tree) leaves the edge Orphaned — no resurrection to Active and no new edge created targeting the archived repo.",
          "Unarchiving B and pushing to A returns the edge to Active via the existing reconcile un-orphan path.",
          "Deleting B (after archive) leaves the edges targeting B present in storage with status Orphaned — they are not deleted with the repo.",
          "An edge whose target repo row no longer resolves (or is archived) is transitioned to Orphaned by running dep_staleness_check.",
          "cargo build and cargo test pass; scripts/check-arch.sh passes; no new entries in any scripts/*-exemptions.txt; the per-edge port contract (save) is honored by both storage adapters unchanged."
        ],
        "negative_probe": "Create a real detected Active edge A→B (push A containing `repo-b = { path = \"../repo-b\" }` in Cargo.toml with B a known tenant repo), then archive B and push A again. On current code this fails the specced behavior: archive_repo never touches dependency edges (the handler's post-archive work only cancels tasks and emits events), and detect_dependencies_on_push matches archived repos because state.repos.list() still includes them, so reconcile re-confirms the edge and would even un-orphan it (git_http.rs:4446). The probe asserts status == Orphaned after archive + re-push — it must fail before the fix and pass after it. A second probe for the monitor: remove the target repo row (a pre-fix deletion leaves exactly this state) and run dep_staleness_check — current code leaves the edge untouched (the job only skips already-Orphaned edges and never orphans anything; target_repo None yields target_version None with no status change), so asserting Orphaned there kills the real defect rather than checking a symbol exists.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "dep-edge-orphaned-on-target-archive-delete": "task-341",
    "_publication": "task-342",
    "_base": "0b33cb3a4a586c587e2566ecbcd82c03da94d84b"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
