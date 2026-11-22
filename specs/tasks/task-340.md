---
title: "Publish reviewed spec backlog for specs/system/spec-registry.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/spec-registry.md",
  "plan_id": "831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f",
  "audit": {
    "base": "52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40",
    "requirements": "a458cd7cdf30d69cbd96111eb9b0d2b8e8979afc979c0b52419b93153d6bec1a",
    "sections": [
      {
        "id": "e603a8baa9deb6b6a96730c6",
        "decision": "context",
        "requirement_quote": "> **Status: Implemented (M21)** — The forge spec ledger (`spec_registry` + `spec_approvals` tables), all `GET/POST /api/v1/specs/*` endpoints, spec link graph, and Spec Dashboard UI are live. `specs/manifest.yaml` is not yet enforced server-side (push rejection for unregistered spec files is future work).",
        "detail": "Document-level status annotation, not an implementable requirement. Per the audit rules, coverage labels and status notes are hints, not implementation proof; every implementable claim in this note (ledger, endpoints, index, dashboard, manifest enforcement) lives in a child section and is audited there. Note the annotation is itself partly stale: it says push rejection for unregistered spec files is 'future work', while policy-gated push rejection is implemented in production (TASK-017) — the child sections, not this note, are the contract.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    // TASK-017: Manifest enforcement — reject pushes with unregistered spec files.",
            "detail": "Push rejection for unregistered spec files exists in production (policy-gated), showing the status note's 'future work' claim is outdated; the normative requirements are audited under the Manifest Rules and Ledger Sync sections."
          }
        ]
      },
      {
        "id": "6a0cf1bf749740cd59f794c4",
        "decision": "context",
        "requirement_quote": "Specs are currently identified by convention: files under `specs/` are specs because of their path. The approval ledger tracks approvals by path + SHA. The lifecycle hooks fire on path prefixes. `specs/index.md` is hand-maintained.",
        "detail": "Rationale-only section: it describes the pre-registry fragility that motivates the design (no explicit spec declaration, no per-spec policy, hand-maintained index). No implementable requirement beyond what the child sections specify; the remedy for each listed fragility is audited under the Solution, Manifest, Ledger, and Index sections.",
        "evidence": []
      },
      {
        "id": "2b44e3712461bf73fc68242e",
        "decision": "satisfied",
        "requirement_quote": "2. **Forge spec ledger** (in the database) - tracks runtime state: current SHA, approval status, linked MRs, linked tasks, drift status. This is the source of truth for **the current state of each spec**.",
        "detail": "Both systems exist in production. The manifest is a versioned file in git (specs/manifest.yaml) read via `git show` at any commit by read_manifest. The ledger is a durable database store — the spec_ledger_entries SQL table behind the SpecLedgerRepository port with SQLite and PostgreSQL adapters (mem adapter for tests) — wired into AppState and written by sync_spec_ledger on pushes, by the sync-specs endpoint, and by mirror sync; approve/revoke update approval_status through the same store.",
        "evidence": [
          {
            "file": "specs/manifest.yaml",
            "quote": "version: 1\n\n# Default policies applied to all specs unless overridden\ndefaults:\n  requires_approval: true\n  auto_create_tasks: true\n  auto_invalidate_on_change: true",
            "detail": "The git-manifest half of the solution exists as a versioned file in the repository declaring specs and their policies."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "        spec_ledger: store_shared!(dyn SpecLedgerRepository, mem_spec_ledger),",
            "detail": "The forge-ledger half is a port-backed store wired into AppState; in DB-backed modes the store! macro wiring substitutes the SQLite/PostgreSQL SpecLedgerRepository, so ledger state is durable in the database, not process memory."
          },
          {
            "file": "crates/gyre-adapters/migrations/2026-03-23-000010_persist_batch_a/up.sql",
            "quote": "CREATE TABLE IF NOT EXISTS spec_ledger_entries (\n    path TEXT PRIMARY KEY NOT NULL,\n    title TEXT NOT NULL,\n    owner TEXT NOT NULL,\n    kind TEXT,\n    current_sha TEXT NOT NULL,\n    approval_mode TEXT NOT NULL,\n    approval_status TEXT NOT NULL,",
            "detail": "Durable SQL table holding the per-spec runtime state (current SHA, approval mode/status, linked tasks/MRs, drift status), created by migration and read/written by the SQLite and PostgreSQL adapters."
          }
        ]
      },
      {
        "id": "8003153391392a6e2c0e3ed0",
        "decision": "satisfied",
        "requirement_quote": "# Default policies applied to all specs unless overridden\ndefaults:\n  requires_approval: true\n  auto_create_tasks: true\n  auto_invalidate_on_change: true",
        "detail": "The manifest exists in the repository exactly in the specced shape — `version: 1`, a `defaults:` policy block, and per-spec entries carrying owner, approval config (mode, human/agent approvers), gates, and per-spec policy overrides — and is parsed by production code (serde_yaml into SpecManifest with ManifestDefaults and SpecEntry) and read from git at any commit SHA, so it is versioned, diffable, and reviewable as required.",
        "evidence": [
          {
            "file": "specs/manifest.yaml",
            "quote": "  - path: system/design-principles.md\n    title: Design Principles\n    owner: user:jsell\n    approval:\n      mode: human_only\n      human_approvers:\n        - user:jsell\n    gates:\n      - persona: accountability",
            "detail": "The live manifest declares per-spec policies (approval mode, approvers, gates) in the specced form; 31 spec entries are registered."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "pub async fn read_manifest(repo_path: &str, sha: &str) -> Option<SpecManifest> {\n    let git_bin = std::env::var(\"GYRE_GIT_PATH\").unwrap_or_else(|_| \"git\".to_string());\n    let yaml = read_git_file(&git_bin, repo_path, sha, \"specs/manifest.yaml\").await?;",
            "detail": "Production code reads and parses the manifest from any git commit via `git show`, making the in-git manifest the machine-read source of spec identity and policy."
          }
        ]
      },
      {
        "id": "0c04e18483ce2f9bf15e2c1c",
        "decision": "gap",
        "requirement_quote": "| `approval.human_approvers` | string[] | [owner] | Users who can approve; a `team:{slug}` entry counts every member of that workspace team as an approver (user-management.md §Team Management — spec ownership; an unresolvable team reference approves nobody) |",
        "detail": "Most of the schema is implemented: SpecEntry parses path/title/owner/approval.mode/human_approvers/agent_approvers/gates/auto_create_tasks/auto_invalidate_on_change/superseded_by with the specced defaults (approval.mode defaults to human_and_agent), and `team:{slug}` entries are expanded through real workspace-membership lookups at approval-resolution time, with an unresolvable team leaving only the literal `team:slug` string in the list — which matches no real approver id, so it approves nobody. The gap is the default for `approval.human_approvers`: the spec says the default is `[owner]`, but the implementation treats an omitted/empty list as unrestricted — any human approval is valid. No code applies an owner default at parse time or in the validity resolver, so a manifest entry that sets an approval mode but omits human_approvers lets any human approve where the spec reserves approval to the owner.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "        && approval\n            .map(|a| a.human_approvers.is_empty() || a.human_approvers.contains(&event.approver_id))\n            .unwrap_or(true)",
            "detail": "Human-approval validity explicitly treats an empty human_approvers list as unrestricted (any human), and membership is literal containment — the specced [owner] default is never applied, so omitting human_approvers widens approval from owner-only to any-human."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "pub struct ApprovalConfig {\n    pub mode: ApprovalMode,\n    #[serde(default)]\n    pub human_approvers: Vec<String>,",
            "detail": "Plain serde empty-vec default; there is no normalization step that substitutes [owner] when the list is omitted (inspected the full parse path: parse_manifest is a direct serde_yaml::from_str with no post-processing)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        if crate::notifications::parse_team_ref(approver).is_some() {\n            let resolved =\n                crate::notifications::resolve_team_members(state, workspace_id, approver).await;",
            "detail": "The team:{slug} half of the row IS implemented: approvers are expanded through real membership resolution before the validity check, and the `_ => expanded.push(approver.clone())` arm keeps the unresolvable literal so it matches no user id (approves nobody), as specced."
          }
        ]
      },
      {
        "id": "f206399fe63f1922ab2f73a5",
        "decision": "satisfied",
        "requirement_quote": "When `stack_hash` is set, the forge verifies the agent's OIDC token contains a matching `stack_hash` claim. This prevents a weakened persona from producing approvals that satisfy policies written for the original persona.",
        "detail": "Agent approvers and gates share the specced schema (persona required; min_attestation_level defaulting to 1; optional stack_hash). At approval time the forge captures the agent's stack fingerprint from the verified JWT `wl_stack_hash` claim and its attestation level from the recorded workload attestation (never invented), records both on the approval event, and the §9 validity resolver enforces persona match, attestation minimum, and exact stack-hash match when the manifest pins one; when stack_hash is null, any attested agent running the named persona is accepted. Manifest-declared gates carry the same constraints into gate-agent execution (spawned reviewers receive min_attestation_level and stack_hash, which the executor re-checks).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "pub struct AgentApproverConfig {\n    pub persona: String,\n    pub min_attestation_level: Option<u32>,\n    pub stack_hash: Option<String>,\n}",
            "detail": "Agent approver config carries the specced schema; GateConfig (immediately below in the same file) is the identical struct, satisfying 'agent approvers and gates share the same schema'."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "            let wl_stack_hash = claims\n                .get(\"wl_stack_hash\")\n                .and_then(|h| h.as_str())\n                .map(|s| s.to_string());",
            "detail": "The approving agent's stack fingerprint is taken from the verified JWT (OIDC token) claims — auth.jwt_claims is the verified token payload — exactly the specced verification source; attestation level is derived from the agent's stored workload attestation record in the same block."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "        match &cfg.stack_hash {\n            Some(required) => event.stack_hash.as_deref() == Some(required.as_str()),\n            None => true,\n        }",
            "detail": "Agent-approval validity requires an exact stack-hash match when the manifest pins one, and accepts any attested agent running the persona when it is null — both halves of the section's requirement."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "        out.push(ManifestGateSpec {\n            persona: gate.persona.clone(),\n            min_attestation_level: gate.min_attestation_level.unwrap_or(1),\n            stack_hash: gate.stack_hash.clone(),",
            "detail": "Manifest gates materialize with the same attestation/stack-hash constraints (default level 1) and propagate them to the spawned gate agent's review request, so the shared schema is enforced on the gate side too."
          }
        ]
      },
      {
        "id": "ae1931a63a11d0886924f188",
        "decision": "satisfied",
        "requirement_quote": "| `human_and_agent` | Security-sensitive specs, anything where both intent and detail matter | At least one human AND at least one agent must approve |",
        "detail": "All three approval modes are implemented exactly as the table specifies: resolve_approval_status returns Approved for human_only with ≥1 valid human approval, agent_only with ≥1 valid agent approval (satisfying the configured agent_approvers constraints), and human_and_agent only with both; otherwise Pending. The default mode when a manifest entry omits approval config is HumanAndAgent, matching 'Default is human_and_agent'. The resolver is wired into production: every approve/revoke recomputes the ledger's stored approval_status from the recorded events via resolve_new_approval_status.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "        ApprovalMode::HumanOnly => has_valid_human,\n        ApprovalMode::AgentOnly => has_valid_agent,\n        ApprovalMode::HumanAndAgent => has_valid_human && has_valid_agent,\n    };",
            "detail": "The mode matrix is implemented over real recorded approval events (has_valid_human / has_valid_agent each check SHA match, non-revocation, and manifest approver constraints)."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    pub fn effective_approval_mode(&self) -> ApprovalMode {\n        self.approval\n            .as_ref()\n            .map(|a| a.mode.clone())\n            .unwrap_or(ApprovalMode::HumanAndAgent)\n    }",
            "detail": "Default approval mode is human_and_agent when the manifest entry carries no approval config, as specced."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "            let new_status = resolve_new_approval_status(&state, &spec_path, &entry).await;",
            "detail": "The resolver is not dead code: approve_spec (and revoke) call it after every approval mutation and persist the recomputed status to the ledger store, so the mode table governs the live ledger state."
          }
        ]
      },
      {
        "id": "dc58139efeac521a97fffb3d",
        "decision": "gap",
        "requirement_quote": "2. **The manifest itself is a spec.** Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval.",
        "detail": "Rule 2 is not implemented at all: nothing treats specs/manifest.yaml as a spec. The repo's own manifest does not list itself as an entry, no code special-cases the manifest path in the lifecycle hook (in manifest mode, should_create_lifecycle_task creates a task only when the changed file has a manifest entry — 'specs/manifest.yaml' normalizes to 'manifest.yaml', which no manifest declares), and the manifest is not in the ledger, so manifest changes neither create lifecycle tasks nor require approval. Rules 1 and 3 (reject unregistered spec files; reject removing a manifest entry while keeping the file) share one mechanism — check_manifest_coverage at push time — which does reject with 403 FORBIDDEN, but only when the repo's spec policy sets enforce_manifest, and that flag defaults to false (warn-only); §Ledger Sync on Push explicitly permits 'reject push (or warn, policy-dependent)', so the policy gate itself is spec-compliant, but rules 1 and 3 as written ('the forge will reject') hold only for repos that opt in. Rule 4 (manifest as single source of truth over the spec_lifecycle config block) is only partially implemented — accounted in detail under the Spec Lifecycle integration section.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
            "detail": "Manifest-mode lifecycle task creation requires the changed path to have a manifest entry; 'specs/manifest.yaml' has no entry in any manifest (the repo's own specs/manifest.yaml does not list itself — verified by grep), and no special-case for the manifest path exists anywhere in process_spec_lifecycle (inspected the full hook: parse_name-status → should_create_lifecycle_task → classify → create task; the manifest file is diffed like any other file and dropped here). So changes to the manifest trigger no lifecycle hook and require no approval — rule 2 is missing."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let enforce = state\n                .spec_policies\n                .get_for_repo(&repo_id)\n                .await\n                .map(|p| p.enforce_manifest)\n                .unwrap_or(false);",
            "detail": "Rules 1/3 push rejection is policy-gated and defaults to false (warn-only): a missing policy row or enforce_manifest=false means unregistered spec files and entry-removed-but-file-present states only log a warning, so the specced rejection is opt-in per repo rather than forge behavior."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                return (StatusCode::FORBIDDEN, rejection).into_response();",
            "detail": "When enforce_manifest is set, the push IS rejected (403 with the unregistered-file list and ref updates undone), so the enforcement mechanism itself is real — the gap for rules 1/3 is only the default-off policy gate, while rule 2 has no implementation at all."
          }
        ]
      },
      {
        "id": "4d63a1dfc0ed8a65984289ea",
        "decision": "satisfied",
        "requirement_quote": "The forge maintains runtime state for each registered spec:",
        "detail": "Implemented as a durable, port-backed ledger. The spec_ledger_entries SQL table (SQLite and PostgreSQL adapters; mem for tests) holds every runtime-state field the section's sketch requires — path (primary key), title, owner, current_sha, approval_mode, approval_status, linked_tasks, linked_mrs, drift_status, created_at, updated_at — plus repo/workspace scope for signal routing, and is written by sync_spec_ledger on push and by approve/revoke. Deviations from the SQL sketch, none of which remove behavior: the table is named spec_ledger_entries rather than spec_registry; the `last_checked` column has no counterpart (no code maintains a last-checked timestamp — noted because the Accountability patrol's 'pending for more than one Ralph loop cycle' check would need it); approval rows are split across two tables (spec_approvals carrying the signature/state-machine, spec_approval_events carrying approver_type/persona/attestation_level/stack_hash) instead of the single specced spec_approvals table — the 'one table' requirement is stated in the Agent Gates integration section and is accounted as a gap there; and linked_mrs/linked_tasks columns are persisted but populated at read time by GET /api/v1/specs/:path from the task and MR stores rather than maintained in the columns.",
        "evidence": [
          {
            "file": "crates/gyre-adapters/migrations/2026-03-23-000010_persist_batch_a/up.sql",
            "quote": "    linked_tasks TEXT NOT NULL DEFAULT '[]',  -- JSON: Vec<String>\n    linked_mrs TEXT NOT NULL DEFAULT '[]',    -- JSON: Vec<String>\n    drift_status TEXT NOT NULL DEFAULT 'unknown',\n    created_at INTEGER NOT NULL,\n    updated_at INTEGER NOT NULL\n);",
            "detail": "The durable ledger table carries the full runtime-state field set of the section's sketch (only `last_checked` has no counterpart), maintained by real adapters on both storage backends."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/spec_ledger.rs",
            "quote": "    async fn save(&self, entry: &SpecLedgerEntry) -> Result<()> {",
            "detail": "Real persistence path: save() upserts SpecLedgerEntry rows into spec_ledger_entries (same-shaped implementation exists for PostgreSQL in crates/gyre-adapters/src/postgres/spec_ledger.rs), so ledger state survives restarts."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "pub async fn sync_spec_ledger(\n    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,",
            "detail": "The ledger is maintained by production sync code through the port (create/update/deprecate per manifest state at HEAD), called from the git-http post-receive path, POST /repos/:id/sync-specs, and mirror sync."
          }
        ]
      },
      {
        "id": "31ae3bba2b68be8c48fb1860",
        "decision": "satisfied",
        "requirement_quote": "An approval is **valid** when:\n- `spec_sha` matches `current_sha` (not stale)\n- `revoked_at` is null (not revoked)\n- For agents: `attestation_level >= min_attestation_level` from manifest\n- For agents: `stack_hash` matches manifest's required `stack_hash` (if specified)",
        "detail": "All four validity conditions and the three-mode status table are implemented in resolve_approval_status over real recorded SpecApprovalEvents: SHA must equal the ledger's current_sha, revoked events are inactive (is_active checks revoked_at), agent approvals must satisfy a configured agent_approvers entry (persona match, attestation_level >= min_attestation_level with default 1, exact stack_hash match when pinned), and human approvals must come from human_approvers when the list is non-empty (with team:{slug} expansion through workspace membership). The resolver runs after every approval mutation (approve/revoke) and its result is persisted as the ledger's approval_status, so the stored status is always the §9 resolution over current events.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "    if event.approver_type != \"agent\" || event.spec_sha != current_sha || !event.is_active() {\n        return false;\n    }",
            "detail": "Agent-approval validity enforces the not-stale and not-revoked conditions before evaluating the manifest's attestation/stack_hash constraints in the loop that follows."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "        let min_level = cfg.min_attestation_level.unwrap_or(1);\n        if event.attestation_level.unwrap_or(0) < min_level {\n            return false;\n        }",
            "detail": "Attestation minimum from the manifest is enforced (default 1), and the stack-hash exact-match arm follows in the same any() predicate."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "            approval.human_approvers =\n                expand_team_approvers(state, &workspace_id, &approval.human_approvers).await;",
            "detail": "The resolver is invoked with the real manifest entry read from the spec's repo at HEAD and team-scoped approvers expanded through membership before resolution, then the result is saved to the ledger store — production wiring, not audit-only."
          }
        ]
      },
      {
        "id": "de861e52781f4597ffc9d2c4",
        "decision": "satisfied",
        "requirement_quote": "When content changes, ALL existing approvals for the old SHA become stale automatically (the SHA no longer matches). Both human and agent approvals must be re-obtained for the new SHA.",
        "detail": "The state machine is implemented. Push with new content: sync_spec_ledger updates current_sha and resets approval_status to Pending when the entry's effective auto_invalidate_on_change is true; because validity requires spec_sha == current_sha, every old approval (human and agent alike) is automatically stale with no per-row mutation needed. All required approvals submitted: approve_spec recomputes status via resolve_new_approval_status and promotes the ledger entry to Approved when the mode's requirements are met. Spec deleted from manifest: sync marks the ledger entry Deprecated. Re-adding a previously deprecated entry re-pends it. Approvals going stale on content change is enforced by the SHA-match condition in both the resolver and the merge gate's per-SHA ledger query.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                existing.current_sha = blob_sha;\n                existing.updated_at = now;\n                if auto_invalidate {\n                    existing.approval_status = ApprovalStatus::Pending;",
            "detail": "Content change at push advances current_sha and re-pends the entry (per auto_invalidate_on_change), which makes all old-SHA approvals stale via the SHA-match validity condition."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                info!(spec_path = %e.path, \"spec-registry: spec deprecated (removed from manifest)\");\n                e.approval_status = ApprovalStatus::Deprecated;",
            "detail": "Removal from the manifest transitions the ledger entry to Deprecated at sync time; the preceding branch re-pends a Deprecated entry when it reappears in the manifest."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                let _ = state.spec_ledger.save(&entry).await;",
            "detail": "The PENDING → APPROVED transition is durable: after resolve_new_approval_status computes the mode-based status from the recorded approvals, the updated entry is saved to the ledger store."
          }
        ]
      },
      {
        "id": "25dd5440b933a3e2bc8e97bf",
        "decision": "satisfied",
        "requirement_quote": "4. For files under `specs/` not in manifest: reject push (or warn, policy-dependent)",
        "detail": "All four sync steps are implemented in production. sync_spec_ledger (1) reads specs/manifest.yaml from the pushed HEAD via git show, (2) computes each entry's git blob SHA, creates new ledger records (Pending) and, on SHA change, updates current_sha and re-pends/invalidates per auto_invalidate_on_change, (3) marks ledger entries absent from the manifest Deprecated, and (4) handles unregistered files under specs/ exactly as the spec's policy-dependent clause allows: check_manifest_coverage rejects the push with 403 FORBIDDEN (refs undone, PushRejected event emitted) when the repo's spec policy sets enforce_manifest, and logs a warning otherwise. The sync is wired into the git-http post-receive path for default-branch pushes, POST /repos/:id/sync-specs, and mirror sync.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "/// Reads `specs/manifest.yaml` from the new HEAD via `git show`, parses it,",
            "detail": "Step 1: the manifest is read from the new HEAD; the function then computes blob SHAs per entry (get_blob_sha), creates/updates ledger records per steps 2-3, and deprecates absent entries ('spec-registry: spec deprecated (removed from manifest)')."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        // Spec registry: sync ledger from manifest on pushes to the default branch (M21.1).",
            "detail": "The sync runs in the post-receive path on default-branch pushes (same wiring exists in api/repos.rs sync-specs and mirror_sync.rs), so the ledger tracks every landed push."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            if let Err(rejection) = crate::spec_registry::check_manifest_coverage(\n                &repo_path,\n                &update.new_sha,\n                &manifest_paths,\n                enforce,\n            )\n            .await",
            "detail": "Step 4: unregistered spec files under specs/ cause rejection (403 FORBIDDEN, refs undone) when the repo's enforce_manifest policy is set, and a warning otherwise — precisely the 'reject push (or warn, policy-dependent)' behavior the section specifies."
          }
        ]
      },
      {
        "id": "16caff87fba84183d8dfcc06",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/specs/{path}` | GET | Single spec: policy, approval status, linked MRs/tasks, drift |",
        "detail": "All seven endpoints in the table are registered and implemented against the real ledger and approval stores: GET /api/v1/specs (list), GET /api/v1/specs/{path} (single spec with policy/status/links/drift and best-effort content), POST .../approve (with optional Sigstore signature), POST .../revoke (reason required), GET .../history, GET /api/v1/specs/pending, GET /api/v1/specs/drifted. The gap is in the single-spec endpoint's linked-tasks delivery: it populates linked_tasks via TaskRepository::list_by_spec_path with the manifest-relative spelling only ('system/x.md'), while the post-receive lifecycle hook records tasks with the git-rooted spelling ('specs/system/x.md') and the repository matches spec_path exactly — so hook-created drift-review/implementation tasks are invisible in GET /api/v1/specs/{path}'s linked_tasks for specs whose tasks were recorded the hook's way. The spec-lifecycle patrol works around exactly this dual-spelling problem by querying both forms; the API does not. linked_mrs derivation (MR spec_ref prefix match) does not have this defect.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/specs\", get(specs::list_specs))\n        .route(\"/api/v1/specs/pending\", get(specs::list_pending_specs))\n        .route(\"/api/v1/specs/drifted\", get(specs::list_drifted_specs))\n        .route(\"/api/v1/specs/index\", get(specs::spec_index))",
            "detail": "The listed endpoints exist (together with /api/v1/specs/:path, /approve, /revoke, /history registered in the same router block), so the surface itself is complete."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    // Populate linked_tasks from tasks with matching spec_path.\n    entry.linked_tasks = state\n        .tasks\n        .list_by_spec_path(&spec_path)\n        .await\n        .unwrap_or_default()",
            "detail": "linked_tasks is populated with a single-spelling exact query: spec_path here is the manifest-relative form the ledger uses ('system/x.md'), so tasks stored under the alternate spelling never match."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            task.spec_path = Some(path.clone());",
            "detail": "The lifecycle hook records tasks with the git-diff path spelling ('specs/system/x.md') — the other spelling than the one get_spec queries, so the hook's drift-review/implementation tasks are missing from the single-spec response's linked_tasks."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/task.rs",
            "quote": "                .filter(tasks::spec_path.eq(&sp))",
            "detail": "The repository matches spec_path by exact equality (no normalization, no prefix logic; the PostgreSQL adapter is the same), so the two spellings never meet — the linked-tasks half of the row underreports for hook-created tasks."
          },
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "    for query in [rooted_path, alt.as_str()] {",
            "detail": "The dual-spelling hazard is real and known: the patrol's spec_has_live_task queries BOTH spellings to answer spec/task coverage, which the API's single-spec endpoint does not — confirming the linked-tasks linkage in GET /api/v1/specs/{path} is incomplete rather than a different intentional design."
          }
        ]
      },
      {
        "id": "9df024e265cff9471afd92d4",
        "decision": "gap",
        "requirement_quote": "Returns a markdown document with:\n- All specs grouped by directory\n- Current approval status per spec\n- Links to spec files\n- Open task count per spec",
        "detail": "GET /api/v1/specs/index is implemented and generates a real markdown document from the ledger: specs are grouped by directory (BTreeMap on the first path segment), each row carries the current approval status, and rows link to the spec files. Three of the four required contents are present. The fourth — 'Open task count per spec' — is absent: the generated table has only Spec | Status | SHA columns and the handler never queries the task repository, so the index cannot tell a reader which specs have open work. The web UI spec dashboard exists (SpecDashboard.svelte renders the specs list), so that closing sentence is met; the gap is confined to the missing per-spec open task count in the generated document.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        let dir = entry.path.split('/').next().unwrap_or(\"other\").to_string();",
            "detail": "Grouping by directory is real (BTreeMap<String, Vec<SpecLedgerEntry>>), and each row renders title, approval_status, and a markdown link to specs/{path} — three of the four required contents."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        md.push_str(\"| Spec | Status | SHA |\\n\");\n        md.push_str(\"|------|--------|-----|\\n\");",
            "detail": "The generated table columns are only Spec, Status, and truncated SHA: there is no open-task-count column, and the handler body (inspected in full from `pub async fn spec_index` to the response return) contains no query against state.tasks — the specced 'Open task count per spec' is not produced."
          },
          {
            "file": "web/src/components/SpecDashboard.svelte",
            "quote": "   * SpecDashboard — S4.5 Specs View (list pane)",
            "detail": "The web UI half of the section (spec dashboard rendering spec state) exists as a live Svelte component fed from the specs API."
          }
        ]
      },
      {
        "id": "b727d737d4b419076eab3964",
        "decision": "context",
        "requirement_quote": "## Integration with Existing Specs",
        "detail": "Parent heading with no standalone prose or requirements: its entire content is the five child sections (Spec Lifecycle, Agent Gates, Spec-to-Code Binding, Supply Chain, Accountability Agent), each of which is audited separately below. No requirement is counted here to avoid double-counting against the children.",
        "evidence": []
      },
      {
        "id": "ff5387eb7f01a9b7b31c5126",
        "decision": "gap",
        "requirement_quote": "- The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section",
        "detail": "Three of the four bullets are implemented: which files trigger hooks is manifest-driven (should_create_lifecycle_task creates tasks only for manifest entries whose effective auto_create_tasks is true; unregistered files create nothing), per-spec priority overrides are applied from the manifest entry/defaults over the change-kind default, and per-spec gates feed the gate chain (audited under Agent Gates). The supersession bullet is only partially implemented: when a manifest exists the manifest decides task eligibility and priority, but the [spec_lifecycle] config block still overrides the manifest — process_spec_lifecycle returns early when the repo's config has enabled=false, which disables manifest-driven task creation for manifest repos (the config block acts as a master switch the manifest cannot override), and the hook's approval invalidation keys on config.auto_invalidate_approvals rather than the manifest's auto_invalidate_on_change. The code comments cite spec-lifecycle.md §Configuration for the fallback, but spec-lifecycle.md's Configuration section does not mention the manifest at all, so this is an implementation-side harmonization, not a canonical supersession: the config block remains authoritative for enabled/invalidation/dedup even where the manifest declares policy.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
            "detail": "Bullet 1 satisfied: in manifest mode only manifest entries with effective auto_create_tasks (per-entry override, else the defaults: section) trigger lifecycle tasks; files not registered create none."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let priority = manifest\n                .as_ref()\n                .and_then(|m| {\n                    crate::spec_registry::manifest_entry_for_path(m, &path)\n                        .and_then(|e| e.effective_priority(&m.defaults))\n                })\n                .unwrap_or(priority);",
            "detail": "Bullet 2 satisfied: the manifest-declared priority (per-entry, else defaults) overrides the change-kind default from the config block."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    if !config.enabled {\n        info!(repo_id, \"spec-lifecycle: disabled for repo, skipping\");\n        return;\n    }",
            "detail": "The supersession bullet's gap: this early return runs BEFORE the manifest is read, so a repo with a manifest declaring auto_create_tasks: true still creates no lifecycle tasks when its [spec_lifecycle] config sets enabled=false — the config block overrides the manifest instead of being superseded by it."
          },
          {
            "file": "specs/system/spec-lifecycle.md",
            "quote": "## Configuration",
            "detail": "The cited fallback authority does not resolve the contradiction: spec-lifecycle.md's Configuration section defines the [spec_lifecycle] block with no mention of the manifest or supersession, while spec-registry.md requires the block to be superseded by the manifest's defaults — the implementation keeps both authoritative (config for enabled/auto_invalidate/dedup, manifest for eligibility/priority), which satisfies neither statement fully."
          }
        ]
      },
      {
        "id": "4f7e9a6ec00744d40ce24f65",
        "decision": "gap",
        "requirement_quote": "The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.",
        "detail": "The gates-field half of the section is implemented: manifest_gates_for_mr resolves the manifest entry matching the MR's spec_ref path and contributes its gates: entries as gate agents, which are materialized as executed QualityGates carrying persona/min_attestation_level/stack_hash constraints and spawned as reviewing agents — different specs do get different gate agents. The unification half is NOT implemented: spec approvals live in two separate durable stores with separate ports — spec_approvals (SpecApprovalRepository; the per-SHA state-machine ledger the merge gate verify_spec_ref enforces) and spec_approval_events (SpecApprovalEventRepository; the event history the §9 resolver reads) — and approve_spec writes both on every approval. There is no single table: the §9 approval-status resolution and the merge-gate approval check read different tables, and the two records can diverge (the judgment-ledger identity-key fix, visible in approve_spec's comments, had to be applied to one store's writer specifically because the stores are separate).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "pub(crate) fn manifest_gates_for_mr(\n    manifest: Option<&crate::spec_registry::SpecManifest>,\n    spec_ref: Option<&str>,\n) -> Vec<ManifestGateSpec> {",
            "detail": "The gates: field feeds the gate chain as specced: the manifest entry matching the MR's spec_ref contributes its gate configs, which are materialized and executed (spawned gate agents receive persona, min_attestation_level, and stack_hash)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    pub spec_approvals: Arc<dyn gyre_ports::SpecApprovalRepository>,",
            "detail": "First of two approval stores in AppState — the per-SHA ledger (spec_approvals table) that verify_spec_ref/verify_signed_spec_ref enforce at merge."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    pub spec_approval_history: Arc<dyn gyre_ports::SpecApprovalEventRepository>,",
            "detail": "Second approval store in AppState — the event history (spec_approval_events table) that resolve_new_approval_status reads for §9 status resolution. Two ports, two tables: not 'one table, one source of truth'."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    let _ = state.spec_approval_history.record(&event).await;",
            "detail": "Every approval is dual-written: the same approval is recorded into the event store here and into the spec_approvals ledger immediately below (`if let Err(e) = state.spec_approvals.create(&ledger_entry).await`), each with different field sets and identity keys — the unification this section requires is absent."
          }
        ]
      },
      {
        "id": "a81aa30dd4413677c04273d8",
        "decision": "gap",
        "requirement_quote": "- If the spec has `requires_approval: false`, any SHA is accepted",
        "detail": "The middle bullet is implemented (policy-gated): when a repo's spec policy sets require_spec_ref/require_approved_spec/require_signed_approval, the merge processor calls verify_spec_ref, which rejects merges whose spec_ref (path, sha) has no active approval in the per-SHA ledger — the referenced SHA must be approved. The other two bullets are not implemented: (1) the spec_ref path's existence in the manifest is never checked — verify_spec_ref queries only approval rows and never reads specs/manifest.yaml, so 'path must exist in the manifest' has no dedicated check (an unregistered path merely fails indirectly, and only when the approval policy is on); (2) the requires_approval: false exemption is missing — verify_spec_ref demands an active approval row regardless of the manifest's policy, and nothing in the merge path (merge_processor spec-policy block, verify_spec_ref, verify_signed_spec_ref — all inspected) consults the manifest or the requires_approval flag, so an MR referencing a spec with requires_approval: false — which per §9/§17 is Approved with no approval events — fails the require_approved_spec gate with 'no active approval in the ledger' because no approval rows exist for it.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/gates.rs",
            "quote": "    let for_sha = state\n        .spec_approvals\n        .find_by_spec_sha(path, sha)\n        .await\n        .unwrap_or_default();\n    let has_active_approval = for_sha.iter().any(|a| a.is_active());",
            "detail": "The approved-SHA check is real (bullet 2), but its entire inputs are the per-SHA approval rows: the manifest is never read, so neither the manifest-existence check (bullet 1) nor the requires_approval: false exemption (bullet 3) can be evaluated here."
          },
          {
            "file": "crates/gyre-server/src/api/gates.rs",
            "quote": "            \"spec '{path}' at SHA '{sha}' has no active approval in the ledger\"",
            "detail": "The only failure branch: a requires_approval: false spec with no approval rows gets this rejection when require_approved_spec is on — the specced 'any SHA is accepted' exemption has no code path (grep for requires_approval across the merge/gate modules returns nothing; the flag is only evaluated inside spec_registry's resolver and specs.rs request fixtures)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                if policy.require_approved_spec {\n                    if let Err(reason) = crate::api::gates::verify_spec_ref(state, spec_ref).await {",
            "detail": "The pre-accept call site: the inspected chain (this block → verify_spec_ref → spec_approvals.find_by_spec_sha) contains no manifest lookup and no requires_approval branch, so two of the section's three checks are absent from the enforced path."
          }
        ]
      },
      {
        "id": "e59bb14c2bff758c2dd86cb7",
        "decision": "satisfied",
        "requirement_quote": "The AIBOM includes manifest state at release time:\n- Which specs were approved and by whom\n- Which specs had open drift-review tasks (flagged as risk)\n- Manifest version (git SHA of `specs/manifest.yaml`)",
        "detail": "build_spec_registry_section produces the spec_registry AIBOM section from real state: per-spec ledger status for the release's repo; for approved specs, the approval chain (approver ids, human/agent type, persona, timestamps) from the recorded active approval events; drift risk flagged from the ledger's drift status (the drifted-spec notion this spec's own ledger defines and GET /api/v1/specs/drifted exposes); the manifest version as the git blob SHA of specs/manifest.yaml resolved from the repo at HEAD (absent manifest reported as None, never fabricated); plus spec-to-code binding status derived from the repo's MRs and the same per-SHA ledger the merge gate enforces. Storage failures propagate rather than under-reporting, and the section is shared by GET /repos/:id/aibom and POST /release/prepare so the release artifact and the standalone AIBOM cannot disagree.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/aibom_spec_registry.rs",
            "quote": "    let manifest_version =\n        crate::git_refs::resolve_blob_sha(&repo.path, \"specs/manifest.yaml\").await;",
            "detail": "Manifest version is the real git blob SHA of specs/manifest.yaml at HEAD (None reported as absent, per the field's doc comment 'never fabricated')."
          },
          {
            "file": "crates/gyre-server/src/aibom_spec_registry.rs",
            "quote": "        if entry.drift_status == \"drifted\" {\n            drifted += 1;\n        }",
            "detail": "Drift risk is flagged per spec and counted in the summary, using the ledger drift status this specification itself defines (drift_status column, /specs/drifted endpoint)."
          },
          {
            "file": "crates/gyre-server/src/aibom_spec_registry.rs",
            "quote": "                let mut active: Vec<_> = state\n                    .spec_approval_history\n                    .list_by_path(&entry.path)\n                    .await?",
            "detail": "'Approved and by whom' comes from the real approval event history filtered to the current SHA and active events — approver ids, types, personas, and timestamps populate the AIBOM approval chain."
          },
          {
            "file": "crates/gyre-server/src/aibom_spec_registry.rs",
            "quote": "//! The section is shared by `GET /api/v1/repos/:id/aibom` (embedded in the",
            "detail": "The section is embedded in both the AIBOM response and POST /release/prepare release metadata, satisfying 'at release time'."
          }
        ]
      },
      {
        "id": "2236f6ecfe981117d57f7990",
        "decision": "gap",
        "requirement_quote": "The Accountability agent's patrol gains manifest-aware checks:\n- Spec files that exist but aren't in the manifest (orphans)\n- Manifest entries whose files don't exist (stale entries)\n- Specs with `requires_approval: true` that have been `pending` for more than one Ralph loop cycle\n- Specs with `auto_create_tasks: true` that were modified but have no corresponding task",
        "detail": "None of the four manifest-aware patrol checks exists as a patrol check. The two live patrols check other contracts: spec_lifecycle_patrol (spec-lifecycle.md's accountability integration, task-204) checks drift-review/implementation task ages and modified-specs-without-tasks — but its coverage check keys on the repo's SpecLifecycleConfig watched prefixes, not the manifest's auto_create_tasks, and it has no orphan, stale-entry, or pending-age check; spec_patrol inspects the spec link graph (stale links, orphaned supersessions, unresolved conflicts), not manifest coverage. Orphan detection (files under specs/ not in the manifest) exists only as push-time warn/reject in check_manifest_coverage, not as an accountability patrol finding; stale manifest entries (entry with no file at HEAD) produce only a sync-time warning log; and there is no pending-age signal at all — the ledger entry has no last_checked/pending-since timestamp from which 'pending for more than one Ralph loop cycle' could be evaluated, so that check cannot even be expressed against current state.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "//! - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle\n//! - `spec-implementation` tasks that have been in Backlog for more than N days\n//! - Specs that were modified but have no corresponding task (should never happen",
            "detail": "The live lifecycle patrol implements spec-lifecycle.md's three checks, not this section's four manifest-aware checks — no orphan check, no stale-entry check, no requires_approval pending-age check."
          },
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "        if !config.is_watched(&path) {\n            continue;\n        }",
            "detail": "Even the closest check (modified-without-task) keys on the repo's SpecLifecycleConfig watched prefixes — the path-prefix mechanism this spec's Problem section rejects — rather than the manifest's auto_create_tasks: true, so a manifest entry with auto_create_tasks enabled but outside the config's watched prefixes is never examined."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                \"spec-registry: file under specs/ is not registered in manifest.yaml — \\",
            "detail": "Orphan detection exists only as a push-time warning (or policy-gated rejection) inside check_manifest_coverage/sync — a log line, not an accountability patrol finding escalated to the orchestrator."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                    \"spec-registry: manifest entry has no corresponding file at HEAD\"",
            "detail": "Stale-entry detection (manifest entry with no file) is likewise only a sync-time warning log in sync_spec_ledger; no patrol surfaces it as a finding."
          },
          {
            "file": "crates/gyre-server/src/spec_patrol.rs",
            "quote": "//! - Orphaned supersessions: a spec is superseded but code still references it",
            "detail": "The other patrol inspects the spec link graph (stale/orphaned-supersession/unresolved-conflict links per spec-links.md), not manifest coverage — confirming the four manifest-aware checks in this section have no implementation anywhere (inspected both patrol modules and the patrol endpoints /api/v1/patrol/spec-links and /api/v1/patrol/spec-lifecycle)."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "0c04e18483ce2f9bf15e2c1c",
        "decision": "gap",
        "requirement_quote": "| `approval.human_approvers` | string[] | [owner] | Users who can approve; a `team:{slug}` entry counts every member of that workspace team as an approver (user-management.md §Team Management — spec ownership; an unresolvable team reference approves nobody) |",
        "detail": "Most of the schema is implemented: SpecEntry parses path/title/owner/approval.mode/human_approvers/agent_approvers/gates/auto_create_tasks/auto_invalidate_on_change/superseded_by with the specced defaults (approval.mode defaults to human_and_agent), and `team:{slug}` entries are expanded through real workspace-membership lookups at approval-resolution time, with an unresolvable team leaving only the literal `team:slug` string in the list — which matches no real approver id, so it approves nobody. The gap is the default for `approval.human_approvers`: the spec says the default is `[owner]`, but the implementation treats an omitted/empty list as unrestricted — any human approval is valid. No code applies an owner default at parse time or in the validity resolver, so a manifest entry that sets an approval mode but omits human_approvers lets any human approve where the spec reserves approval to the owner.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "        && approval\n            .map(|a| a.human_approvers.is_empty() || a.human_approvers.contains(&event.approver_id))\n            .unwrap_or(true)",
            "detail": "Human-approval validity explicitly treats an empty human_approvers list as unrestricted (any human), and membership is literal containment — the specced [owner] default is never applied, so omitting human_approvers widens approval from owner-only to any-human."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "pub struct ApprovalConfig {\n    pub mode: ApprovalMode,\n    #[serde(default)]\n    pub human_approvers: Vec<String>,",
            "detail": "Plain serde empty-vec default; there is no normalization step that substitutes [owner] when the list is omitted (inspected the full parse path: parse_manifest is a direct serde_yaml::from_str with no post-processing)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        if crate::notifications::parse_team_ref(approver).is_some() {\n            let resolved =\n                crate::notifications::resolve_team_members(state, workspace_id, approver).await;",
            "detail": "The team:{slug} half of the row IS implemented: approvers are expanded through real membership resolution before the validity check, and the `_ => expanded.push(approver.clone())` arm keeps the unresolvable literal so it matches no user id (approves nobody), as specced."
          }
        ]
      },
      {
        "id": "dc58139efeac521a97fffb3d",
        "decision": "gap",
        "requirement_quote": "2. **The manifest itself is a spec.** Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval.",
        "detail": "Rule 2 is not implemented at all: nothing treats specs/manifest.yaml as a spec. The repo's own manifest does not list itself as an entry, no code special-cases the manifest path in the lifecycle hook (in manifest mode, should_create_lifecycle_task creates a task only when the changed file has a manifest entry — 'specs/manifest.yaml' normalizes to 'manifest.yaml', which no manifest declares), and the manifest is not in the ledger, so manifest changes neither create lifecycle tasks nor require approval. Rules 1 and 3 (reject unregistered spec files; reject removing a manifest entry while keeping the file) share one mechanism — check_manifest_coverage at push time — which does reject with 403 FORBIDDEN, but only when the repo's spec policy sets enforce_manifest, and that flag defaults to false (warn-only); §Ledger Sync on Push explicitly permits 'reject push (or warn, policy-dependent)', so the policy gate itself is spec-compliant, but rules 1 and 3 as written ('the forge will reject') hold only for repos that opt in. Rule 4 (manifest as single source of truth over the spec_lifecycle config block) is only partially implemented — accounted in detail under the Spec Lifecycle integration section.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
            "detail": "Manifest-mode lifecycle task creation requires the changed path to have a manifest entry; 'specs/manifest.yaml' has no entry in any manifest (the repo's own specs/manifest.yaml does not list itself — verified by grep), and no special-case for the manifest path exists anywhere in process_spec_lifecycle (inspected the full hook: parse_name-status → should_create_lifecycle_task → classify → create task; the manifest file is diffed like any other file and dropped here). So changes to the manifest trigger no lifecycle hook and require no approval — rule 2 is missing."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let enforce = state\n                .spec_policies\n                .get_for_repo(&repo_id)\n                .await\n                .map(|p| p.enforce_manifest)\n                .unwrap_or(false);",
            "detail": "Rules 1/3 push rejection is policy-gated and defaults to false (warn-only): a missing policy row or enforce_manifest=false means unregistered spec files and entry-removed-but-file-present states only log a warning, so the specced rejection is opt-in per repo rather than forge behavior."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "                return (StatusCode::FORBIDDEN, rejection).into_response();",
            "detail": "When enforce_manifest is set, the push IS rejected (403 with the unregistered-file list and ref updates undone), so the enforcement mechanism itself is real — the gap for rules 1/3 is only the default-off policy gate, while rule 2 has no implementation at all."
          }
        ]
      },
      {
        "id": "16caff87fba84183d8dfcc06",
        "decision": "gap",
        "requirement_quote": "| `GET /api/v1/specs/{path}` | GET | Single spec: policy, approval status, linked MRs/tasks, drift |",
        "detail": "All seven endpoints in the table are registered and implemented against the real ledger and approval stores: GET /api/v1/specs (list), GET /api/v1/specs/{path} (single spec with policy/status/links/drift and best-effort content), POST .../approve (with optional Sigstore signature), POST .../revoke (reason required), GET .../history, GET /api/v1/specs/pending, GET /api/v1/specs/drifted. The gap is in the single-spec endpoint's linked-tasks delivery: it populates linked_tasks via TaskRepository::list_by_spec_path with the manifest-relative spelling only ('system/x.md'), while the post-receive lifecycle hook records tasks with the git-rooted spelling ('specs/system/x.md') and the repository matches spec_path exactly — so hook-created drift-review/implementation tasks are invisible in GET /api/v1/specs/{path}'s linked_tasks for specs whose tasks were recorded the hook's way. The spec-lifecycle patrol works around exactly this dual-spelling problem by querying both forms; the API does not. linked_mrs derivation (MR spec_ref prefix match) does not have this defect.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "        .route(\"/api/v1/specs\", get(specs::list_specs))\n        .route(\"/api/v1/specs/pending\", get(specs::list_pending_specs))\n        .route(\"/api/v1/specs/drifted\", get(specs::list_drifted_specs))\n        .route(\"/api/v1/specs/index\", get(specs::spec_index))",
            "detail": "The listed endpoints exist (together with /api/v1/specs/:path, /approve, /revoke, /history registered in the same router block), so the surface itself is complete."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    // Populate linked_tasks from tasks with matching spec_path.\n    entry.linked_tasks = state\n        .tasks\n        .list_by_spec_path(&spec_path)\n        .await\n        .unwrap_or_default()",
            "detail": "linked_tasks is populated with a single-spelling exact query: spec_path here is the manifest-relative form the ledger uses ('system/x.md'), so tasks stored under the alternate spelling never match."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            task.spec_path = Some(path.clone());",
            "detail": "The lifecycle hook records tasks with the git-diff path spelling ('specs/system/x.md') — the other spelling than the one get_spec queries, so the hook's drift-review/implementation tasks are missing from the single-spec response's linked_tasks."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/task.rs",
            "quote": "                .filter(tasks::spec_path.eq(&sp))",
            "detail": "The repository matches spec_path by exact equality (no normalization, no prefix logic; the PostgreSQL adapter is the same), so the two spellings never meet — the linked-tasks half of the row underreports for hook-created tasks."
          },
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "    for query in [rooted_path, alt.as_str()] {",
            "detail": "The dual-spelling hazard is real and known: the patrol's spec_has_live_task queries BOTH spellings to answer spec/task coverage, which the API's single-spec endpoint does not — confirming the linked-tasks linkage in GET /api/v1/specs/{path} is incomplete rather than a different intentional design."
          }
        ]
      },
      {
        "id": "9df024e265cff9471afd92d4",
        "decision": "gap",
        "requirement_quote": "Returns a markdown document with:\n- All specs grouped by directory\n- Current approval status per spec\n- Links to spec files\n- Open task count per spec",
        "detail": "GET /api/v1/specs/index is implemented and generates a real markdown document from the ledger: specs are grouped by directory (BTreeMap on the first path segment), each row carries the current approval status, and rows link to the spec files. Three of the four required contents are present. The fourth — 'Open task count per spec' — is absent: the generated table has only Spec | Status | SHA columns and the handler never queries the task repository, so the index cannot tell a reader which specs have open work. The web UI spec dashboard exists (SpecDashboard.svelte renders the specs list), so that closing sentence is met; the gap is confined to the missing per-spec open task count in the generated document.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        let dir = entry.path.split('/').next().unwrap_or(\"other\").to_string();",
            "detail": "Grouping by directory is real (BTreeMap<String, Vec<SpecLedgerEntry>>), and each row renders title, approval_status, and a markdown link to specs/{path} — three of the four required contents."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "        md.push_str(\"| Spec | Status | SHA |\\n\");\n        md.push_str(\"|------|--------|-----|\\n\");",
            "detail": "The generated table columns are only Spec, Status, and truncated SHA: there is no open-task-count column, and the handler body (inspected in full from `pub async fn spec_index` to the response return) contains no query against state.tasks — the specced 'Open task count per spec' is not produced."
          },
          {
            "file": "web/src/components/SpecDashboard.svelte",
            "quote": "   * SpecDashboard — S4.5 Specs View (list pane)",
            "detail": "The web UI half of the section (spec dashboard rendering spec state) exists as a live Svelte component fed from the specs API."
          }
        ]
      },
      {
        "id": "ff5387eb7f01a9b7b31c5126",
        "decision": "gap",
        "requirement_quote": "- The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section",
        "detail": "Three of the four bullets are implemented: which files trigger hooks is manifest-driven (should_create_lifecycle_task creates tasks only for manifest entries whose effective auto_create_tasks is true; unregistered files create nothing), per-spec priority overrides are applied from the manifest entry/defaults over the change-kind default, and per-spec gates feed the gate chain (audited under Agent Gates). The supersession bullet is only partially implemented: when a manifest exists the manifest decides task eligibility and priority, but the [spec_lifecycle] config block still overrides the manifest — process_spec_lifecycle returns early when the repo's config has enabled=false, which disables manifest-driven task creation for manifest repos (the config block acts as a master switch the manifest cannot override), and the hook's approval invalidation keys on config.auto_invalidate_approvals rather than the manifest's auto_invalidate_on_change. The code comments cite spec-lifecycle.md §Configuration for the fallback, but spec-lifecycle.md's Configuration section does not mention the manifest at all, so this is an implementation-side harmonization, not a canonical supersession: the config block remains authoritative for enabled/invalidation/dedup even where the manifest declares policy.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
            "detail": "Bullet 1 satisfied: in manifest mode only manifest entries with effective auto_create_tasks (per-entry override, else the defaults: section) trigger lifecycle tasks; files not registered create none."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "            let priority = manifest\n                .as_ref()\n                .and_then(|m| {\n                    crate::spec_registry::manifest_entry_for_path(m, &path)\n                        .and_then(|e| e.effective_priority(&m.defaults))\n                })\n                .unwrap_or(priority);",
            "detail": "Bullet 2 satisfied: the manifest-declared priority (per-entry, else defaults) overrides the change-kind default from the config block."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    if !config.enabled {\n        info!(repo_id, \"spec-lifecycle: disabled for repo, skipping\");\n        return;\n    }",
            "detail": "The supersession bullet's gap: this early return runs BEFORE the manifest is read, so a repo with a manifest declaring auto_create_tasks: true still creates no lifecycle tasks when its [spec_lifecycle] config sets enabled=false — the config block overrides the manifest instead of being superseded by it."
          },
          {
            "file": "specs/system/spec-lifecycle.md",
            "quote": "## Configuration",
            "detail": "The cited fallback authority does not resolve the contradiction: spec-lifecycle.md's Configuration section defines the [spec_lifecycle] block with no mention of the manifest or supersession, while spec-registry.md requires the block to be superseded by the manifest's defaults — the implementation keeps both authoritative (config for enabled/auto_invalidate/dedup, manifest for eligibility/priority), which satisfies neither statement fully."
          }
        ]
      },
      {
        "id": "4f7e9a6ec00744d40ce24f65",
        "decision": "gap",
        "requirement_quote": "The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.",
        "detail": "The gates-field half of the section is implemented: manifest_gates_for_mr resolves the manifest entry matching the MR's spec_ref path and contributes its gates: entries as gate agents, which are materialized as executed QualityGates carrying persona/min_attestation_level/stack_hash constraints and spawned as reviewing agents — different specs do get different gate agents. The unification half is NOT implemented: spec approvals live in two separate durable stores with separate ports — spec_approvals (SpecApprovalRepository; the per-SHA state-machine ledger the merge gate verify_spec_ref enforces) and spec_approval_events (SpecApprovalEventRepository; the event history the §9 resolver reads) — and approve_spec writes both on every approval. There is no single table: the §9 approval-status resolution and the merge-gate approval check read different tables, and the two records can diverge (the judgment-ledger identity-key fix, visible in approve_spec's comments, had to be applied to one store's writer specifically because the stores are separate).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "pub(crate) fn manifest_gates_for_mr(\n    manifest: Option<&crate::spec_registry::SpecManifest>,\n    spec_ref: Option<&str>,\n) -> Vec<ManifestGateSpec> {",
            "detail": "The gates: field feeds the gate chain as specced: the manifest entry matching the MR's spec_ref contributes its gate configs, which are materialized and executed (spawned gate agents receive persona, min_attestation_level, and stack_hash)."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    pub spec_approvals: Arc<dyn gyre_ports::SpecApprovalRepository>,",
            "detail": "First of two approval stores in AppState — the per-SHA ledger (spec_approvals table) that verify_spec_ref/verify_signed_spec_ref enforce at merge."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "    pub spec_approval_history: Arc<dyn gyre_ports::SpecApprovalEventRepository>,",
            "detail": "Second approval store in AppState — the event history (spec_approval_events table) that resolve_new_approval_status reads for §9 status resolution. Two ports, two tables: not 'one table, one source of truth'."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "    let _ = state.spec_approval_history.record(&event).await;",
            "detail": "Every approval is dual-written: the same approval is recorded into the event store here and into the spec_approvals ledger immediately below (`if let Err(e) = state.spec_approvals.create(&ledger_entry).await`), each with different field sets and identity keys — the unification this section requires is absent."
          }
        ]
      },
      {
        "id": "a81aa30dd4413677c04273d8",
        "decision": "gap",
        "requirement_quote": "- If the spec has `requires_approval: false`, any SHA is accepted",
        "detail": "The middle bullet is implemented (policy-gated): when a repo's spec policy sets require_spec_ref/require_approved_spec/require_signed_approval, the merge processor calls verify_spec_ref, which rejects merges whose spec_ref (path, sha) has no active approval in the per-SHA ledger — the referenced SHA must be approved. The other two bullets are not implemented: (1) the spec_ref path's existence in the manifest is never checked — verify_spec_ref queries only approval rows and never reads specs/manifest.yaml, so 'path must exist in the manifest' has no dedicated check (an unregistered path merely fails indirectly, and only when the approval policy is on); (2) the requires_approval: false exemption is missing — verify_spec_ref demands an active approval row regardless of the manifest's policy, and nothing in the merge path (merge_processor spec-policy block, verify_spec_ref, verify_signed_spec_ref — all inspected) consults the manifest or the requires_approval flag, so an MR referencing a spec with requires_approval: false — which per §9/§17 is Approved with no approval events — fails the require_approved_spec gate with 'no active approval in the ledger' because no approval rows exist for it.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/gates.rs",
            "quote": "    let for_sha = state\n        .spec_approvals\n        .find_by_spec_sha(path, sha)\n        .await\n        .unwrap_or_default();\n    let has_active_approval = for_sha.iter().any(|a| a.is_active());",
            "detail": "The approved-SHA check is real (bullet 2), but its entire inputs are the per-SHA approval rows: the manifest is never read, so neither the manifest-existence check (bullet 1) nor the requires_approval: false exemption (bullet 3) can be evaluated here."
          },
          {
            "file": "crates/gyre-server/src/api/gates.rs",
            "quote": "            \"spec '{path}' at SHA '{sha}' has no active approval in the ledger\"",
            "detail": "The only failure branch: a requires_approval: false spec with no approval rows gets this rejection when require_approved_spec is on — the specced 'any SHA is accepted' exemption has no code path (grep for requires_approval across the merge/gate modules returns nothing; the flag is only evaluated inside spec_registry's resolver and specs.rs request fixtures)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                if policy.require_approved_spec {\n                    if let Err(reason) = crate::api::gates::verify_spec_ref(state, spec_ref).await {",
            "detail": "The pre-accept call site: the inspected chain (this block → verify_spec_ref → spec_approvals.find_by_spec_sha) contains no manifest lookup and no requires_approval branch, so two of the section's three checks are absent from the enforced path."
          }
        ]
      },
      {
        "id": "2236f6ecfe981117d57f7990",
        "decision": "gap",
        "requirement_quote": "The Accountability agent's patrol gains manifest-aware checks:\n- Spec files that exist but aren't in the manifest (orphans)\n- Manifest entries whose files don't exist (stale entries)\n- Specs with `requires_approval: true` that have been `pending` for more than one Ralph loop cycle\n- Specs with `auto_create_tasks: true` that were modified but have no corresponding task",
        "detail": "None of the four manifest-aware patrol checks exists as a patrol check. The two live patrols check other contracts: spec_lifecycle_patrol (spec-lifecycle.md's accountability integration, task-204) checks drift-review/implementation task ages and modified-specs-without-tasks — but its coverage check keys on the repo's SpecLifecycleConfig watched prefixes, not the manifest's auto_create_tasks, and it has no orphan, stale-entry, or pending-age check; spec_patrol inspects the spec link graph (stale links, orphaned supersessions, unresolved conflicts), not manifest coverage. Orphan detection (files under specs/ not in the manifest) exists only as push-time warn/reject in check_manifest_coverage, not as an accountability patrol finding; stale manifest entries (entry with no file at HEAD) produce only a sync-time warning log; and there is no pending-age signal at all — the ledger entry has no last_checked/pending-since timestamp from which 'pending for more than one Ralph loop cycle' could be evaluated, so that check cannot even be expressed against current state.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "//! - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle\n//! - `spec-implementation` tasks that have been in Backlog for more than N days\n//! - Specs that were modified but have no corresponding task (should never happen",
            "detail": "The live lifecycle patrol implements spec-lifecycle.md's three checks, not this section's four manifest-aware checks — no orphan check, no stale-entry check, no requires_approval pending-age check."
          },
          {
            "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
            "quote": "        if !config.is_watched(&path) {\n            continue;\n        }",
            "detail": "Even the closest check (modified-without-task) keys on the repo's SpecLifecycleConfig watched prefixes — the path-prefix mechanism this spec's Problem section rejects — rather than the manifest's auto_create_tasks: true, so a manifest entry with auto_create_tasks enabled but outside the config's watched prefixes is never examined."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                \"spec-registry: file under specs/ is not registered in manifest.yaml — \\",
            "detail": "Orphan detection exists only as a push-time warning (or policy-gated rejection) inside check_manifest_coverage/sync — a log line, not an accountability patrol finding escalated to the orchestrator."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "                    \"spec-registry: manifest entry has no corresponding file at HEAD\"",
            "detail": "Stale-entry detection (manifest entry with no file) is likewise only a sync-time warning log in sync_spec_ledger; no patrol surfaces it as a finding."
          },
          {
            "file": "crates/gyre-server/src/spec_patrol.rs",
            "quote": "//! - Orphaned supersessions: a spec is superseded but code still references it",
            "detail": "The other patrol inspects the spec link graph (stale/orphaned-supersession/unresolved-conflict links per spec-links.md), not manifest coverage — confirming the four manifest-aware checks in this section have no implementation anywhere (inspected both patrol modules and the patrol endpoints /api/v1/patrol/spec-links and /api/v1/patrol/spec-lifecycle)."
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
        "file": "specs/manifest.yaml",
        "sha256": "484e91e266a529a4f2fc258a7031cd0a8f5609e1cbb03c2d4f133c21e869358c"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "52041a21332b7acab66ba6776b75aae482078706d7681b68847f982b26ba7684"
      },
      {
        "file": "crates/gyre-adapters/migrations/2026-03-23-000010_persist_batch_a/up.sql",
        "sha256": "b1d935171753fd71d8b09654be18896651a5d6f812d3d17e95184c921434a42f"
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "sha256": "033b2dd34cdcd2c436b4391868c33fb8b1db37adc9c71d3d87faf1061bcea3b6"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "f6b1f8cd14524591bdca669b2c5ee44c355c3b09ec53d6accea4e5b0e1ceb94c"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "c4b7e05809c8be5cd04d754fbcd53619bb8832ac29c355222a5c031972ac1ef5"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/spec_ledger.rs",
        "sha256": "bbeafaae8740f05b205e5a223a11d141ec1452c3a461984108ff9c2ce1eeac85"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "b38ff786cd1af7a60305965df54869d498c1dc977be645d0046124466709422a"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/task.rs",
        "sha256": "90b3aa9a2f9bf0c83d07370a588fe5fbcc5ebe8b05f22c073e57babb363da8a7"
      },
      {
        "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
        "sha256": "2a3ab7dfcf3bb6995233c53a173e86b28782736408d30a6fc18acf483234afb3"
      },
      {
        "file": "web/src/components/SpecDashboard.svelte",
        "sha256": "0cc9f2c8ae3b96d7728b5f8c3cf35c7341f799a65c7830c7860883e65b6e31c2"
      },
      {
        "file": "specs/system/spec-lifecycle.md",
        "sha256": "a0a2eecc88a3d4368b12f115f7bf29ff9c185498a1d227a2b4cab94013ef9ec6"
      },
      {
        "file": "crates/gyre-server/src/api/gates.rs",
        "sha256": "e4fdcf7e32e2af83bb1b4c574d9d870591d2b9505a61602612e0de319eddc4fa"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "a7e02b2014b6acda7c6bdc11f463ac2c6707d23f5b47cf6167025f8cdd80dfea"
      },
      {
        "file": "crates/gyre-server/src/aibom_spec_registry.rs",
        "sha256": "486575d7841f354116a2736961d6bafe3ddd40fa44ff8b15268b99e59b2af071"
      },
      {
        "file": "crates/gyre-server/src/spec_patrol.rs",
        "sha256": "e79938cfd8706c1fc16a7e4619f9855c8196d61ccd9ea58d0cce875909919592"
      }
    ]
  },
  "plan": {
    "audit": "ec6c180f961b4aca8cc4a41025fd0be0a3e4e28ddd47de931fe8a17a1275b076",
    "tasks": [
      {
        "key": "approver-owner-default",
        "title": "Human-approver default [owner]: omitted human_approvers reserves approval to the spec owner",
        "covers": [
          "0c04e18483ce2f9bf15e2c1c"
        ],
        "required_behavior": "Manifest Schema row `approval.human_approvers` defaults to `[owner]`. Today `is_valid_human_approval` (crates/gyre-server/src/spec_registry.rs:336-348) treats an omitted/empty list as unrestricted — any active human approval at current_sha is valid — and the single-spec GET path's team expansion (crates/gyre-server/src/api/specs.rs:1164-1175) leaves an empty list empty, so a manifest entry that sets an approval mode but omits human_approvers lets any human approve where the spec reserves approval to the owner. Implement the spec default: the effective human-approver set for a manifest entry is `approval.human_approvers` when non-empty, otherwise `[owner]` (owner is a required field; the default applies to entries with no approval config at all, whose schema defaults are mode human_and_agent + human_approvers [owner]). Apply it (a) in `is_valid_human_approval` so `resolve_approval_status` (the §9 resolution that drives ledger approval_status, GET /api/v1/specs, /pending, /drifted) enforces it, and (b) at the team-expansion site in api/specs.rs so `team:{slug}` resolution composes with the owner default (an unresolvable team reference still approves nobody — that part is satisfied and must not regress). Do not alter agent-approver validity (attestation/stack_hash/persona), team-member resolution semantics, or the approval event schema — those sections are satisfied.",
        "acceptance": [
          "A manifest entry with approval mode human_only (and human_and_agent), owner user:alice, and NO human_approvers, resolved at current_sha with only an active human approval event from user:bob, yields approval_status Pending; the same event from user:alice yields Approved.",
          "GET /api/v1/specs and GET /api/v1/specs/{path} reflect that resolution: a bob-only approval leaves the spec pending, an alice approval approves it.",
          "Team refs still work alongside the default: an entry listing team:eng resolves Approved on a member's approval (user:carol), and an unresolvable team:nonexistent approves nobody whether or not the owner default also applies."
        ],
        "negative_probe": "Probe resolve_approval_status (unit) or the resolution through GET /api/v1/specs (integration): manifest entry owner user:alice, mode human_only, no human_approvers; seed an active human approval event for user:bob at current_sha; assert status == Pending. At the base this fails — `is_valid_human_approval` returns true for any human when the list is empty (`a.human_approvers.is_empty() || ...`), so bob's approval marks the spec Approved where the spec reserves approval to the owner. The twin case (alice's approval) must stay Approved, so a fix that simply inverts the predicate cannot pass both.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "manifest-supersedes-lifecycle-config",
        "title": "Manifest supersedes the [spec_lifecycle] config block for manifest repos (enabled flag and invalidation keying)",
        "covers": [
          "ff5387eb7f01a9b7b31c5126"
        ],
        "required_behavior": "spec-registry.md §Integration/Spec Lifecycle: \"The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section.\" Today `process_spec_lifecycle` (crates/gyre-server/src/git_http.rs:3390) returns early when the repo's SpecLifecycleConfig has enabled=false — a master switch the manifest cannot override — and its approval-invalidation block keys on `config.auto_invalidate_approvals` (git_http.rs:3501) rather than the manifest's per-entry/defaults `auto_invalidate_on_change`. For repos whose pushed HEAD has a specs/manifest.yaml, make the manifest authoritative: (1) the config enabled=false early-return must not suppress manifest-driven lifecycle task creation for manifest repos — the flag keeps its meaning only for manifest-less repos, whose watched-prefix fallback it governs today; (2) the hook's approval invalidation on M/D/R of a registered spec must key on the manifest entry's effective `auto_invalidate_on_change` (per-entry override, else `defaults:`), falling back to `config.auto_invalidate_approvals` only for manifest-less repos and unregistered files. Aspects the manifest does not express (e.g. dedup window) remain config-governed; manifest-less prefix-fallback behavior (should_create_lifecycle_task None-arm) must not regress. Harmonize the cross-reference: spec-lifecycle.md §Configuration must state that for manifest repos the manifest's defaults/entries supersede the config block for hook eligibility and invalidation (alignment via the normal spec-change process; do not weaken either spec's contract). Coordination boundary: the staged spec-lifecycle.md plan tasks (spec-lifecycle-sha-pin, spec-lifecycle-binding-refs, spec-lifecycle-dedup-update) own task content, SHA pinning, and dedup inside the same function — this task changes only precedence and invalidation keying and must not regress those behaviors.",
        "acceptance": [
          "A repo with specs/manifest.yaml (entry auto_create_tasks: true) and [spec_lifecycle] config enabled=false: pushing a modification to that spec still creates the drift-review lifecycle task; a manifest-less repo with enabled=false still creates nothing (fallback preserved).",
          "A repo whose manifest entry sets auto_invalidate_on_change: false: pushing new content updates the ledger SHA but does not invalidate/revoke existing approvals via the hook; with true (or default) it does. config.auto_invalidate_approvals no longer decides this for manifest repos.",
          "spec-lifecycle.md §Configuration documents the manifest supersession for manifest repos."
        ],
        "negative_probe": "E2E push probe on the mismatch case: repo whose manifest entry has auto_create_tasks: true and auto_invalidate_on_change: false, repo config [spec_lifecycle] enabled=true with auto_invalidate_approvals=true. Push a content change to the spec. At the base the hook revokes the approvals (config key wins the invalidation decision); the probe asserts the drift-review task exists AND the approvals survive. Run the twin with enabled=false: at the base no task is created at all (master switch); the probe asserts the task exists. A fix that only re-keys invalidation or only removes the master switch fails one of the two probes.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "manifest-self-registration",
        "title": "The manifest is a spec: specs/manifest.yaml triggers lifecycle hooks and requires approval",
        "covers": [
          "dc58139efeac521a97fffb3d"
        ],
        "required_behavior": "Manifest Rule 2: \"The manifest itself is a spec. Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval.\" Nothing currently treats the manifest as a spec: the repo's own manifest does not list itself (31 entries, no self-entry), `should_create_lifecycle_task` finds no manifest entry for it ('specs/manifest.yaml' normalizes to 'manifest.yaml', which no manifest declares), and the ledger has no row for it — manifest changes create no lifecycle task and require no approval. Deliver: (1) register specs/manifest.yaml as an entry in the repository's own specs/manifest.yaml (path `manifest.yaml`, title, owner, default requires_approval: true so the approval requirement flows from the existing machinery) and in the gyre-cli bootstrap starter manifest (crates/gyre-cli/src/bootstrap/starter-manifest.yaml) so bootstrapped repos get the same rule; (2) make the self-entry flow end-to-end through the generic machinery: ledger sync creates/updates a ledger row for manifest.yaml with its git blob SHA and resets approval_status to pending on content change (sync_spec_ledger already iterates manifest entries and resolves `specs/{path}` — confirm no special-case excludes the self-entry), the post-receive lifecycle hook creates a drift-review task for manifest.yaml modifications (manifest_entry_for_path normalization already matches a `manifest.yaml` entry), and the push-time coverage scan stays consistent (manifest.yaml is registered, so the existing exclusion produces no orphan warning); (3) any code that special-cases the manifest path in these paths must remain correct with the self-entry present. Depends on manifest-supersedes-lifecycle-config because rule 2 must hold regardless of the [spec_lifecycle] config block — the manifest must be authoritative for its own file's hook gating before self-registration guarantees that changes to it always trigger the hooks.",
        "acceptance": [
          "Pushing a modification to specs/manifest.yaml in a repo whose manifest lists itself creates a spec-drift-review lifecycle task whose spec_path points at the manifest, and the manifest's ledger row transitions approval_status to pending (old approvals stale on the new SHA).",
          "The repository's own specs/manifest.yaml contains a self-entry and GET /api/v1/specs lists the manifest as a registered spec with its current SHA and approval status.",
          "gyre bootstrap starter-kit repos register their manifest the same way (starter-manifest.yaml carries the self-entry)."
        ],
        "negative_probe": "E2E: repo with self-registered manifest; push a commit modifying only specs/manifest.yaml (e.g. adding an unrelated entry). At the base no lifecycle task is created (no manifest entry matches 'manifest.yaml', so the manifest-mode gate returns false) and no ledger row exists for the manifest; the probe asserts (a) a drift-review task with spec_path specs/manifest.yaml exists in task storage and (b) the ledger row for manifest.yaml is pending. Both assertions fail on current code — proving the hook and ledger actually treat the manifest as a spec rather than the test merely passing a structural check.",
        "dependencies": [
          "manifest-supersedes-lifecycle-config"
        ],
        "existing_task": null
      },
      {
        "key": "spec-api-read-completion",
        "title": "Spec API read path: linked_tasks under both spellings and per-spec open task counts in the generated index",
        "covers": [
          "16caff87fba84183d8dfcc06",
          "9df024e265cff9471afd92d4"
        ],
        "required_behavior": "Two read-path defects in the spec API surface. (1) GET /api/v1/specs/{path} (crates/gyre-server/src/api/specs.rs:385-393) populates linked_tasks via TaskRepository::list_by_spec_path with the manifest-relative spelling only ('system/x.md'), while the post-receive lifecycle hook records tasks with the git-rooted spelling ('specs/system/x.md', git_http.rs:3630) and the repository matches spec_path exactly — so hook-created drift-review/implementation tasks are invisible in the single-spec response; the spec-lifecycle patrol already works around exactly this dual-spelling problem by querying both forms (spec_lifecycle_patrol.rs rooted_path/alt). Fix the single-spec endpoint — and the history endpoint's linked_tasks at specs.rs:2191, which shares the defect — to resolve tasks under both spellings (the patrol's established approach), so linked_tasks contains hook-created tasks. (2) GET /api/v1/specs/index (specs.rs:312-342) generates the markdown index with only Spec | Status | SHA columns; the spec requires \"Open task count per spec\". Query the task repository using the same both-spelling resolution and add a per-spec open-task count (open = not Done/Cancelled) to each row. Keep the satisfied parts unchanged: grouping by directory, per-spec approval status, links to spec files.",
        "acceptance": [
          "After a push that triggers the lifecycle hook for specs/system/x.md, GET /api/v1/specs/system/x.md returns the hook-created task's id in linked_tasks (and GET history likewise).",
          "GET /api/v1/specs/index markdown carries an open-task-count column whose value equals the number of open tasks for each spec — N for a spec with N open hook-created tasks, 0 for a spec with none."
        ],
        "negative_probe": "Seed storage with a task whose spec_path is exactly 'specs/system/x.md' (the spelling the hook writes) plus a ledger entry 'system/x.md'. GET /api/v1/specs/system/x.md: at the base linked_tasks is empty (exact-match query misses the rooted spelling) — the probe asserts the task id is present. Same seed against the index: at the base the document has no task-count column at all — the probe asserts the row for system/x.md shows 1 and a taskless spec's row shows 0. A fix that only widens one endpoint or hardcodes counts cannot satisfy both probes.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "approval-ledger-unification",
        "title": "One table, one source of truth: unify spec_approvals and spec_approval_events into a single approval store",
        "covers": [
          "4f7e9a6ec00744d40ce24f65"
        ],
        "required_behavior": "\"The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.\" Today approvals live in two durable stores with separate ports — spec_approvals (SpecApprovalRepository; the per-(path,sha,approver) state machine the merge gate's verify_spec_ref enforces, migration 000058 shape) and spec_approval_events (SpecApprovalEventRepository; the §9 resolver's history with approver_type/persona/attestation_level/stack_hash) — and approve_spec writes both (specs.rs:652 and :687), while constraint_check.rs:3427 and aibom_spec_registry.rs:613 write only the event store, so the §9 resolution and the merge-gate check read different tables and can diverge. Unify into a single durable store that is the one source of truth for spec approvals: one consolidated port (replacing the SpecApprovalRepository/SpecApprovalEventRepository duality), one SQL table carrying the union of fields (approver_type, persona, attestation_level, stack_hash, signature, approved/revoked/rejected timestamps, reasons, approver identity), a migration at the next unused sequence number (000071 at planning time; re-check with `ls crates/gyre-adapters/migrations/ | sort | tail` at implementation time) written in portable SQL that runs on BOTH SQLite and PostgreSQL (check-migration-sql-portability.sh), data migration of existing rows from both tables, and adapters for SQLite, PostgreSQL, and mem with every port failure constraint enforced in each (check-mem-port-contracts.sh), wired via the store! pattern. Rework every reader onto the unified store: the §9 resolution path (resolve_approval_status callers), the merge-gate checks (verify_spec_ref, verify_signed_spec_ref, list_spec_approvals), the approve/reject handlers, users.rs judgments (list_by_approver, preserving the HSI §12 canonical-user-id attribution key), aibom.rs, constraint_check.rs, aibom_spec_registry.rs. All §9 validity semantics — including the owner-default human-approver set from this plan's approver-owner-default task (dependency: the unified store must carry the corrected predicate, not the current any-human default) — must be preserved and evaluated against the unified rows. Boundaries: only adapt the approval readers/writers; AIBOM artifact-generation behavior owned by the staged supply-chain plan and meta-spec kind-conditional approval modes owned by the meta-spec-reconciliation plan are out of scope.",
        "acceptance": [
          "One port type and one SQL table back spec approvals: AppState exposes a single approval store, no production code path reads or writes spec approval state through a second store, and the old dual ports are gone or reduced to the unified one.",
          "Approving via POST /api/v1/specs/:path/approve is a single write that both consumers see: GET /api/v1/specs shows the resolved status AND verify_spec_ref accepts the (path, sha) — revoke flips both. No code path can make the resolver and the merge gate disagree.",
          "Existing rows from both tables survive migration on SQLite and PostgreSQL and remain queryable (approvals, judgments, history).",
          "The existing approval-semantics test suite (mode resolution, attestation/stack_hash validity, revoke/reject, signed approvals, judgments identity key) passes against the unified store, plus the owner-default cases from approver-owner-default."
        ],
        "negative_probe": "Divergence probe the two-store design permits and the unified design cannot: record an approval event directly through the event store exactly the way constraint_check.rs does (same repository call, same fields), leaving the per-SHA ledger store empty. On the base, the §9 resolver counts the event and GET /api/v1/specs shows the spec Approved, while verify_spec_ref (reading the other table) rejects with 'no active approval in the ledger' — the same approval is simultaneously valid and invalid. After unification both consumers read the same rows, so the probe's paired assertions (resolver says Approved AND merge gate accepts) must agree; any residual second store, unsynced writer, or one-sided migration makes the pair fail.",
        "dependencies": [
          "approver-owner-default"
        ],
        "existing_task": null
      },
      {
        "key": "merge-gate-manifest-policy",
        "title": "Merge-time spec-to-code binding consults the manifest: registered-path check and the requires_approval:false any-SHA exemption",
        "covers": [
          "a81aa30dd4413677c04273d8"
        ],
        "required_behavior": "Spec-to-Code Binding pre-accept validation: (1) the MR's spec_ref path must exist in the manifest; (2) if the referenced spec has `requires_approval: false`, any SHA is accepted. Today verify_spec_ref (crates/gyre-server/src/api/gates.rs:819) only queries approval rows and never reads specs/manifest.yaml: an unregistered path passes whenever an approval row exists, and an MR referencing a requires_approval:false spec — which per §9/§17 is Approved with no approval events at all — fails the require_approved_spec gate with 'no active approval in the ledger'. Extend the merge-time check (merge_processor.rs spec-policy block → verify_spec_ref) with the repo context it needs (the repo is at hand at the merge site; read specs/manifest.yaml at the repo's default-branch HEAD via the existing read_manifest machinery, using the repo's real scoped identity — no fabricated scope): if the spec_ref path has no manifest entry, reject naming the unregistered path; if the entry's effective requires_approval (per-entry override, else defaults) is false, accept regardless of approval rows — any SHA; otherwise the existing per-SHA approval check applies, evaluated against the unified ledger (dependency: building the check on the dual store being replaced would be discarded work). require_spec_ref and require_signed_approval policy semantics are preserved unchanged, and the signed-approval check keeps its own contract.",
        "acceptance": [
          "With repo policy require_approved_spec=true and a manifest entry declaring requires_approval: false, an MR whose spec_ref pins that spec at a SHA with zero approval rows merges (today it is blocked with 'no active approval in the ledger').",
          "An MR whose spec_ref path is not in the repo's manifest is blocked at merge with a reason naming the unregistered path, even when an active approval row for that path exists in the ledger.",
          "Registered specs with requires_approval:true keep today's behavior: an unapproved SHA blocks the merge, the approved SHA merges."
        ],
        "negative_probe": "Two-sided merge-queue probe (run_once on seeded state): (a) manifest entry system/exempt.md with requires_approval: false, no approval rows anywhere, MR spec_ref 'specs/system/exempt.md@<sha>', policy require_approved_spec=true — at the base the queue entry fails with 'spec '{path}' at SHA '{sha}' has no active approval in the ledger'; after the fix it must merge. (b) MR spec_ref 'system/ghost.md@<sha>' with a manually inserted ACTIVE approval row but no manifest entry — at the base it merges; after the fix it must be blocked naming the unregistered path. The two sides fail in opposite directions on current code, so a change that merely toggles the approval query (or hardcodes an exemption) cannot pass both.",
        "dependencies": [
          "approval-ledger-unification"
        ],
        "existing_task": null
      },
      {
        "key": "accountability-manifest-patrol",
        "title": "Accountability patrol manifest-aware checks: orphans, stale entries, pending-beyond-one-cycle, modified-without-task keyed on the manifest",
        "covers": [
          "2236f6ecfe981117d57f7990"
        ],
        "required_behavior": "The Accountability agent's patrol gains the four manifest-aware checks from §Accountability Agent: (1) orphan spec files — files under specs/ at repo HEAD not registered in the manifest (reuse the file-scan class of the push-time coverage check in spec_registry.rs; once the manifest itself is registered it must not be flagged); (2) stale manifest entries — manifest entries whose file does not exist at HEAD (today only a sync-time warning log, never a patrol finding); (3) specs with effective requires_approval: true that have been pending for more than one Ralph loop cycle — this needs a real pending-since signal: durably record when a ledger entry's approval_status transitions to pending (e.g. a pending_since timestamp maintained by the sync/approve/reject paths, migration at the next unused sequence number at implementation time, portable across SQLite and PostgreSQL — or equivalent durable state; NOT updated_at, which churns on unrelated writes), and compare it against the patrol's cycle bound (the same one-Ralph-loop-cycle bound spec_lifecycle_patrol already uses — DEFAULT_DRIFT_REVIEW_MAX_AGE_SECS, overridable per request); (4) specs with effective auto_create_tasks: true that were modified but have no corresponding task — rekey the existing modified-without-task defense-in-depth check in spec_lifecycle_patrol.rs from the config's watched prefixes to the manifest's auto_create_tasks, querying both spec_path spellings as the patrol already does. Findings surface through the patrol response and escalate to the workspace orchestrator via MessageKind::Escalation exactly like the existing checks (only with a resolvable workspace — no fabricated scope, per the standing constraint). Boundaries: this extends the spec-lifecycle patrol (spec_lifecycle_patrol.rs); it must not touch spec_patrol.rs (the spec-links graph patrol owned by task-292) or the push-time coverage rejection (task-017).",
        "acceptance": [
          "A repo with an unregistered specs/orphan.md on disk and a manifest entry pointing at a missing specs/ghost.md: one patrol run returns both findings and emits escalations to the owning workspace orchestrator's event stream.",
          "A requires_approval:true spec pending longer than the cycle bound produces a pending-age finding; the same spec pending less than the bound does not — the age comparison is evaluated against the recorded pending-since, not flagged unconditionally.",
          "A spec with auto_create_tasks:true whose SHA changed on a push but whose lifecycle task was deleted produces the modified-without-task finding even when the repo's [spec_lifecycle] config watches no prefixes (manifest-keyed, not config-keyed)."
        ],
        "negative_probe": "Patrol-run probe with three seeded states: (a) specs/orphan.md on disk but absent from the manifest; (b) manifest entry system/stale.md with no file at HEAD; (c) a requires_approval:true ledger entry pending with a fresh pending-since (minutes old, well under the bound). At the base the patrol returns none of these — spec_lifecycle_patrol has no orphan, stale-entry, or pending-age check at all, and its coverage check is config-prefix-keyed. The probe asserts findings (a) and (b) are returned with real escalation rows on the orchestrator's stream, and that (c) is NOT returned; a check that flags (c) unconditionally (no real timestamp), returns findings without durable escalations, or only mirrors the push-time warning log fails the probe.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "approver-owner-default": "task-333",
    "manifest-supersedes-lifecycle-config": "task-334",
    "manifest-self-registration": "task-335",
    "spec-api-read-completion": "task-336",
    "approval-ledger-unification": "task-337",
    "merge-gate-manifest-policy": "task-338",
    "accountability-manifest-patrol": "task-339",
    "_publication": "task-340",
    "_base": "c42bfe033949ef04f8c4c41681126f116ab6a9de"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
