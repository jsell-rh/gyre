---
title: "Publish reviewed spec backlog for specs/system/spec-links.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/spec-links.md",
  "plan_id": "88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "5a915155d9bc8230bd5286f504b41d189fdc09e58c54fc28e9df0e763fc36e91",
    "sections": [
      {
        "id": "22f01b1b9b4b8e1bd0fca177",
        "decision": "context",
        "requirement_quote": "The current system has informal cross-references (prose like \"see `agent-gates.md`\") but no machine-readable links. Prose references don't survive renames, don't cascade invalidation, and can't be mechanically enforced.",
        "detail": "Document title/introduction. Rationale for the machine-readable link graph; every implementable requirement lives in the child headings audited individually below (Link Types, Manifest Integration, Cross-Repo Links, Forge-Maintained Spec Graph, Mechanical Gates, Querying the Graph, Accountability Integration).",
        "evidence": []
      },
      {
        "id": "324d2464ec991f92b07b547e",
        "decision": "context",
        "requirement_quote": "At enterprise scale (hundreds of repos, many teams), specs form a massive graph. Without machine-readable links between specs, you get:",
        "detail": "Problem statement / motivation only (contradictions, stale references, circular dependencies, false positives). No implementable requirement beyond what the child sections specify.",
        "evidence": []
      },
      {
        "id": "675c0dc7be4a127cb63039f3",
        "decision": "gap",
        "requirement_quote": "Every link is SHA-pinned. Not \"this spec relates to identity-security.md\" but \"this spec relates to identity-security.md@abc123.\" When the linked spec changes, the link becomes stale and the forge flags it.",
        "detail": "The six typed link variants and most per-type enforcement are real (implements/conflicts_with checked at approval, supersedes deprecation on approval, extends drift handling, references deliberately unenforced). But the section's foundational contract -- every link SHA-pinned so a changed target forces the link stale -- is not enforced: the manifest link schema parses target_sha as optional with no validation anywhere in the push/sync path, an unpinned link is stored with status \"active\", and both staleness paths (push-time outbound check and the daily staleness job) skip links with no pinned target_sha, so such a link can never be flagged no matter how the target changes. The table's depends_on row (\"Implementation tasks blocked until dependency spec's implementation is complete\") is also not implemented as stated -- the forge blocks approval of the source spec instead of blocking implementation tasks and models completeness as approval status; that deviation is fully accounted under Approval Gates (dcd440115bac4622a351c857) to avoid double counting.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    /// SHA the link was pinned to. Stale if target spec SHA advances.\n    pub target_sha: Option<String>,",
            "detail": "Manifest link schema: target_sha is Option<String>. No code path validates that a declared link carries a SHA, so 'every link is SHA-pinned' is unenforced and unpinned links are accepted into the graph."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "        // Only check links that have a pinned target_sha and are currently active.\n        let Some(pinned_sha) = &link.target_sha else {\n            continue;\n        };",
            "detail": "Daily staleness job: links without a pinned target_sha are skipped with `continue`, so they can never be marked stale -- the 'when the linked spec changes, the link becomes stale and the forge flags it' guarantee does not hold for them."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "            // Staleness detection: if the link has a pinned target_sha but the target\n            // spec's current SHA in the ledger differs, mark the link as stale.\n            if let Some(pinned_sha) = &link.target_sha {",
            "detail": "Push-time outbound staleness check likewise only runs `if let Some(pinned_sha) = &link.target_sha`; unpinned links stay \"active\" forever."
          }
        ]
      },
      {
        "id": "cb357bc5aba4adacc6a155fc",
        "decision": "satisfied",
        "requirement_quote": "Links are declared in the spec's manifest entry:",
        "detail": "Implemented as specced: SpecEntry carries a `links` vec parsed from the manifest YAML with type/target/target_sha/reason exactly matching the section's example shape (approval, supersedes, depends_on, extends, conflicts_with entries all parse), and sync_spec_ledger step 6 iterates every manifest entry's links, resolves each target, and replaces the durable spec_links rows for the manifest's source paths (including clearing rows for links removed from the manifest).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    /// Links to other specs (machine-readable graph edges).\n    #[serde(default)]\n    pub links: Vec<SpecLink>,",
            "detail": "Manifest entry schema: links are declared per spec entry in the manifest, machine-readable, with the link type, target, target_sha and reason fields shown in the spec's example."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    // 6. Process spec links from manifest — enforce supersedes/extends, update links store.\n    {\n        let mut new_links: Vec<SpecLinkEntry> = Vec::new();\n        for entry in &manifest.specs {\n            for link in &entry.links {\n                let id = gyre_domain::spec_links::spec_link_id(",
            "detail": "sync_spec_ledger step 6 processes spec links from the manifest on every push to the default branch, building SpecLinkEntry rows for each declared link (id, target resolution, target_sha, reason)."
          }
        ]
      },
      {
        "id": "ff74a147e0ca35a407836447",
        "decision": "gap",
        "requirement_quote": "The forge resolves cross-repo links by querying the tenant-wide spec graph. The link is validated:\n- Target workspace exists and the linking user has read access\n- Target repo exists within that workspace\n- Target spec path exists in the target repo's manifest\n- Target SHA matches (or is flagged as stale if the spec has moved on)",
        "detail": "Target formats (@{workspace_slug}/{repo_name}/{spec_path}, @{repo_name}/{spec_path}, bare path) parse correctly, cross-workspace workspace+repo existence is resolved tenant-scoped, and target-SHA drift is flagged stale against the ledger. Four validation requirements are not delivered: (1) same-workspace cross-repo links (@{repo_name}/{spec_path}, an explicitly specced format) never resolve -- sync_spec_ledger passes None as the workspace to resolve_repo_by_name, which returns None whenever workspace_id is None, so every such link is stored \"unresolved\" with target_repo_id NULL even when the repo exists, and the staleness job's re-resolution explicitly leaves CrossRepo links unresolved; (2) the linking user's read access is never validated -- link resolution runs post-push with no caller identity in scope; (3) target spec path existence in the target repo's manifest is never validated -- a link whose repo resolves is marked \"active\" without checking the target repo's manifest contains the path; (4) the staleness checker's cross-workspace re-resolution queries under a hardcoded fabricated tenant id \"default\", so in any other tenant the workspace slug never resolves (also violating the repo's fabricated-scope-defaults invariant).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                        let resolved_repo_id = resolve_repo_by_name(\n                            repos, workspaces, tenant_id,\n                            None, // same workspace — we'll look up by name across all\n                            repo_name,\n                        )\n                        .await;",
            "detail": "Same-workspace cross-repo resolution at push time passes None as workspace_id; the comment claims 'look up by name across all' but the callee cannot."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    let ws_id = workspace_id?;\n    repos",
            "detail": "resolve_repo_by_name immediately returns None when workspace_id is None (`let ws_id = workspace_id?;`), so every @{repo_name}/path link is permanently unresolved despite the repo existing -- 'Target repo exists within that workspace' is never positively validated for this format."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "            crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {\n                // For cross-repo same-workspace links, we'd need the source workspace.\n                // Without it, we can't resolve. Leave as unresolved.",
            "detail": "The staleness checker (which the spec says resolves NULL target_repo_id links later) explicitly refuses to re-resolve same-workspace cross-repo links, leaving them unresolved forever."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "                let ws = state\n                    .workspaces\n                    .find_by_slug(&Id::new(\"default\"), workspace_slug)",
            "detail": "Cross-workspace re-resolution fabricates the tenant scope as the literal id \"default\" instead of the source link's tenant, so workspaces in any other tenant never resolve."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                        let status = if resolved_repo_id.is_some() {\n                            \"active\".to_string()\n                        } else {\n                            \"unresolved\".to_string()",
            "detail": "A cross-repo link whose repo resolves is marked \"active\" with no check that the target spec path exists in the target repo's manifest -- only repo existence is consulted, never the target manifest; the linking user's read access appears nowhere in this path."
          }
        ]
      },
      {
        "id": "6a37bd9a69d9a133a99fb90a",
        "decision": "satisfied",
        "requirement_quote": "The forge maintains a tenant-wide directed graph of all spec links:",
        "detail": "Implemented with the exact 13-column spec_links schema from the section's SQL (id, source_repo_id, source_path, source_sha, link_type, target_repo_id NULL for unresolved links, target_path, target_sha, target_display, reason, status default 'active', created_at, stale_since) as a real migration, behind the gyre_ports::SpecLinkRepository port with SQLite and Postgres adapters. The in-memory SpecLinksStore is a hot cache: it is rebuilt from the durable table at boot (load_spec_links_into_store) and every mutation (manifest sync replace_for_source, staleness transitions, re-resolution) is written through to the repository, so the graph survives restarts.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/migrations/2026-11-08-000061_spec_links/up.sql",
            "quote": "CREATE TABLE IF NOT EXISTS spec_links (\n    id              TEXT PRIMARY KEY,\n    source_repo_id  TEXT NOT NULL DEFAULT '',",
            "detail": "Migration creates the spec_links table with the exact column set and defaults from the spec's SQL block (task-198)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    /// Durable source of truth; `spec_links_store` is the hot in-memory cache\n    /// loaded from this at boot and written through on every mutation.\n    pub spec_link_repo: Arc<dyn gyre_ports::SpecLinkRepository>,",
            "detail": "AppState carries the durable SpecLinkRepository as the source of truth with the in-memory store as a write-through cache."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "                        if !persisted.is_empty() {\n                            tracing::info!(\n                                count = persisted.len(),\n                                \"spec-links: loaded persisted spec link graph\"\n                            );\n                        }\n                        *store.lock().await = persisted;",
            "detail": "At startup the persisted graph is loaded from the repository into the hot cache, so links survive restarts rather than living only in memory."
          }
        ]
      },
      {
        "id": "b222f4b88e85892d669131d7",
        "decision": "gap",
        "requirement_quote": "| `broken` | Target spec was deleted or is no longer in the manifest |\n| `conflicted` | A `conflicts_with` link where both specs are approved (violation) |",
        "detail": "active (default) and stale (push-time outbound/inbound detection plus the daily job) are delivered. Two of the four statuses are not delivered as stored link states: (1) broken is only set when the target is absent from the spec ledger entirely -- but a spec removed from the manifest is kept in the ledger as Deprecated (never deleted), so links targeting it remain status \"active\" forever; the \"no longer in the manifest\" half of the broken definition never fires, and the daily job's Ok(Some(entry)) path leaves the link untouched. (2) conflicted is never assigned as a status anywhere in the codebase -- both-approved conflicts are only computed on the fly by GET /api/v1/specs/conflicts and the patrol check; the status column (documented in the domain type as one of the five health values) can never hold \"conflicted\", so the stored graph cannot be relied on for either condition. Additionally the stored statuses that do exist are a one-way ratchet: the daily staleness pass skips links already marked broken (\"Already known stale/broken — skip\"), so a target spec re-added to its manifest or restored to the ledger leaves inbound links broken forever even though the defining condition (\"Target spec was deleted or is no longer in the manifest\") has ceased; the stored graph the spec promises must reflect current conditions, and GET /api/v1/specs/conflicts is required to return \"All active conflicts\" — which a never-reverting conflicted status would misreport as active forever once one exists.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "            Ok(None) => {\n                // Target spec not in ledger — mark as broken.\n                mark_link_status(state, &link.id, \"broken\", now_secs).await;",
            "detail": "The only code path that sets \"broken\": target lookup returns Ok(None) (not in the ledger at all). Since manifest removal keeps the ledger entry, this path does not cover 'no longer in the manifest'."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                e.approval_status = ApprovalStatus::Deprecated;\n                e.updated_at = now;\n                let _ = ledger.save(&e).await;\n            }",
            "detail": "A spec removed from the manifest is marked Deprecated and kept in the ledger (save, not delete) -- so links targeting it find Ok(Some(entry)) in the staleness job, the SHA is unchanged, and the link stays \"active\" instead of \"broken\"."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    // An \"active conflict\" is a conflicts_with link where both specs are approved.\n    // spec-links.md §Link Status: conflicted = both specs approved (violation).",
            "detail": "Conflicted links are computed on the fly by the conflicts endpoint rather than ever being stored: no code assigns the \"conflicted\" status to a link row."
          },
          {
            "file": "crates/gyre-domain/src/spec_links.rs",
            "quote": "    /// Link health: \"active\" | \"stale\" | \"broken\" | \"conflicted\" | \"unresolved\"",
            "detail": "The domain type documents \"conflicted\" as a link health value, but a repository-wide search finds no producer: the value is dead in the stored graph."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "        if link.status == \"stale\" || link.status == \"broken\" {\n            // Already known stale/broken — skip.\n            continue;\n        }",
            "detail": "The staleness pass skips links already marked broken, so a target that returns to the ledger or its repo's manifest never transitions the link back — \"broken\" is permanent once set, a one-way ratchet rather than a reflection of current conditions."
          }
        ]
      },
      {
        "id": "b50c96c02e383e13c9d73a06",
        "decision": "satisfied",
        "requirement_quote": "When any spec changes (new SHA), the forge:\n1. Queries `spec_links` for all links where `target_path` matches the changed spec\n2. Marks those links as `stale` (target_sha no longer matches)\n3. Creates drift-review tasks in the source specs' repos",
        "detail": "Steps 1-3 are implemented at push time in both directions. Outbound: when a manifest is synced, each new link's pinned target_sha is compared to the target's current ledger SHA and mismatches mark the link stale. Inbound (step 6b): when a push changes spec SHAs, the forge iterates the whole (tenant-wide) links store for links whose target_path matches a changed spec, marks them stale with stale_since, persists the mutation to the durable table, and creates real drift-review Task entities in the source spec's repo (High priority, drift-review label, repo/workspace scoped from the link). A daily background job re-checks all pinned SHAs, marks targets missing from the ledger broken, and re-resolves unresolved cross-workspace links. `references` links are excluded from drift tasks per the Link Types table (\"No mechanical enforcement\"). Steps 4-6 of the list are commentary (\"may need\", \"should already\"), not separate mechanisms.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "            if let Some(pinned_sha) = &link.target_sha {\n                if let Ok(Some(target_entry)) = ledger.find_by_path(&link.target_path).await {\n                    let current_sha = &target_entry.current_sha;\n                    if !current_sha.is_empty() && current_sha != pinned_sha {",
            "detail": "Outbound staleness: a link whose pinned target_sha differs from the ledger's current SHA is marked stale at sync time."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "            if changed_set.contains(link.target_path.as_str())\n                && link.status != \"stale\"\n                && link.status != \"broken\"\n            {",
            "detail": "Inbound staleness (spec change): links whose target_path matches a changed spec and that are not already stale/broken are marked stale -- steps 1 and 2 of the section."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "\n    let mut task = gyre_domain::Task::new(task_id, title, now);\n    task.description = Some(description);",
            "detail": "Step 3: a real Task entity with the drift-review label is created in the source spec's repo (repo_id/workspace_id taken from the link's source scope), not just a log line."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "            |state| async move { crate::spec_link_staleness::run_once(&state).await },",
            "detail": "The staleness checker is also registered as a recurring background job, providing the daily re-check the section's staleness model implies."
          }
        ]
      },
      {
        "id": "867470114391d12eb64457f2",
        "decision": "context",
        "requirement_quote": "Mechanical Gates",
        "detail": "Structural parent heading with no independent prose; its requirements are entirely the Approval Gates, Merge Gates, and Cycle Detection subsections, each audited and dispositioned individually below.",
        "evidence": []
      },
      {
        "id": "dcd440115bac4622a351c857",
        "decision": "gap",
        "requirement_quote": "| `implements` | Source spec cannot be approved until target (parent) spec is approved at the pinned SHA |\n| `depends_on` | Implementation tasks for source spec are blocked until target spec's implementation tasks are complete |\n| `conflicts_with` | Both specs cannot have `approval_status: approved` simultaneously. The forge rejects the second approval and creates a conflict-resolution task. |",
        "detail": "Two rows are really enforced: supersedes (approving the source automatically marks every superseded target Deprecated in the ledger, with a deprecation event) and extends (a changed target marks the extending spec drifted and resets its approval to Pending, in both the outbound and inbound sync paths). Three rows deviate from the contract: (1) depends_on -- the gate blocks APPROVAL of the source spec, not its implementation tasks, and judges \"implementation complete\" by the target's ledger approval_status; per spec-lifecycle.md approval precedes decomposition, so an approved target with open/incomplete implementation tasks passes the gate -- the specced signal (target spec's implementation tasks complete) is never consulted (no Task query anywhere in the gate). (2) implements -- the gate checks only the parent's current approval_status and never compares the approval to the link's pinned target_sha, so a source can be approved against a parent version it does not pin (\"approved at the pinned SHA\" is not verified). (3) conflicts_with -- the second approval is rejected with the specced message, but no conflict-resolution task is created anywhere in the rejection path (the handler returns InvalidInput immediately; a repository-wide search for conflict-resolution task creation finds none).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                SpecLinkType::DependsOn => {\n                    // TASK-016: Target spec's implementation must be complete (approved)\n                    // before the depending spec can be approved.\n                    // spec-links.md §Approval Gates: \"Implementation tasks for source spec\n                    // are blocked until target spec's implementation tasks are complete.\"\n                    if let Some(target) = state.spec_ledger.find_by_path(&link.target_path).await? {",
            "detail": "depends_on gate: blocks approval of the source spec and models 'implementation complete' as the target's approval_status -- the specced mechanism (blocking implementation tasks until the target's implementation tasks are complete) is absent."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                SpecLinkType::Implements => {\n                    // Parent spec must be approved before this spec can be approved.\n                    if let Some(parent) = state.spec_ledger.find_by_path(&link.target_path).await? {\n                        if parent.approval_status != ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': implements '{}' which is not yet approved\",\n                                spec_path, link.target_path",
            "detail": "implements gate: checks only parent.approval_status != Approved; the link's pinned target_sha is never compared, so 'approved at the pinned SHA' is not enforced."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                    {\n                        if conflicting.approval_status == ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': conflicts with '{}' which is already approved — resolve the conflict first\",\n                                spec_path, link.target_path",
            "detail": "conflicts_with gate: rejects the second approval but immediately returns the error -- no conflict-resolution task is created, contrary to 'The forge rejects the second approval and creates a conflict-resolution task.'"
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                if target_entry.approval_status != ApprovalStatus::Deprecated {\n                    target_entry.approval_status = ApprovalStatus::Deprecated;\n                    target_entry.updated_at = now;\n                    let _ = state.spec_ledger.save(&target_entry).await;",
            "detail": "supersedes row is satisfied: approving the superseding spec marks the target Deprecated in the ledger (plus a StaleSpecWarning deprecation event)."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                            source_entry.drift_status = \"drifted\".to_string();\n                            source_entry.approval_status = ApprovalStatus::Pending;\n                            source_entry.updated_at = now;",
            "detail": "extends row is satisfied: when the target changes, the extending spec is marked drifted and its approval is reset to Pending (both in the outbound sync path shown and the inbound path)."
          }
        ]
      },
      {
        "id": "cb0d56cefcb50d54be0121c4",
        "decision": "gap",
        "requirement_quote": "- If the spec `depends_on` another spec that isn't implemented yet, the MR gets a warning (not a block - the code might be valid, but the dependency isn't satisfied yet)",
        "detail": "Two of the three bullets are real: the superseded-spec rejection (Check 1 rejects the merge with exactly the specced message \"spec '{path}' has been superseded by '{new_path}'. Update your spec_ref.\") and the conflicts_with block (Check 2 blocks, bidirectionally, when the conflicting spec is approved). The depends_on-warning bullet is a gap: 'isn't implemented yet' is judged by the target spec's ledger approval status, not by implementation-task completeness. The spec's own vocabulary denotes task completeness (Approval Gates: \"until target spec's implementation tasks are complete\"), and per spec-lifecycle.md approval precedes decomposition, so approval never implied implementation -- the warning stays silent exactly in the common case the bullet exists for (target spec approved, implementation tasks incomplete). This corrects the prior cycle's 'satisfied' disposition per the controller review feedback; the warning event itself (StaleSpecWarning, warn-not-block) is correctly shaped. Per controller review feedback this includes the zero-task case: a depends_on target that has never been decomposed (zero implementation tasks — the normal state per spec-lifecycle.md, where approval precedes decomposition) is not implemented and its dependency is not satisfied, so the warning must fire there too; the approval-status predicate stays silent for every Approved target regardless of implementation-task state, including that most common case.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                if let Ok(Some(target)) = state.spec_ledger.find_by_path(&link.target_path).await {\n                    if target.approval_status != crate::spec_registry::ApprovalStatus::Approved {\n                        warn!(",
            "detail": "Check 3 predicate: the depends_on warning fires only when the target's approval_status != Approved. A target that is Approved with open implementation tasks produces no warning, so 'isn't implemented yet' is approximated by approval rather than judged by implementation completeness."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "            if link.link_type == crate::spec_registry::SpecLinkType::Supersedes\n                && link.target_path == spec_path\n            {\n                let reason = format!(\n                    \"spec '{}' has been superseded by '{}'. Update your spec_ref.\",\n                    spec_path, link.source_path\n                );",
            "detail": "Superseded-spec bullet is satisfied: the merge is rejected with the specced message whenever the referenced spec has an inbound supersedes link."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                        let reason = format!(\n                            \"spec '{}' conflicts with approved spec '{}' — resolve the conflict first\",\n                            spec_path, other_spec_path",
            "detail": "Conflicts_with bullet is satisfied: the merge is blocked when the referenced spec conflicts with an approved spec (checked in both directions)."
          }
        ]
      },
      {
        "id": "8772dba48e530c04a3d2f953",
        "decision": "gap",
        "requirement_quote": "The forge rejects manifest changes that would create cycles in the spec graph:\n- A `depends_on` B `depends_on` C `depends_on` A -> rejected",
        "detail": "Push-time rejection is real and the excluded types match the spec (conflicts_with bidirectional, references and supersedes excluded; depends_on/implements/extends participate), with default-branch pushes undone and a PushRejected event plus audit on rejection. The gap is detection scope: detect_link_cycles builds its adjacency list exclusively from the single manifest being pushed -- the forge's tenant-wide spec graph (the durable spec_links table, other repos' manifests, or even previously synced links of the same repo) is never consulted. A cycle whose edges are declared in two different manifests or repos -- the enterprise case this spec's Problem section exists for -- is therefore accepted: each push only sees its own edges, so A→B in repo 1 and B→A in repo 2 both land cleanly and the cycle lives permanently in the forge-maintained graph.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    let mut adj: HashMap<String, Vec<String>> = HashMap::new();\n    for entry in &manifest.specs {\n        for link in &entry.links {\n            match link.link_type {",
            "detail": "Cycle detection adjacency is built only from `manifest.specs` of the one manifest being pushed; links already in the forge graph (other repos/manifests, the durable spec_links table) are never consulted, so cross-manifest cycles are not detected."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {\n            if let Err(rejection) =\n                crate::spec_registry::check_spec_link_cycles(&repo_path, &update.new_sha).await\n            {",
            "detail": "The rejection wiring itself is real: a failing cycle check on a default-branch push undoes the ref updates (rejection side confirmed; only the detection scope is deficient)."
          }
        ]
      },
      {
        "id": "5d3fd88412e043612c794558",
        "decision": "context",
        "requirement_quote": "Querying the Graph",
        "detail": "Structural parent heading introducing the API, CLI, and UI subsections; no independent requirement. Each subsection is audited and dispositioned individually below.",
        "evidence": []
      },
      {
        "id": "58102d3a27f920beeb3fba71",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/specs/graph?repo={id}` | GET | Specs for one repo with all outbound/inbound links |",
        "detail": "Six of the seven specced endpoints exist and query the real links store and spec ledger: GET /api/v1/specs/graph (full graph), /specs/{path}/links (both outbound and inbound), /specs/{path}/dependents and /dependencies (impact analysis over depends_on/implements), /specs/stale-links, and /specs/conflicts (both-approved conflicts_with links). Two defects keep the section open. (1) The repo-filtered graph variant is not implemented: get_spec_graph takes no query parameter and always returns every ledger node and every store edge; no handler anywhere consumes a repo filter for the spec graph, so \"specs for one repo with all outbound/inbound links\" cannot be queried. (2) Four of the endpoints — GET /api/v1/specs/stale-links, GET /api/v1/specs/conflicts, GET /api/v1/specs/{path}/dependents, GET /api/v1/specs/{path}/dependencies — are listed in the frozen ABAC route-registry exemption file and their handlers (get_stale_links, get_conflicts, get_spec_dependents, get_spec_dependencies) perform no per-handler authorization at all: the signatures take only State/Path with no auth extractor and no scope comparison, so any authenticated principal in any tenant receives the forge-wide links store (cross-tenant spec-graph data). This violates the repo invariant that every registry-exempt route MUST enforce per-handler authorization; check-abac-exempt-handlers.sh passes only because its Rule 1 skips handlers that name no auth extractor (\"no auth identity at all is a different check's job\"). By contrast /api/v1/specs/graph and /api/v1/specs/:path/links are properly resolver-mapped in abac_middleware.rs and receive middleware policy evaluation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/specs/graph\", get(specs::get_spec_graph))",
            "detail": "Only one spec-graph route is registered; there is no repo-scoped variant."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn get_spec_graph(State(state): State<Arc<AppState>>) -> Json<SpecGraphResponse> {",
            "detail": "The handler signature accepts only State -- no Query extractor, no repo parameter. The ?repo={id} filter of the spec's endpoint table is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        .iter()\n        .filter(|l| l.source_path == spec_path || l.target_path == spec_path)",
            "detail": "By contrast the per-spec links endpoint does return both directions (source_path or target_path match), confirming the six implemented endpoints query real state."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/specs/:path/dependencies\n/api/v1/specs/:path/dependents",
            "detail": "The dependents and dependencies endpoints are in the frozen ABAC exemption file, so they run without middleware policy evaluation; per the repo invariant an exempt handler must then enforce per-handler authorization, which these handlers do not."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/specs/conflicts\n/api/v1/specs/stale-links",
            "detail": "The conflicts and stale-links endpoints are likewise registry-exempt with no per-handler authorization, so any authenticated principal in any tenant receives the forge-wide (cross-tenant) links store."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn get_stale_links(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {",
            "detail": "Handler signature: only State, no auth extractor and no scope comparison — the response is the unfiltered links store. get_conflicts, get_spec_dependents and get_spec_dependencies have the same shape (State/Path only)."
          },
          {
            "file": "scripts/check-abac-exempt-handlers.sh",
            "quote": "continue  # no auth identity at all is a different check's job",
            "detail": "The mechanical check passes despite the missing authorization because Rule 1 skips any exempt handler whose signature names no auth parameter — handlers with no auth identity at all fall outside its scope, so the four query endpoints are never flagged."
          },
          {
            "file": "crates/gyre-server/src/abac_middleware.rs",
            "quote": "RouteResourceMapping::api(\"/api/v1/specs/graph\", \"spec\", None),",
            "detail": "By contrast /api/v1/specs/graph (and /api/v1/specs/:path/links) are resolver-mapped, proving the four exempt query endpoints could be brought under the same middleware policy evaluation."
          }
        ]
      },
      {
        "id": "d357602cf54bb50ce2880f6d",
        "decision": "satisfied",
        "requirement_quote": "gyre spec links system/identity-security.md    # Show all links for a spec\ngyre spec dependents system/source-control.md  # Who depends on this spec?",
        "detail": "All five specced commands are implemented against the real API: `gyre spec links <path>` (get_spec_links), `gyre spec dependents <path>`, `gyre spec graph` with text output by default and `--format dot` emitting Graphviz DOT, `gyre spec stale-links`, and `gyre spec conflicts`. Unsupported formats are rejected rather than silently misrendered.",
        "evidence": [
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "                SpecCommands::Graph { format } => {\n                    let graph = api.get_spec_graph().await?;\n                    let fmt = format.as_deref().unwrap_or(\"text\");\n                    match fmt {",
            "detail": "`gyre spec graph` fetches the real graph and renders text or DOT per --format, matching the spec's CLI block including the dot usage."
          },
          {
            "file": "crates/gyre-cli/src/main.rs",
            "quote": "                SpecCommands::StaleLinks => {\n                    let links = api.get_stale_spec_links().await?;",
            "detail": "`gyre spec stale-links` queries the stale-links endpoint and prints the table; the links/dependents/conflicts commands are implemented in the same match arm block."
          }
        ]
      },
      {
        "id": "5e761a34ffed5b87f0b401c1",
        "decision": "gap",
        "requirement_quote": "- **Spec detail page:** link section showing all outbound/inbound links with staleness indicators",
        "detail": "Two of the three bullets are real: the spec graph visualization (SpecGraphDAG renders the interactive DAG with nodes colored by approval status, stale edges highlighted yellow, conflicts red, and click navigation) and the impact analysis view (SpecDashboard impact mode computes all transitive dependents via BFS over the real graph edges and aggregates them per repo). The spec-detail link section is partially broken: the diagram's inbound/outbound columns and the list's direction labels are computed from a `direction` field on each link object, but the links API response (SpecLinkResponse) has no such field, so `link.direction` is always undefined -- every link, inbound included, is classified outbound, renders in the outbound column, and is labeled '-> to'. The outbound/inbound distinction the bullet requires is therefore not displayed. Staleness is surfaced via the link status badge (status string incl. \"stale\").",
        "evidence": [
          {
            "file": "web/src/lib/DetailPanel.svelte",
            "quote": "            {@const inbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) === 'inbound')}\n            {@const outbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) !== 'inbound')}",
            "detail": "The spec-detail links tab splits inbound/outbound and labels each row by `link.direction` -- a field the API never emits, so inbound links are silently rendered as outbound."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub struct SpecLinkResponse {\n    pub id: String,\n    pub source_path: String,",
            "detail": "SpecLinkResponse (the serialized shape of GET /specs/{path}/links) has no direction field: id, source_path, source_repo_id, link_type, target_path, ... only."
          },
          {
            "file": "web/src/components/SpecGraphDAG.svelte",
            "quote": "    const isStale = edge.status === 'stale';",
            "detail": "DAG visualization bullet is satisfied: stale edges are highlighted (yellow stroke) in the interactive graph."
          },
          {
            "file": "web/src/components/SpecDashboard.svelte",
            "quote": "    const queue = [{ path: impactPath, depth: 0 }];",
            "detail": "Impact analysis bullet is satisfied: BFS over the spec graph computes all transitive dependents of the selected spec."
          }
        ]
      },
      {
        "id": "ac4a8552cbca5b1333030655",
        "decision": "gap",
        "requirement_quote": "The Accountability agent's patrol gains spec-graph awareness:",
        "detail": "All five checks are implemented as real logic over the links store and spec ledger (stale links beyond a threshold, orphaned supersessions -- links targeting deprecated specs, unresolved conflicts -- both specs of a conflicts_with link approved, dangling implementations -- implements target absent from the ledger, and deep depends_on chains via memoized longest-path depth > 5). The gap is the integration the heading requires: the patrol exists only as a manually invoked HTTP endpoint. It is not part of any scheduled job (jobs.rs registers the staleness job and many others, but no spec patrol), the accountability persona's documented patrol inputs list no spec-graph items (specs/personas/accountability.md \"What You Scan\" covers specs, code, MRs, AGENTS.md, milestones only), and the endpoint is listed in the frozen ABAC route-registry exemption file while its handler performs no per-handler authorization at all -- patrol_spec_links declares no auth extractor, so any authenticated principal in any tenant can trigger the forge-wide patrol and receive cross-tenant findings plus notification fan-out (check-abac-exempt-handlers.sh passes only because its Rule 1 matches handlers that name an auth extractor; this handler names none).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_patrol.rs",
            "quote": "\n    // 1. Stale links beyond threshold\n    check_stale_links(&links, now_secs, stale_threshold_secs, &mut findings);\n\n    // 2. Orphaned supersessions\n    check_orphaned_supersessions(&links, &ledger_map, &mut findings);\n\n    // 3. Unresolved conflicts\n    check_unresolved_conflicts(&links, &ledger_map, &mut findings);\n\n    // 4. Dangling implementations\n    check_dangling_implementations(&links, &ledger_map, &mut findings);",
            "detail": "The five specced patrol checks are genuinely implemented over the real links store and ledger snapshot."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn patrol_spec_links(\n    State(state): State<Arc<AppState>>,\n    Json(req): Json<crate::spec_patrol::PatrolRequest>,\n) -> Json<crate::spec_patrol::PatrolResponse> {",
            "detail": "The only delivery surface is this handler: no auth extractor, no scope comparison -- combined with the ABAC exemption this means no authorization decision at all on the patrol endpoint."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/patrol/spec-links",
            "detail": "/api/v1/patrol/spec-links is in the frozen ABAC exemption file, so it runs without middleware policy evaluation; per the repo invariant an exempt handler must enforce per-handler authorization, which the handler above does not."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "            |state| async move { crate::spec_link_staleness::run_once(&state).await },",
            "detail": "The job scheduler registers the staleness checker but no spec patrol job -- the patrol never runs on the accountability agent's cycle unless manually invoked."
          },
          {
            "file": "specs/personas/accountability.md",
            "quote": "On each patrol cycle, read and cross-reference:",
            "detail": "The accountability persona's patrol inputs enumerate specs, code, MRs, AGENTS.md and milestones -- no spec-graph items, so the agent's patrol prompt has no awareness of the spec-link checks."
          }
        ]
      },
      {
        "id": "9810f4a19cbf548c75517e3f",
        "decision": "context",
        "requirement_quote": "- **Spec Registry** (`spec-registry.md`): links are declared in the manifest and tracked in the forge ledger. The approval status computation now considers link constraints.",
        "detail": "Cross-reference section mapping this spec onto its neighbors; each claim's normative force lives in the section audited here: 'approval status computation considers link constraints' = Approval Gates (gap, dcd440115bac4622a351c857), 'spec changes cascade through the link graph, creating drift-review tasks' = Automatic Staleness Detection (satisfied), 'cross-workspace links are resolved through the tenant-wide graph' = Cross-Repo and Cross-Workspace Links (gap, ff74a147e0ca35a407836447), 'MRs referencing superseded specs are rejected' = Merge Gates Check 1 (satisfied within that section). The 'Workspace Orchestrator uses the graph for impact analysis' and agent-gates pointers describe other documents' surfaces; the impact-analysis capability itself is delivered and dispositioned under UI. No independent requirement to audit beyond those sections.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "675c0dc7be4a127cb63039f3",
        "decision": "gap",
        "requirement_quote": "Every link is SHA-pinned. Not \"this spec relates to identity-security.md\" but \"this spec relates to identity-security.md@abc123.\" When the linked spec changes, the link becomes stale and the forge flags it.",
        "detail": "The six typed link variants and most per-type enforcement are real (implements/conflicts_with checked at approval, supersedes deprecation on approval, extends drift handling, references deliberately unenforced). But the section's foundational contract -- every link SHA-pinned so a changed target forces the link stale -- is not enforced: the manifest link schema parses target_sha as optional with no validation anywhere in the push/sync path, an unpinned link is stored with status \"active\", and both staleness paths (push-time outbound check and the daily staleness job) skip links with no pinned target_sha, so such a link can never be flagged no matter how the target changes. The table's depends_on row (\"Implementation tasks blocked until dependency spec's implementation is complete\") is also not implemented as stated -- the forge blocks approval of the source spec instead of blocking implementation tasks and models completeness as approval status; that deviation is fully accounted under Approval Gates (dcd440115bac4622a351c857) to avoid double counting.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    /// SHA the link was pinned to. Stale if target spec SHA advances.\n    pub target_sha: Option<String>,",
            "detail": "Manifest link schema: target_sha is Option<String>. No code path validates that a declared link carries a SHA, so 'every link is SHA-pinned' is unenforced and unpinned links are accepted into the graph."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "        // Only check links that have a pinned target_sha and are currently active.\n        let Some(pinned_sha) = &link.target_sha else {\n            continue;\n        };",
            "detail": "Daily staleness job: links without a pinned target_sha are skipped with `continue`, so they can never be marked stale -- the 'when the linked spec changes, the link becomes stale and the forge flags it' guarantee does not hold for them."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "            // Staleness detection: if the link has a pinned target_sha but the target\n            // spec's current SHA in the ledger differs, mark the link as stale.\n            if let Some(pinned_sha) = &link.target_sha {",
            "detail": "Push-time outbound staleness check likewise only runs `if let Some(pinned_sha) = &link.target_sha`; unpinned links stay \"active\" forever."
          }
        ]
      },
      {
        "id": "ff74a147e0ca35a407836447",
        "decision": "gap",
        "requirement_quote": "The forge resolves cross-repo links by querying the tenant-wide spec graph. The link is validated:\n- Target workspace exists and the linking user has read access\n- Target repo exists within that workspace\n- Target spec path exists in the target repo's manifest\n- Target SHA matches (or is flagged as stale if the spec has moved on)",
        "detail": "Target formats (@{workspace_slug}/{repo_name}/{spec_path}, @{repo_name}/{spec_path}, bare path) parse correctly, cross-workspace workspace+repo existence is resolved tenant-scoped, and target-SHA drift is flagged stale against the ledger. Four validation requirements are not delivered: (1) same-workspace cross-repo links (@{repo_name}/{spec_path}, an explicitly specced format) never resolve -- sync_spec_ledger passes None as the workspace to resolve_repo_by_name, which returns None whenever workspace_id is None, so every such link is stored \"unresolved\" with target_repo_id NULL even when the repo exists, and the staleness job's re-resolution explicitly leaves CrossRepo links unresolved; (2) the linking user's read access is never validated -- link resolution runs post-push with no caller identity in scope; (3) target spec path existence in the target repo's manifest is never validated -- a link whose repo resolves is marked \"active\" without checking the target repo's manifest contains the path; (4) the staleness checker's cross-workspace re-resolution queries under a hardcoded fabricated tenant id \"default\", so in any other tenant the workspace slug never resolves (also violating the repo's fabricated-scope-defaults invariant).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                        let resolved_repo_id = resolve_repo_by_name(\n                            repos, workspaces, tenant_id,\n                            None, // same workspace — we'll look up by name across all\n                            repo_name,\n                        )\n                        .await;",
            "detail": "Same-workspace cross-repo resolution at push time passes None as workspace_id; the comment claims 'look up by name across all' but the callee cannot."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    let ws_id = workspace_id?;\n    repos",
            "detail": "resolve_repo_by_name immediately returns None when workspace_id is None (`let ws_id = workspace_id?;`), so every @{repo_name}/path link is permanently unresolved despite the repo existing -- 'Target repo exists within that workspace' is never positively validated for this format."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "            crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {\n                // For cross-repo same-workspace links, we'd need the source workspace.\n                // Without it, we can't resolve. Leave as unresolved.",
            "detail": "The staleness checker (which the spec says resolves NULL target_repo_id links later) explicitly refuses to re-resolve same-workspace cross-repo links, leaving them unresolved forever."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "                let ws = state\n                    .workspaces\n                    .find_by_slug(&Id::new(\"default\"), workspace_slug)",
            "detail": "Cross-workspace re-resolution fabricates the tenant scope as the literal id \"default\" instead of the source link's tenant, so workspaces in any other tenant never resolve."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                        let status = if resolved_repo_id.is_some() {\n                            \"active\".to_string()\n                        } else {\n                            \"unresolved\".to_string()",
            "detail": "A cross-repo link whose repo resolves is marked \"active\" with no check that the target spec path exists in the target repo's manifest -- only repo existence is consulted, never the target manifest; the linking user's read access appears nowhere in this path."
          }
        ]
      },
      {
        "id": "b222f4b88e85892d669131d7",
        "decision": "gap",
        "requirement_quote": "| `broken` | Target spec was deleted or is no longer in the manifest |\n| `conflicted` | A `conflicts_with` link where both specs are approved (violation) |",
        "detail": "active (default) and stale (push-time outbound/inbound detection plus the daily job) are delivered. Two of the four statuses are not delivered as stored link states: (1) broken is only set when the target is absent from the spec ledger entirely -- but a spec removed from the manifest is kept in the ledger as Deprecated (never deleted), so links targeting it remain status \"active\" forever; the \"no longer in the manifest\" half of the broken definition never fires, and the daily job's Ok(Some(entry)) path leaves the link untouched. (2) conflicted is never assigned as a status anywhere in the codebase -- both-approved conflicts are only computed on the fly by GET /api/v1/specs/conflicts and the patrol check; the status column (documented in the domain type as one of the five health values) can never hold \"conflicted\", so the stored graph cannot be relied on for either condition. Additionally the stored statuses that do exist are a one-way ratchet: the daily staleness pass skips links already marked broken (\"Already known stale/broken — skip\"), so a target spec re-added to its manifest or restored to the ledger leaves inbound links broken forever even though the defining condition (\"Target spec was deleted or is no longer in the manifest\") has ceased; the stored graph the spec promises must reflect current conditions, and GET /api/v1/specs/conflicts is required to return \"All active conflicts\" — which a never-reverting conflicted status would misreport as active forever once one exists.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "            Ok(None) => {\n                // Target spec not in ledger — mark as broken.\n                mark_link_status(state, &link.id, \"broken\", now_secs).await;",
            "detail": "The only code path that sets \"broken\": target lookup returns Ok(None) (not in the ledger at all). Since manifest removal keeps the ledger entry, this path does not cover 'no longer in the manifest'."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                e.approval_status = ApprovalStatus::Deprecated;\n                e.updated_at = now;\n                let _ = ledger.save(&e).await;\n            }",
            "detail": "A spec removed from the manifest is marked Deprecated and kept in the ledger (save, not delete) -- so links targeting it find Ok(Some(entry)) in the staleness job, the SHA is unchanged, and the link stays \"active\" instead of \"broken\"."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    // An \"active conflict\" is a conflicts_with link where both specs are approved.\n    // spec-links.md §Link Status: conflicted = both specs approved (violation).",
            "detail": "Conflicted links are computed on the fly by the conflicts endpoint rather than ever being stored: no code assigns the \"conflicted\" status to a link row."
          },
          {
            "file": "crates/gyre-domain/src/spec_links.rs",
            "quote": "    /// Link health: \"active\" | \"stale\" | \"broken\" | \"conflicted\" | \"unresolved\"",
            "detail": "The domain type documents \"conflicted\" as a link health value, but a repository-wide search finds no producer: the value is dead in the stored graph."
          },
          {
            "file": "crates/gyre-server/src/spec_link_staleness.rs",
            "quote": "        if link.status == \"stale\" || link.status == \"broken\" {\n            // Already known stale/broken — skip.\n            continue;\n        }",
            "detail": "The staleness pass skips links already marked broken, so a target that returns to the ledger or its repo's manifest never transitions the link back — \"broken\" is permanent once set, a one-way ratchet rather than a reflection of current conditions."
          }
        ]
      },
      {
        "id": "dcd440115bac4622a351c857",
        "decision": "gap",
        "requirement_quote": "| `implements` | Source spec cannot be approved until target (parent) spec is approved at the pinned SHA |\n| `depends_on` | Implementation tasks for source spec are blocked until target spec's implementation tasks are complete |\n| `conflicts_with` | Both specs cannot have `approval_status: approved` simultaneously. The forge rejects the second approval and creates a conflict-resolution task. |",
        "detail": "Two rows are really enforced: supersedes (approving the source automatically marks every superseded target Deprecated in the ledger, with a deprecation event) and extends (a changed target marks the extending spec drifted and resets its approval to Pending, in both the outbound and inbound sync paths). Three rows deviate from the contract: (1) depends_on -- the gate blocks APPROVAL of the source spec, not its implementation tasks, and judges \"implementation complete\" by the target's ledger approval_status; per spec-lifecycle.md approval precedes decomposition, so an approved target with open/incomplete implementation tasks passes the gate -- the specced signal (target spec's implementation tasks complete) is never consulted (no Task query anywhere in the gate). (2) implements -- the gate checks only the parent's current approval_status and never compares the approval to the link's pinned target_sha, so a source can be approved against a parent version it does not pin (\"approved at the pinned SHA\" is not verified). (3) conflicts_with -- the second approval is rejected with the specced message, but no conflict-resolution task is created anywhere in the rejection path (the handler returns InvalidInput immediately; a repository-wide search for conflict-resolution task creation finds none).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                SpecLinkType::DependsOn => {\n                    // TASK-016: Target spec's implementation must be complete (approved)\n                    // before the depending spec can be approved.\n                    // spec-links.md §Approval Gates: \"Implementation tasks for source spec\n                    // are blocked until target spec's implementation tasks are complete.\"\n                    if let Some(target) = state.spec_ledger.find_by_path(&link.target_path).await? {",
            "detail": "depends_on gate: blocks approval of the source spec and models 'implementation complete' as the target's approval_status -- the specced mechanism (blocking implementation tasks until the target's implementation tasks are complete) is absent."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                SpecLinkType::Implements => {\n                    // Parent spec must be approved before this spec can be approved.\n                    if let Some(parent) = state.spec_ledger.find_by_path(&link.target_path).await? {\n                        if parent.approval_status != ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': implements '{}' which is not yet approved\",\n                                spec_path, link.target_path",
            "detail": "implements gate: checks only parent.approval_status != Approved; the link's pinned target_sha is never compared, so 'approved at the pinned SHA' is not enforced."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                    {\n                        if conflicting.approval_status == ApprovalStatus::Approved {\n                            return Err(ApiError::InvalidInput(format!(\n                                \"cannot approve '{}': conflicts with '{}' which is already approved — resolve the conflict first\",\n                                spec_path, link.target_path",
            "detail": "conflicts_with gate: rejects the second approval but immediately returns the error -- no conflict-resolution task is created, contrary to 'The forge rejects the second approval and creates a conflict-resolution task.'"
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                if target_entry.approval_status != ApprovalStatus::Deprecated {\n                    target_entry.approval_status = ApprovalStatus::Deprecated;\n                    target_entry.updated_at = now;\n                    let _ = state.spec_ledger.save(&target_entry).await;",
            "detail": "supersedes row is satisfied: approving the superseding spec marks the target Deprecated in the ledger (plus a StaleSpecWarning deprecation event)."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                            source_entry.drift_status = \"drifted\".to_string();\n                            source_entry.approval_status = ApprovalStatus::Pending;\n                            source_entry.updated_at = now;",
            "detail": "extends row is satisfied: when the target changes, the extending spec is marked drifted and its approval is reset to Pending (both in the outbound sync path shown and the inbound path)."
          }
        ]
      },
      {
        "id": "cb0d56cefcb50d54be0121c4",
        "decision": "gap",
        "requirement_quote": "- If the spec `depends_on` another spec that isn't implemented yet, the MR gets a warning (not a block - the code might be valid, but the dependency isn't satisfied yet)",
        "detail": "Two of the three bullets are real: the superseded-spec rejection (Check 1 rejects the merge with exactly the specced message \"spec '{path}' has been superseded by '{new_path}'. Update your spec_ref.\") and the conflicts_with block (Check 2 blocks, bidirectionally, when the conflicting spec is approved). The depends_on-warning bullet is a gap: 'isn't implemented yet' is judged by the target spec's ledger approval status, not by implementation-task completeness. The spec's own vocabulary denotes task completeness (Approval Gates: \"until target spec's implementation tasks are complete\"), and per spec-lifecycle.md approval precedes decomposition, so approval never implied implementation -- the warning stays silent exactly in the common case the bullet exists for (target spec approved, implementation tasks incomplete). This corrects the prior cycle's 'satisfied' disposition per the controller review feedback; the warning event itself (StaleSpecWarning, warn-not-block) is correctly shaped. Per controller review feedback this includes the zero-task case: a depends_on target that has never been decomposed (zero implementation tasks — the normal state per spec-lifecycle.md, where approval precedes decomposition) is not implemented and its dependency is not satisfied, so the warning must fire there too; the approval-status predicate stays silent for every Approved target regardless of implementation-task state, including that most common case.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                if let Ok(Some(target)) = state.spec_ledger.find_by_path(&link.target_path).await {\n                    if target.approval_status != crate::spec_registry::ApprovalStatus::Approved {\n                        warn!(",
            "detail": "Check 3 predicate: the depends_on warning fires only when the target's approval_status != Approved. A target that is Approved with open implementation tasks produces no warning, so 'isn't implemented yet' is approximated by approval rather than judged by implementation completeness."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "            if link.link_type == crate::spec_registry::SpecLinkType::Supersedes\n                && link.target_path == spec_path\n            {\n                let reason = format!(\n                    \"spec '{}' has been superseded by '{}'. Update your spec_ref.\",\n                    spec_path, link.source_path\n                );",
            "detail": "Superseded-spec bullet is satisfied: the merge is rejected with the specced message whenever the referenced spec has an inbound supersedes link."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                        let reason = format!(\n                            \"spec '{}' conflicts with approved spec '{}' — resolve the conflict first\",\n                            spec_path, other_spec_path",
            "detail": "Conflicts_with bullet is satisfied: the merge is blocked when the referenced spec conflicts with an approved spec (checked in both directions)."
          }
        ]
      },
      {
        "id": "8772dba48e530c04a3d2f953",
        "decision": "gap",
        "requirement_quote": "The forge rejects manifest changes that would create cycles in the spec graph:\n- A `depends_on` B `depends_on` C `depends_on` A -> rejected",
        "detail": "Push-time rejection is real and the excluded types match the spec (conflicts_with bidirectional, references and supersedes excluded; depends_on/implements/extends participate), with default-branch pushes undone and a PushRejected event plus audit on rejection. The gap is detection scope: detect_link_cycles builds its adjacency list exclusively from the single manifest being pushed -- the forge's tenant-wide spec graph (the durable spec_links table, other repos' manifests, or even previously synced links of the same repo) is never consulted. A cycle whose edges are declared in two different manifests or repos -- the enterprise case this spec's Problem section exists for -- is therefore accepted: each push only sees its own edges, so A→B in repo 1 and B→A in repo 2 both land cleanly and the cycle lives permanently in the forge-maintained graph.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    let mut adj: HashMap<String, Vec<String>> = HashMap::new();\n    for entry in &manifest.specs {\n        for link in &entry.links {\n            match link.link_type {",
            "detail": "Cycle detection adjacency is built only from `manifest.specs` of the one manifest being pushed; links already in the forge graph (other repos/manifests, the durable spec_links table) are never consulted, so cross-manifest cycles are not detected."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        for update in ref_updates.iter().filter(|u| u.refname == default_ref) {\n            if let Err(rejection) =\n                crate::spec_registry::check_spec_link_cycles(&repo_path, &update.new_sha).await\n            {",
            "detail": "The rejection wiring itself is real: a failing cycle check on a default-branch push undoes the ref updates (rejection side confirmed; only the detection scope is deficient)."
          }
        ]
      },
      {
        "id": "58102d3a27f920beeb3fba71",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/specs/graph?repo={id}` | GET | Specs for one repo with all outbound/inbound links |",
        "detail": "Six of the seven specced endpoints exist and query the real links store and spec ledger: GET /api/v1/specs/graph (full graph), /specs/{path}/links (both outbound and inbound), /specs/{path}/dependents and /dependencies (impact analysis over depends_on/implements), /specs/stale-links, and /specs/conflicts (both-approved conflicts_with links). Two defects keep the section open. (1) The repo-filtered graph variant is not implemented: get_spec_graph takes no query parameter and always returns every ledger node and every store edge; no handler anywhere consumes a repo filter for the spec graph, so \"specs for one repo with all outbound/inbound links\" cannot be queried. (2) Four of the endpoints — GET /api/v1/specs/stale-links, GET /api/v1/specs/conflicts, GET /api/v1/specs/{path}/dependents, GET /api/v1/specs/{path}/dependencies — are listed in the frozen ABAC route-registry exemption file and their handlers (get_stale_links, get_conflicts, get_spec_dependents, get_spec_dependencies) perform no per-handler authorization at all: the signatures take only State/Path with no auth extractor and no scope comparison, so any authenticated principal in any tenant receives the forge-wide links store (cross-tenant spec-graph data). This violates the repo invariant that every registry-exempt route MUST enforce per-handler authorization; check-abac-exempt-handlers.sh passes only because its Rule 1 skips handlers that name no auth extractor (\"no auth identity at all is a different check's job\"). By contrast /api/v1/specs/graph and /api/v1/specs/:path/links are properly resolver-mapped in abac_middleware.rs and receive middleware policy evaluation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/specs/graph\", get(specs::get_spec_graph))",
            "detail": "Only one spec-graph route is registered; there is no repo-scoped variant."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn get_spec_graph(State(state): State<Arc<AppState>>) -> Json<SpecGraphResponse> {",
            "detail": "The handler signature accepts only State -- no Query extractor, no repo parameter. The ?repo={id} filter of the spec's endpoint table is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        .iter()\n        .filter(|l| l.source_path == spec_path || l.target_path == spec_path)",
            "detail": "By contrast the per-spec links endpoint does return both directions (source_path or target_path match), confirming the six implemented endpoints query real state."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/specs/:path/dependencies\n/api/v1/specs/:path/dependents",
            "detail": "The dependents and dependencies endpoints are in the frozen ABAC exemption file, so they run without middleware policy evaluation; per the repo invariant an exempt handler must then enforce per-handler authorization, which these handlers do not."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/specs/conflicts\n/api/v1/specs/stale-links",
            "detail": "The conflicts and stale-links endpoints are likewise registry-exempt with no per-handler authorization, so any authenticated principal in any tenant receives the forge-wide (cross-tenant) links store."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn get_stale_links(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {",
            "detail": "Handler signature: only State, no auth extractor and no scope comparison — the response is the unfiltered links store. get_conflicts, get_spec_dependents and get_spec_dependencies have the same shape (State/Path only)."
          },
          {
            "file": "scripts/check-abac-exempt-handlers.sh",
            "quote": "continue  # no auth identity at all is a different check's job",
            "detail": "The mechanical check passes despite the missing authorization because Rule 1 skips any exempt handler whose signature names no auth parameter — handlers with no auth identity at all fall outside its scope, so the four query endpoints are never flagged."
          },
          {
            "file": "crates/gyre-server/src/abac_middleware.rs",
            "quote": "RouteResourceMapping::api(\"/api/v1/specs/graph\", \"spec\", None),",
            "detail": "By contrast /api/v1/specs/graph (and /api/v1/specs/:path/links) are resolver-mapped, proving the four exempt query endpoints could be brought under the same middleware policy evaluation."
          }
        ]
      },
      {
        "id": "5e761a34ffed5b87f0b401c1",
        "decision": "gap",
        "requirement_quote": "- **Spec detail page:** link section showing all outbound/inbound links with staleness indicators",
        "detail": "Two of the three bullets are real: the spec graph visualization (SpecGraphDAG renders the interactive DAG with nodes colored by approval status, stale edges highlighted yellow, conflicts red, and click navigation) and the impact analysis view (SpecDashboard impact mode computes all transitive dependents via BFS over the real graph edges and aggregates them per repo). The spec-detail link section is partially broken: the diagram's inbound/outbound columns and the list's direction labels are computed from a `direction` field on each link object, but the links API response (SpecLinkResponse) has no such field, so `link.direction` is always undefined -- every link, inbound included, is classified outbound, renders in the outbound column, and is labeled '-> to'. The outbound/inbound distinction the bullet requires is therefore not displayed. Staleness is surfaced via the link status badge (status string incl. \"stale\").",
        "evidence": [
          {
            "file": "web/src/lib/DetailPanel.svelte",
            "quote": "            {@const inbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) === 'inbound')}\n            {@const outbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) !== 'inbound')}",
            "detail": "The spec-detail links tab splits inbound/outbound and labels each row by `link.direction` -- a field the API never emits, so inbound links are silently rendered as outbound."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub struct SpecLinkResponse {\n    pub id: String,\n    pub source_path: String,",
            "detail": "SpecLinkResponse (the serialized shape of GET /specs/{path}/links) has no direction field: id, source_path, source_repo_id, link_type, target_path, ... only."
          },
          {
            "file": "web/src/components/SpecGraphDAG.svelte",
            "quote": "    const isStale = edge.status === 'stale';",
            "detail": "DAG visualization bullet is satisfied: stale edges are highlighted (yellow stroke) in the interactive graph."
          },
          {
            "file": "web/src/components/SpecDashboard.svelte",
            "quote": "    const queue = [{ path: impactPath, depth: 0 }];",
            "detail": "Impact analysis bullet is satisfied: BFS over the spec graph computes all transitive dependents of the selected spec."
          }
        ]
      },
      {
        "id": "ac4a8552cbca5b1333030655",
        "decision": "gap",
        "requirement_quote": "The Accountability agent's patrol gains spec-graph awareness:",
        "detail": "All five checks are implemented as real logic over the links store and spec ledger (stale links beyond a threshold, orphaned supersessions -- links targeting deprecated specs, unresolved conflicts -- both specs of a conflicts_with link approved, dangling implementations -- implements target absent from the ledger, and deep depends_on chains via memoized longest-path depth > 5). The gap is the integration the heading requires: the patrol exists only as a manually invoked HTTP endpoint. It is not part of any scheduled job (jobs.rs registers the staleness job and many others, but no spec patrol), the accountability persona's documented patrol inputs list no spec-graph items (specs/personas/accountability.md \"What You Scan\" covers specs, code, MRs, AGENTS.md, milestones only), and the endpoint is listed in the frozen ABAC route-registry exemption file while its handler performs no per-handler authorization at all -- patrol_spec_links declares no auth extractor, so any authenticated principal in any tenant can trigger the forge-wide patrol and receive cross-tenant findings plus notification fan-out (check-abac-exempt-handlers.sh passes only because its Rule 1 matches handlers that name an auth extractor; this handler names none).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_patrol.rs",
            "quote": "\n    // 1. Stale links beyond threshold\n    check_stale_links(&links, now_secs, stale_threshold_secs, &mut findings);\n\n    // 2. Orphaned supersessions\n    check_orphaned_supersessions(&links, &ledger_map, &mut findings);\n\n    // 3. Unresolved conflicts\n    check_unresolved_conflicts(&links, &ledger_map, &mut findings);\n\n    // 4. Dangling implementations\n    check_dangling_implementations(&links, &ledger_map, &mut findings);",
            "detail": "The five specced patrol checks are genuinely implemented over the real links store and ledger snapshot."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub async fn patrol_spec_links(\n    State(state): State<Arc<AppState>>,\n    Json(req): Json<crate::spec_patrol::PatrolRequest>,\n) -> Json<crate::spec_patrol::PatrolResponse> {",
            "detail": "The only delivery surface is this handler: no auth extractor, no scope comparison -- combined with the ABAC exemption this means no authorization decision at all on the patrol endpoint."
          },
          {
            "file": "scripts/abac-route-registry-exemptions.txt",
            "quote": "/api/v1/patrol/spec-links",
            "detail": "/api/v1/patrol/spec-links is in the frozen ABAC exemption file, so it runs without middleware policy evaluation; per the repo invariant an exempt handler must enforce per-handler authorization, which the handler above does not."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "            |state| async move { crate::spec_link_staleness::run_once(&state).await },",
            "detail": "The job scheduler registers the staleness checker but no spec patrol job -- the patrol never runs on the accountability agent's cycle unless manually invoked."
          },
          {
            "file": "specs/personas/accountability.md",
            "quote": "On each patrol cycle, read and cross-reference:",
            "detail": "The accountability persona's patrol inputs enumerate specs, code, MRs, AGENTS.md and milestones -- no spec-graph items, so the agent's patrol prompt has no awareness of the spec-link checks."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "sha256": "187a311bccfff72faea7b86d5225210383ab7031f1c87762fdb1d677c60b5cec"
      },
      {
        "file": "crates/gyre-server/src/spec_link_staleness.rs",
        "sha256": "e1e21cbb7595208f7f76ce391e432f0718124c2e3bc7073bda8d628e50d8e0c6"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-11-08-000061_spec_links/up.sql",
        "sha256": "f6ca9876bea9faf4d9682d09ebe424d274564a6251e67467a2ffc58bf4f75688"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "a23a71630b3302002b50d4dbc1f256cf95c2deeed11f69df4a02b083410f2771"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "b6391c5208b1cfe511c0d41b69f7e14e08472d2b597655e299dfcb7e0eac8ca0"
      },
      {
        "file": "crates/gyre-domain/src/spec_links.rs",
        "sha256": "2437865ac02432b281bcb7d141d2cc55e93876a7c0e96f72c80cadf621a41a75"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "42ad3821e16bc5e58a55a8edc2dfd8714b901fe397466b79b78a4213980dbbce"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "a03dd115a9635ca3ae50d45c69931926effafa7ddec1cfa8f317f601a53ec27c"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "036b32975f4110d6aa35cef408441c0be5ecb4cef4a9a7504eee9caf08a8bc35"
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "sha256": "0789c85cf261c0f0c39c9e22bd2711581436792e6686d2ddb0187c9f10d07b06"
      },
      {
        "file": "scripts/check-abac-exempt-handlers.sh",
        "sha256": "d694ac70440117a85b65e2682008edb8170790b5f5f75ddbf76651b69ae8ae86"
      },
      {
        "file": "crates/gyre-server/src/abac_middleware.rs",
        "sha256": "e58c887b8230d43570784f4336c0942a22f7681cf3bba92cafd04d711ffead90"
      },
      {
        "file": "crates/gyre-cli/src/main.rs",
        "sha256": "b16446e777779fcd02857cd61e536961e6c3283b2443339bd35935e9f9ec5c4d"
      },
      {
        "file": "web/src/lib/DetailPanel.svelte",
        "sha256": "373345bbbfc4286bedaa0b7fcf5e979d231b87b950ec53e648c5666cc59de923"
      },
      {
        "file": "web/src/components/SpecGraphDAG.svelte",
        "sha256": "183cb39d3d35d40a8048ef9250f9c3c89a8449b75944be0763dfa8d10adfff3a"
      },
      {
        "file": "web/src/components/SpecDashboard.svelte",
        "sha256": "0cc9f2c8ae3b96d7728b5f8c3cf35c7341f799a65c7830c7860883e65b6e31c2"
      },
      {
        "file": "crates/gyre-server/src/spec_patrol.rs",
        "sha256": "e79938cfd8706c1fc16a7e4619f9855c8196d61ccd9ea58d0cce875909919592"
      },
      {
        "file": "specs/personas/accountability.md",
        "sha256": "efa88c940c71fffa88dfdf293f82afe8a5f4a0be67bcd9dbb3dd3c908a518d48"
      }
    ]
  },
  "plan": {
    "audit": "36545b6c5836807360743e3a315efdda46941c3b9d51c4042fdca8c01d3b113e",
    "tasks": [
      {
        "key": "spec-links-sha-pinning",
        "title": "Spec Links — enforce SHA pinning: no link enters or stays in the graph without a target_sha",
        "covers": [
          "675c0dc7be4a127cb63039f3"
        ],
        "required_behavior": "spec-links.md §Link Types: \"Every link is SHA-pinned ... When the linked spec changes, the link becomes stale and the forge flags it.\" Today SpecLink.target_sha is Option<String> with no validation anywhere in the push/sync path, an unpinned link is stored with status \"active\", and both staleness paths (the push-time outbound check in sync_spec_ledger and the daily job in spec_link_staleness.rs, which does `let Some(pinned_sha) = &link.target_sha else { continue; }`) skip unpinned links, so such a link can never be flagged no matter how the target changes. Deliver: (1) Push-time manifest validation mirroring the existing check_spec_link_cycles wiring in git_http.rs: a default-branch push whose specs/manifest.yaml declares a link without target_sha is rejected — ref update refused, PushRejected event plus audit, error naming the source spec path and the link target. (2) sync_spec_ledger must never store a declared link as \"active\" without a pin on any entry point: the non-rejecting entry points (api/repos.rs repo import/re-sync, mirror_sync.rs, where content is already present) skip the link and log a warning naming it instead of storing an active unpinned row. (3) One-time backfill so every stored link is stale-aware: existing durable rows with empty target_sha are pinned to the target spec's current ledger SHA (e.g. in the staleness pass), after which a target SHA change marks them stale through the normal paths. (4) Amend the forge's own specs/manifest.yaml, whose declared links (e.g. depends_on system/source-control.md, references system/spec-lifecycle.md) currently carry no target_sha: add real blob-SHA pins (data amendment, not a contract change) so the repo's own next default-branch push passes its own gate.",
        "acceptance": [
          "Pushing a manifest whose link omits target_sha to the default branch is rejected with an error naming the source path and link target; the ref is not updated and a PushRejected event is recorded.",
          "After the backfill, every row in the durable spec_links table has a non-empty target_sha, and advancing a target spec's SHA subsequently marks a formerly-unpinned link stale via both the push-time inbound check and the daily staleness job.",
          "All links in specs/manifest.yaml declare target_sha values equal to the target specs' actual blob SHAs at that commit, and a push of the forge's own manifest is accepted."
        ],
        "negative_probe": "Push a manifest declaring a depends_on link without target_sha: before the fix the push lands and the link is stored status \"active\" and is silently skipped by every staleness check forever; after the fix the receive-pack rejects the push (probe asserts the rejection and that GET /api/v1/specs/{path}/links contains no active unpinned link). Second probe: take a legacy stored unpinned link, advance the target spec's SHA, run the staleness job — the link must transition to \"stale\"; on current code it stays \"active\" because of the `else { continue; }` skip.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-links-cross-repo-resolution",
        "title": "Spec Links — cross-repo link validation: same-workspace format resolution, linking-user read access, target-manifest existence, tenant-scoped re-resolution",
        "covers": [
          "ff74a147e0ca35a407836447"
        ],
        "required_behavior": "spec-links.md §Cross-Repo and Cross-Workspace Links: \"The link is validated: Target workspace exists and the linking user has read access / Target repo exists within that workspace / Target spec path exists in the target repo's manifest / Target SHA matches (or is flagged as stale)\". Four requirements are not delivered: (1) same-workspace cross-repo targets (@{repo_name}/{spec_path}, an explicitly specced format) never resolve — sync_spec_ledger passes None as workspace to resolve_repo_by_name, which returns None whenever workspace_id is None, and the staleness checker's re-resolution explicitly refuses CrossRepo links; (2) the linking user's read access is never validated (resolution runs post-push with no caller identity in scope); (3) a link whose repo resolves is marked \"active\" with no check that the target repo's manifest contains the target path; (4) resolve_unresolved_cross_workspace_links queries find_by_slug under the literal fabricated tenant id \"default\" (also violating the repo's fabricated-scope-defaults invariant). Deliver: (1) resolve same-workspace cross-repo links by passing the source workspace id (already a sync_spec_ledger parameter) to resolve_repo_by_name at push-time sync, and re-resolve unresolved CrossRepo links in the staleness pass by looking up the source link's repo → workspace. (2) Thread the pushing principal's identity (available as the authenticated agent id in the git_http push handler) into sync_spec_ledger; when a cross-workspace target resolves, validate the linking principal has read access to the target workspace (workspace membership/role lookup; system principals exempt); on failure store the link \"unresolved\" with target_repo_id NULL and a warning naming the principal and target — never \"active\". (3) When the target repo resolves, read the target repo's current specs/manifest.yaml (repo storage path + the same git-show mechanism sync already uses) and require the target path to be declared; otherwise store the link \"unresolved\" with a warning naming the missing path. (4) Derive the tenant for cross-workspace re-resolution from the source link (source repo → workspace → tenant) instead of the literal \"default\"; if the scope cannot be determined, skip and log, never fabricate. All shared sync entry points (git_http, api/repos.rs, mirror_sync) get the behavior via sync_spec_ledger.",
        "acceptance": [
          "A manifest in repo A (workspace W) declaring depends_on @B/system/auth.md, where repo B exists in W, resolves to B's repo id with status \"active\", and a subsequent change to B's target spec marks A's link stale.",
          "A cross-workspace link pushed by a principal without read access to the target workspace is stored \"unresolved\" (not active), with the denial named in logs.",
          "A link whose target repo resolves but whose target path is absent from the target repo's manifest is stored \"unresolved\" with the missing path named.",
          "In a tenant whose id is not \"default\", an unresolved cross-workspace link whose target workspace exists in that tenant re-resolves on the next staleness pass (no fabricated \"default\" tenant lookup)."
        ],
        "negative_probe": "In workspace W of tenant T (T ≠ literal \"default\"), push to repo A a manifest with link @B/path.md where B exists in W: before the fix the link is stored \"unresolved\" with target_repo_id NULL forever (push-time resolution passes None as workspace; the staleness job refuses CrossRepo re-resolution); after the fix GET /api/v1/specs/{source}/links shows target_repo_id = B and status \"active\", and pushing a SHA change to B's target spec marks A's link stale — on current code that staleness never fires for this link. Second probe: declare a link to @B/nonexistent.md (path not in B's manifest) — before the fix it is stored \"active\"; after, \"unresolved\".",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-links-status-lifecycle",
        "title": "Spec Links — stored link statuses: broken when the target leaves the manifest, conflicted when both endpoints are approved, with reversion when the defining conditions cease",
        "covers": [
          "b222f4b88e85892d669131d7"
        ],
        "required_behavior": "spec-links.md §Link Status defines four statuses; active and stale are delivered, two are not, and the stored graph must reflect CURRENT conditions, not a one-way ratchet. (1) broken: the only producer (spec_link_staleness.rs Ok(None) path) fires when the target is absent from the ledger entirely, but a spec removed from the manifest is kept in the ledger as Deprecated (save, not delete), so links targeting it stay \"active\" forever — the \"Target spec was deleted or is no longer in the manifest\" half never fires. (2) conflicted: no code assigns the status anywhere; both-approved conflicts are only computed on the fly by GET /api/v1/specs/conflicts and the patrol, so the stored graph can never be relied on for either condition. (3) Reversion (per controller review feedback b222f4b88e85892d669131d7:status-transitions-never-revert): the acceptance criteria as previously drafted could be satisfied by an implementation that never exits \"conflicted\" or \"broken\", which would then violate the spec — GET /api/v1/specs/conflicts must return \"All ACTIVE conflicts\" and broken is defined by the current condition \"target was deleted or is no longer in the manifest\". Deliver: (a) broken: the staleness pass (and the push-time inbound check) determines, for each link not currently satisfying the broken condition, whether the target path is present in the target repo's current manifest (using the target-manifest lookup built by the cross-repo resolution task); a target absent from the ledger OR absent from its repo's current manifest transitions the link to \"broken\" via the existing write-through (store + spec_link_repo.save). A target deprecated via supersedes approval but still present in its manifest must NOT be broken. (b) broken reversion: broken links are RE-CHECKED, not skipped — the daily pass's \"Already known stale/broken — skip\" skip must not prevent re-evaluation of the broken condition; a target that re-enters its repo's manifest (or is restored to the ledger) transitions the link back per the normal active/stale rules (active if the pinned SHA matches the restored current SHA, stale otherwise). (c) conflicted: whenever both endpoints of a conflicts_with link are Approved (detected when the link is stored at sync, when an approval lands, and in the daily pass), the link's stored status becomes \"conflicted\" (persisted). (d) conflicted reversion: conflicted links are re-evaluated in the daily pass and whenever an approval changes (approve, revoke, and the extends-drift reset to Pending): when either endpoint is no longer Approved, the link's stored status reverts to whatever the active/stale/broken rules yield for its current pin and target state. GET /api/v1/specs/conflicts returns exactly the links currently in the conflicted state, and its computed notion must agree with the stored status (no drift between endpoint and graph).",
        "acceptance": [
          "Removing a spec from a repo's manifest (pushing the removal) transitions inbound links targeting it to \"broken\" in the stored graph (visible via GET /api/v1/specs/{path}/links and the durable table), where they previously remained \"active\" through every daily pass.",
          "A conflicts_with link whose two specs are both Approved holds stored status \"conflicted\", and GET /api/v1/specs/conflicts lists exactly those links.",
          "A spec deprecated by supersedes approval but still present in its manifest leaves inbound links non-broken (deprecation is not deletion).",
          "Conflicted reverts: after one endpoint of a conflicted pair loses Approved status (e.g. approval reset to Pending via the extends drift path or revoked), GET /api/v1/specs/conflicts no longer lists the link and its stored status is no longer \"conflicted\" (it is whatever the active/stale/broken rules yield).",
          "Broken reverts: after a removed target spec is re-added to its repo's manifest (or restored to the ledger), the previously-broken inbound link is no longer \"broken\" — it transitions to active or stale per the normal SHA rules on the next staleness pass."
        ],
        "negative_probe": "Declare a conflicts_with link between specs A and B after both are already Approved (so the approval-time gate never rejects): before the fix the stored status stays \"active\" and only the on-the-fly conflicts endpoint knows about the violation; after the fix GET /api/v1/specs/{A}/links shows status \"conflicted\" for the link. Second probe (broken entry): remove B's manifest entry (the ledger keeps B as Deprecated): before the fix the A→B link stays \"active\" through daily passes because the Ok(Some(entry)) path leaves it untouched; after, it is \"broken\". Third probe (broken reversion, the one-way-ratchet defect): re-add B's manifest entry and run the staleness pass — on current code the link is skipped forever by the `status == \"broken\" → continue` guard; after the fix it must leave \"broken\" (active or stale per SHA rules), proving the stored graph tracks current conditions. Fourth probe (conflicted reversion): reset one endpoint of the conflicted pair to Pending and run the pass — on a never-reverting implementation the link stays \"conflicted\" and GET /api/v1/specs/conflicts misreports a resolved conflict as active; after the fix the link is delisted and its stored status reflects the active/stale/broken rules.",
        "dependencies": [
          "spec-links-cross-repo-resolution"
        ],
        "existing_task": null
      },
      {
        "key": "spec-links-approval-gates",
        "title": "Spec Links — approval gates: task-based depends_on blocking, implements approved-at-pinned-SHA, conflict-resolution task on rejected approval, merge-gate warning on the same shared predicate",
        "covers": [
          "dcd440115bac4622a351c857",
          "cb0d56cefcb50d54be0121c4"
        ],
        "required_behavior": "Approval Gates: three rows deviate from the contract; Merge Gates: the depends_on warning judges \"isn't implemented yet\" by the target's ledger approval_status (merge_processor.rs Check 3, ~2395-2410), which per controller review feedback (cb0d56cefcb50d54be0121c4) was never the specced signal — and since this task removes the approval-based model, the merge gate must not keep using it. (1) depends_on (\"Implementation tasks for source spec are blocked until target spec's implementation tasks are complete\"): remove the approval-time DependsOn rejection in approve_spec (it blocks approval of the source and models completeness as the target's approval_status — per spec-lifecycle.md approval precedes decomposition, so approval never implied implementation). Install the specced mechanism: implementation tasks of a spec with an outbound depends_on link (tasks with spec_path == that spec, task_type Implementation / label spec-implementation) cannot be started while any depends_on target's implementation is incomplete. Implementation-complete predicate, SHARED VERBATIM by the merge gate below: the target spec has at least one implementation task (tasks.list_by_spec_path) and all of them are Done — equivalently, incomplete = NOT (at least one implementation task AND all Done), which includes the zero-task case. Enforcement: spawning an agent for, or starting, such a task is refused with an error naming the unsatisfied dependency spec; affected tasks surface as Blocked with a recorded reason; once the target's implementation tasks are all Done, the same task starts successfully (re-evaluated at start/spawn). (2) implements (\"cannot be approved until target (parent) spec is approved at the pinned SHA\"): in addition to parent Approved, approve_spec's implements gate must compare the link's pinned target_sha to the parent's current ledger SHA and reject on mismatch with a message naming the pinned and current SHAs. (3) conflicts_with (\"The forge rejects the second approval and creates a conflict-resolution task\"): keep the existing rejection with its specced message, and on rejection create a real durable conflict-resolution Task (tasks repository; workspace/repo-scoped to the spec being approved; labeled conflict-resolution; High priority; describing both conflicting specs and the link). (4) Merge Gates depends_on warning (merge_processor.rs Check 3): replace the approval-status predicate with the EXACT shared predicate from item (1) — emit the warning (never a block: existing StaleSpecWarning event plus warn log, merge proceeds) whenever the referenced spec has an outbound depends_on link whose target does NOT satisfy (at least one implementation task AND all Done). Zero implementation tasks therefore warns: a depends_on target that has never been decomposed (the normal state per spec-lifecycle.md, where approval precedes decomposition) is not implemented and its dependency is not satisfied, so the warning must fire there too; an implementation that narrows item (4) to \"has at least one implementation task and not all are Done\" reintroduces the silent-warning defect in the most common case and is a contract violation. The superseded-spec rejection (Check 1, merge_processor.rs ~2291) and the conflicts_with block (Check 2, ~2317) remain unchanged — both verified real.",
        "acceptance": [
          "An implementation task for spec S (depends_on T) cannot be spawned or started while T has an open implementation task; after T's implementation tasks are all Done, the same spawn/start succeeds. Approving S is no longer rejected on depends_on (the gate sits on implementation tasks, per the spec's table).",
          "Approving a spec whose implements parent is Approved at a SHA different from the link's pin fails with a message naming both SHAs; approving while the parent is at the pinned SHA succeeds.",
          "A rejected second conflicts_with approval leaves a durable conflict-resolution task (label, both spec paths, correct workspace/repo scope) while the approval still fails with the specced message.",
          "An MR referencing a spec whose depends_on target is Approved with open (not-all-Done) implementation tasks emits the StaleSpecWarning warning event and the merge proceeds (warn, not block); an MR whose dependency's implementation tasks are all Done emits no warning.",
          "Zero-task case: an MR referencing a spec whose depends_on target is Approved with ZERO implementation tasks emits the StaleSpecWarning warning event and the merge proceeds (warn, not block) — the shared predicate's NOT (≥1 task AND all Done) covers the never-decomposed target."
        ],
        "negative_probe": "Approve spec T (ledger status Approved) while leaving one of T's spec-implementation tasks InProgress, then merge an MR referencing S which depends_on T: before the fix Check 3 sees approval_status == Approved and stays silent, so the merge proceeds with no warning despite the unimplemented dependency; after the fix the warning event is emitted and the merge still proceeds (asserting a block here would be wrong — the spec says warning). Second probe (zero-task target, the narrowed-predicate hazard): approve T and give it ZERO implementation tasks, then merge an MR referencing S — on an implementation using the narrowed \"≥1 task and not all Done\" predicate no warning is emitted and this probe fails; on the shared predicate the StaleSpecWarning event is emitted and the merge proceeds. Third probe: attempt to spawn an agent on S's implementation task while T's tasks are incomplete — before the fix the spawn succeeds (the old gate only blocked approval of S, never tasks); after, the spawn is refused with an error naming T, and completing T's tasks unblocks it.",
        "dependencies": [
          "spec-links-sha-pinning"
        ],
        "existing_task": null
      },
      {
        "key": "spec-links-graph-repo-filter",
        "title": "Spec Links — repo-scoped graph (?repo= filter) and authorization for the four registry-exempt query endpoints",
        "covers": [
          "58102d3a27f920beeb3fba71"
        ],
        "required_behavior": "spec-links.md §Querying the Graph (API): two defects keep the section open. (A) The repo-filtered variant \"GET /api/v1/specs/graph?repo={id} — Specs for one repo with all outbound/inbound links\" is unimplemented: get_spec_graph (api/specs.rs:1742) takes only State, no Query extractor, and always returns every ledger node and every store edge. Add an optional repo query parameter: when absent, the response is exactly today's full tenant-wide graph (unchanged). When present: nodes are the ledger entries whose repo_id equals the given repo id (SpecLedgerEntry.repo_id), and edges are all stored links touching that repo — source_repo_id == repo (outbound and same-repo inbound) or target_repo_id == repo (cross-repo inbound) — i.e. the repo's specs with all outbound and inbound links. An unknown/invalid repo id returns an explicit error (400/404), not an empty graph. (B) Per controller review feedback (58102d3a27f920beeb3fba71:exempt-query-endpoints-no-authorization): four of the seven specced endpoints — GET /api/v1/specs/stale-links, GET /api/v1/specs/conflicts, GET /api/v1/specs/{path}/dependents, GET /api/v1/specs/{path}/dependencies — are listed in the frozen ABAC route-registry exemption file (scripts/abac-route-registry-exemptions.txt lines 37-41) and their handlers (get_stale_links:1841, get_conflicts:1858, get_spec_dependents:1776, get_spec_dependencies:1809 in api/specs.rs) take only State/Path with no auth extractor and no scope comparison, so any authenticated principal in any tenant receives the forge-wide links store (cross-tenant spec-graph data) — the same hazard class as the patrol endpoint, violating the repo invariant that every registry-exempt route MUST enforce per-handler authorization (check-abac-exempt-handlers.sh passes only because its Rule 1 skips handlers that name no auth extractor). Bring all four under middleware ABAC: register each as a RouteResourceMapping::api entry in abac_middleware.rs (mirroring /api/v1/specs/:path/links at :430 and /api/v1/specs/graph at :425, both already mapped, resource \"spec\") and REMOVE the four routes from scripts/abac-route-registry-exemptions.txt — the file must shrink; no new exemptions, no duplicate resolver entries (the registry is frozen at baseline counts). Do NOT add a resolver entry for /api/v1/specs/:path/reject (also in the exemption file) — it is outside this section's scope and adding an unused mapping is out of contract. If any of the four cannot be resolver-mapped for a structural reason discovered during implementation, the fallback is per-handler scope enforcement in the handler body (load the scoped entity, compare tenant/workspace against the caller, return Forbidden on mismatch) — never leaving it exempt with no authorization decision.",
        "acceptance": [
          "GET /api/v1/specs/graph (no parameter) returns the same full graph as before.",
          "GET /api/v1/specs/graph?repo={id} returns only that repo's spec nodes plus every link touching them, including inbound links whose source spec lives in another repo.",
          "An invalid repo id yields an explicit error response rather than an empty or unfiltered graph.",
          "The four query endpoints (stale-links, conflicts, dependents, dependencies) are removed from scripts/abac-route-registry-exemptions.txt and have RouteResourceMapping resolver entries; scripts/check-abac-route-registry.sh and scripts/check-abac-exempt-handlers.sh pass with a strictly smaller exemption file.",
          "A principal authenticated in tenant B calling GET /api/v1/specs/stale-links, /specs/conflicts, /specs/{path}/dependents, or /specs/{path}/dependencies does not receive tenant A's spec-link data (policy-denied or scoped to tenant B), where on current code the call succeeds and returns the forge-wide store."
        ],
        "negative_probe": "Repo filter: in a tenant with two repos where repo 1's spec has an inbound depends_on link from repo 2's spec, call GET /api/v1/specs/graph?repo={repo1_id}: assert no node with a different repo_id appears and the repo 2 inbound edge is present. On current code the ?repo parameter is ignored entirely (the handler has no Query extractor), so the response is identical to the unfiltered graph and both assertions fail — proving the filter actually constrains the query rather than the endpoint merely accepting the parameter. Authorization: as a principal scoped to tenant B (with tenant A holding stale/conflicted/linked specs), call the four query endpoints: on current code each call succeeds and returns tenant A's cross-tenant link rows (the handlers take no auth extractor and read the forge-wide store); after the fix the calls are policy-denied or return only tenant B's data, and the exemption file's line count has strictly decreased.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-links-ui-link-direction",
        "title": "Spec Links — spec-detail link direction: API emits it, UI renders inbound vs outbound with staleness indicators",
        "covers": [
          "5e761a34ffed5b87f0b401c1"
        ],
        "required_behavior": "spec-links.md §Querying the Graph (UI): \"Spec detail page: link section showing all outbound/inbound links with staleness indicators\". The DAG visualization and impact-analysis bullets are real; the spec-detail bullet is broken: DetailPanel.svelte computes the inbound/outbound columns (lines 3674-3675) and '<- from'/'-> to' row labels (3729, 3744) from a direction field on each link object, but SpecLinkResponse (the serialized shape of GET /api/v1/specs/{path}/links, api/specs.rs:1648) has no such field, so link.direction is always undefined — every link, inbound included, is classified outbound, renders in the outbound column, and is labeled '-> to'. Fix at the source: SpecLinkResponse gains a direction field — \"outbound\" when source_path equals the queried path, \"inbound\" when target_path equals it (a self-link is outbound) — computed by get_spec_links relative to the requested path; the dependents/dependencies endpoints populate it consistently (inbound/outbound respectively). DetailPanel's existing direction-based filtering and labels then display the distinction; staleness continues to surface via the existing status badge.",
        "acceptance": [
          "For a spec with one inbound and one outbound link, GET /api/v1/specs/{path}/links returns direction \"inbound\" and \"outbound\" respectively on the corresponding link objects.",
          "The spec-detail links tab renders the inbound link in the inbound column labeled '<- from' and the outbound link in the outbound column labeled '-> to'.",
          "Stale links still show their staleness indicator in the links tab (status badge), unchanged."
        ],
        "negative_probe": "Open the spec-detail links tab for a spec that is the target of another spec's depends_on link: on current code the inbound link renders in the outbound column labeled '-> to' because link.direction is undefined (the API never emits it); after the fix it renders in the inbound column labeled '<- from'. API-level probe: assert the response's direction field equals \"inbound\" for the link whose target_path is the queried spec — undefined on current code, which is exactly the defect.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-links-accountability-patrol",
        "title": "Spec Links — accountability integration: scheduled patrol job, ABAC-gated endpoint (exemption shrink), persona patrol inputs, and a strictly read-only MCP patrol tool",
        "covers": [
          "ac4a8552cbca5b1333030655"
        ],
        "required_behavior": "spec-links.md §Accountability Agent Integration: the five patrol checks are genuinely implemented over the real links store and ledger (spec_patrol.rs run_patrol: stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep depends_on chains), but the integration the heading requires is missing — the patrol exists only as a manually invoked HTTP endpoint, the accountability persona's documented patrol inputs list no spec-graph items, and the endpoint sits in the frozen ABAC exemption file (line 29) with a handler that performs no per-handler authorization at all (any authenticated principal in any tenant can trigger the forge-wide patrol and receive cross-tenant findings plus notification fan-out). Per controller review feedback, the MCP tool's side-effect boundary must be pinned exactly: run_patrol itself is side-effect-free (it only snapshots the links store and spec ledger and returns findings); the notification fan-out (create_notifications_for_error_findings → durable notification rows) lives in the HTTP handler patrol_spec_links (api/specs.rs:1915), not in run_patrol. Deliver: (1) Scheduled job: register a recurring spec-links patrol job in jobs.rs (mirroring the staleness job's registration at :493) that runs run_patrol and performs the notification fan-out for error findings, so the patrol runs on the accountability cycle without manual invocation. (2) Authorization: register /api/v1/patrol/spec-links in the ABAC resolver (RouteResourceMapping, mirroring /api/v1/patrol/spec-lifecycle at abac_middleware.rs:439) and remove the route from scripts/abac-route-registry-exemptions.txt — the exemption file must shrink; no new exemptions, no duplicate resolver entries — so the endpoint gets middleware policy evaluation instead of no authorization decision at all. (3) Persona patrol inputs: update specs/personas/accountability.md \"What You Scan\" to include the spec-graph surfaces (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep depends_on chains) so the accountability agent's documented patrol covers spec-graph awareness. (4) MCP tool (e.g. gyre_spec_patrol) exposing the patrol to the accountability agent: the tool wraps the pure run_patrol/findings computation ONLY — its handler must NOT call create_notifications_for_error_findings and must not create notifications, tasks, or any durable rows; notification fan-out remains exclusively with the scheduled background job and the ABAC-gated HTTP endpoint. Because the tool performs no repository writes it legitimately stays out of the needs_write RBAC gate — note check-mcp-write-tools.sh cannot catch a handler that fans out notifications via a helper (it scans only direct state.<repo>.<write> calls in mcp.rs), so the boundary is pinned here and must be asserted by test: read-only callers may invoke the tool.",
        "acceptance": [
          "The spec-links patrol job is registered in the job registry and, when due, runs the patrol and creates notifications for error findings without any manual invocation.",
          "/api/v1/patrol/spec-links is removed from scripts/abac-route-registry-exemptions.txt and has a RouteResourceMapping resolver entry; scripts/check-abac-route-registry.sh and scripts/check-abac-exempt-handlers.sh pass.",
          "The MCP tool returns the same findings run_patrol returns for the same state, is callable by a ReadOnly-role principal, and a tool call creates zero durable rows (notification and task row counts identical before/after).",
          "specs/personas/accountability.md \"What You Scan\" lists the spec-graph patrol items."
        ],
        "negative_probe": "As a ReadOnly-role principal, call the MCP patrol tool against state containing a stale link beyond threshold: the tool must return the finding AND leave the durable notification row count unchanged (before/after counts equal). If the handler were wired to the HTTP endpoint's fan-out — the exact hazard the review feedback flags, an ungated durable write invisible to check-mcp-write-tools.sh because the write happens inside create_notifications_for_error_findings — the row-count assertion fails while CI stays green. Second probe: a principal without spec access in another tenant calling POST /api/v1/patrol/spec-links receives a policy-denied response; on current code the route is exempt from ABAC and the handler has no auth extractor, so the call succeeds and returns cross-tenant findings.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spec-links-tenant-cycle-detection",
        "title": "Spec Links — cycle detection over the tenant-wide forge graph, not just the pushed manifest",
        "covers": [
          "8772dba48e530c04a3d2f953"
        ],
        "required_behavior": "spec-links.md §Cycle Detection: \"The forge rejects manifest changes that would create cycles in the spec graph.\" Push-time rejection is real and the excluded types match the spec (depends_on/implements/extends participate; conflicts_with is bidirectional, references and supersedes excluded), with default-branch pushes undone and a PushRejected event plus audit on rejection — but detect_link_cycles builds its adjacency list exclusively from `manifest.specs` of the single manifest being pushed; the forge's tenant-wide spec graph (the durable spec_links table, other repos' manifests, previously synced links) is never consulted, so a cycle whose edges are declared in two different manifests or repos — the enterprise case the spec's Problem section exists for — is accepted and lives permanently in the forge-maintained graph. Extend check_spec_link_cycles (keeping the existing git_http.rs rejection wiring) to build the combined graph: all stored links in the durable spec_links table plus the pushed manifest's edges, with node identity as (repo, path) — the pushed manifest's entries attributed to the source repo, resolved target repo ids for cross-repo/cross-workspace targets, same-repo targets attributed to the source repo. A push that would create a cycle anywhere in the combined graph is rejected with a message naming the cycle. Cross-repo edges must be resolved to their repos for correct attribution (requires the cross-repo resolution task's fixes).",
        "acceptance": [
          "Pushing manifest edge A→B (depends_on) in repo 1 succeeds; then pushing manifest edge B→A (depends_on) in repo 2 is rejected with the cycle named, the ref update undone, and a PushRejected event emitted.",
          "Single-manifest cycles (A depends_on B depends_on A in one push) are still rejected exactly as today — no regression.",
          "Excluded link types never trigger rejection: a references-typed B→A second push is accepted, and conflicts_with bidirectionality is not reported as a cycle."
        ],
        "negative_probe": "The two-push cross-repo cycle: before the fix both pushes land cleanly (each push only sees its own manifest's edges) and GET /api/v1/specs/graph shows the A→B→A cycle living permanently in the forge graph; after the fix the second push is rejected at receive time. The companion assertion — a references-typed second push still accepted — proves the detection was widened rather than the gate made indiscriminate.",
        "dependencies": [
          "spec-links-cross-repo-resolution"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "spec-links-sha-pinning": "task-286",
    "spec-links-cross-repo-resolution": "task-287",
    "spec-links-status-lifecycle": "task-288",
    "spec-links-approval-gates": "task-289",
    "spec-links-graph-repo-filter": "task-290",
    "spec-links-ui-link-direction": "task-291",
    "spec-links-accountability-patrol": "task-292",
    "spec-links-tenant-cycle-detection": "task-293",
    "_publication": "task-294",
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
