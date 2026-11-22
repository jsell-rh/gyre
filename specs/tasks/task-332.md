---
title: "Publish reviewed spec backlog for specs/system/meta-spec-reconciliation.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/meta-spec-reconciliation.md",
  "plan_id": "c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071",
  "audit": {
    "base": "50575cf65e6491279049052901a65c68a4d2993d",
    "requirements": "b288723d2f73f66760ba2975fdb9917f28a872c6534c6035d7eebb29f97788d9",
    "sections": [
      {
        "id": "67a7f81ca6df193b69b92750",
        "decision": "context",
        "requirement_quote": "> **Status: Draft.** This spec defines how Gyre safely iterates on the rules that govern agent behavior (personas, design principles, coding standards) without destructively invalidating downstream code.",
        "detail": "Document preamble and status marker. Scope framing only; no implementable requirement. The normative content lives in Design sections 1-11, each audited under its own heading.",
        "evidence": []
      },
      {
        "id": "e251b247fe28d8a2b734a6be",
        "decision": "context",
        "requirement_quote": "Without solving this, Gyre is a single-use system. The SDLC is only as good as its ability to evolve its own norms.",
        "detail": "Problem statement / motivation. Purely descriptive of the pre-design pain; no implementable requirement. The stated deficiencies (no tracking, no versioning, no impact analysis) are given operational requirements in Design sections audited individually.",
        "evidence": []
      },
      {
        "id": "388a7b61d35f6c2b765ce682",
        "decision": "context",
        "requirement_quote": "Therefore: **meta-spec iteration is speculative re-implementation.** Fork the world, re-run affected work under new rules, compare the output, decide whether to adopt it.",
        "detail": "Design rationale: the effect of a prompt change cannot be statically analyzed, so iteration must run agents. This justifies preview mode (§5) and reconciliation (§6), which are audited under their own headings; the insight itself carries no separate implementable requirement.",
        "evidence": []
      },
      {
        "id": "391ba92662ad8e3ca728fb73",
        "decision": "context",
        "requirement_quote": "| **Meta-spec set** | The bound collection of meta-specs active in a given scope (workspace or repo). Pinned to specific versions. |",
        "detail": "Terminology table defining Spec, Meta-spec, Meta-spec set, Provenance, Drift, Reconciliation, Blast radius, Preview. No standalone requirement; each term is operationalized by the Design sections audited individually (notably the repo-scope element of 'Meta-spec set', assessed under §2).",
        "evidence": []
      },
      {
        "id": "9f7a01774050988ef18286d8",
        "decision": "context",
        "requirement_quote": "## Design",
        "detail": "Container heading only — the section has no body text between '## Design' and '### 1.'; every requirement it introduces lives in subsections 1-11, each audited under its own heading ID.",
        "evidence": []
      },
      {
        "id": "c2ad988fa981b60eae143fd1",
        "decision": "gap",
        "requirement_quote": "Meta-specs get the same SHA-pinned versioning, approval flow, and link graph that regular specs already have. They appear in the spec registry alongside implementation specs.",
        "detail": "Requires meta-specs to be first-class versioned registry artifacts: SHA-pinned versioning, approval flow, link graph, registry presence alongside implementation specs, manifest declaration (kind/scope/auto_create_tasks/auto_reconcile), and per-kind approval modes (meta:persona/meta:principle/meta:process human_only; meta:standard human_and_agent). Registry semantics are amended by agent-runtime.md §2 ('DB-backed, not repo-backed': content-addressable SHA-256 hashes computed from content, full version history in a meta_spec_versions table, all edits stored in the database), which explicitly supersedes this section's git-backed model, and its amendments-table row repeats 'Reconciliation spec defers to this for registry semantics'. Accordingly, the manifest-driven registration surface — the yaml manifest declaring meta-specs with kind/scope/auto_create_tasks/auto_reconcile, and meta-specs 'appearing in the spec registry alongside implementation specs' — is classified as superseded by the DB-backed replacement, not a live gap: the existing meta_specs/meta_spec_versions registry IS the amended model (the four kinds, u32 version + SHA-256 content_hash, immutable version history, full CRUD/approval surface at /api/v1/meta-specs), the manifest's SpecEntry.kind field is vestigial declaration metadata under it (no auto_reconcile flag exists anywhere, and nothing bridges manifest entries into the meta-spec registry — correctly so under 'all edits are stored in the database'), and active task-302 retires the remaining set-based consumers in favor of the required-flags + spec-level-bindings model. What remains missing against the amended model: (1) per-kind approval modes — one approval_status lifecycle applies verbatim to every kind; the kind field is never consulted when an approval is applied (ms.kind is read only for event payloads and task titles), so human_only for meta:persona/meta:principle/meta:process is not enforced by kind — it is only incidentally true because the write gate admits tenant Admins and workspace Owner/Admin members and no agent principal can pass it, which simultaneously makes the specced human_and_agent dual mode for meta:standard unreachable: no kind-conditional agent-admission widening exists anywhere in the approval path; (2) the Repo scope value — agent-runtime.md preserves PersonaScope Repo ('Repo-scoped personas continue to work as nearest-scope-wins overrides ... but cannot be marked required'), but MetaSpecScope is Global/Tenant/Workspace only, so repo-scoped meta-specs have no representation at all; (3) link-graph participation — spec-links operate on the spec ledger only; no link machinery covers meta-specs, and the amendment does not address §1's link-graph clause.",
        "evidence": [
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "This supersedes the git-backed meta-spec model from `meta-spec-reconciliation.md` §1 — the reconciliation spec's registry semantics are replaced by this DB-backed model",
            "detail": "The canonical amendment (agent-runtime.md §2, 'DB-backed, not repo-backed' paragraph): §1's git/manifest-backed registry semantics are explicitly replaced by the DB-backed model, so the manifest-registration surface, the auto_reconcile manifest flag, and 'appear in the spec registry alongside implementation specs' are superseded requirements rather than open gaps — the existing DB registry is the amended model."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "Reconciliation spec defers to this for registry semantics",
            "detail": "The amendments-table row for meta-spec-reconciliation.md in agent-runtime.md confirms the deferral of registry semantics, including the replacement of the set-PUT surface with required flags + spec-level bindings that active task-302 implements."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "if let Some(status_str) = &req.approval_status {",
            "detail": "update_meta_spec_registry applies the caller-supplied approval_status identically for every kind: the block parses the status, stamps approved_by/approved_at/approved_content_hash and assigns ms.approval_status with no kind-conditional branch anywhere in the function (inspected the full update path). meta:persona human_only and meta:standard human_and_agent approval modes from the §1 table are therefore not differentiated."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "only tenant Admin may write Global-scoped meta-specs",
            "detail": "authorize_registry_write gates every registry write (including approval_status PUTs) on tenant Admin role or workspace Owner/Admin membership. Agent principals (UserRole::Agent, no workspace membership, no user_id) receive 403 on every arm, so the specced human_and_agent mode for meta:standard is unreachable: no agent principal can ever record an approval, and nothing kind-conditional widens the approval step for meta:standard."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "invalid scope '{s}'; must be one of: Global, Tenant, Workspace",
            "detail": "parse_scope admits only Global/Tenant/Workspace. agent-runtime.md explicitly preserves PersonaScope Repo for meta-specs ('Repo-scoped personas continue to work as nearest-scope-wins overrides ... but cannot be marked required'), so the Repo scope value is a live requirement under the amended model — and it has no representation in the meta-spec registry."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "kind: entry.kind.clone(),",
            "detail": "The manifest kind field's sole consumption in the spec-registry registration path is being copied onto the ledger entry — vestigial declaration metadata under the DB-backed model: nothing derives approval mode from it or bridges manifest-declared meta-specs into the meta_specs registry that prompt assembly, sets, and reconciliation read from (consistent with the amendment's 'all edits are stored in the database'; no auto_reconcile flag exists anywhere in the codebase)."
          }
        ]
      },
      {
        "id": "815f40a82746fafd22856e09",
        "decision": "gap",
        "requirement_quote": "When an agent is spawned in a workspace, the MCP server injects the resolved meta-spec set into the agent's system prompt (extending the existing protocol injection from platform-model.md Section 8).",
        "detail": "Requires a workspace-level pinned meta-spec set with nearest-scope-wins resolution (repo overrides workspace overrides tenant), SHA-pinned entries whose explicit update triggers reconciliation, and spawn-time injection of the resolved set into the agent system prompt. What exists and is real: MetaSpecSet/TenantMetaSpecSet storage with the specced shape (personas map, principles/standards/process lists of path@sha entries) via port-backed repositories; GET/PUT /api/v1/workspaces/:id/meta-spec-set (Admin-only PUT) and /api/v1/tenants/:id/meta-specs/set; tenant-to-workspace inheritance with workspace-pin-wins (merge_meta_spec_sets / resolve_effective_meta_spec_set); the set-update diff is the explicit reconciliation trigger. The injection clause is amended rather than violated: agent-runtime.md's amendments-table row for meta-spec-reconciliation.md replaces the set as the injection surface ('The PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API'), and spawn-time prompt assembly implements exactly that amended model — required Global/Tenant/Workspace meta-specs (bands 1-2) plus spec-level bindings at pinned versions (band 3) — so the set functioning as an accounting/drift construct (set SHA for provenance attestations, §9 drift checks, §10 sweep, blast-radius resolution) rather than the injection source is the amended design, not a gap. The remaining live gap is the repo tier of the scope cascade: 'Repo-level meta-spec set overrides workspace' is unimplemented. Only workspace and tenant set repositories exist (no repo-level set port, handler, or resolution step — resolve_effective_meta_spec_set merges tenant and workspace sets only), and meta-spec scope has no Repo value, while agent-runtime.md explicitly preserves Repo-scoped personas as nearest-scope-wins overrides that cannot be marked required. Under the amended semantics the repo tier means repo-scoped entries acting as name-resolution overrides that never remove a required workspace/tenant injection (same-name collisions concatenate; dedup only by meta_spec_id) and are never requiredable — none of that exists.",
        "evidence": [
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "The `PUT /api/v1/workspaces/{id}/meta-spec-set` endpoint is replaced by updating `required` flags and spec-level bindings via the meta-spec API",
            "detail": "The canonical amendment (agent-runtime.md amendments-table row for meta-spec-reconciliation.md): the set is replaced as the injection/binding surface by required flags + spec-level bindings, so prompt assembly drawing from required flags and spec-level bindings instead of the workspace set is the amended injection model, not a §2 violation."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "Repo-scoped personas continue to work as nearest-scope-wins overrides per `platform-model.md` §2, but cannot be marked `required`",
            "detail": "The amended repo-tier semantics: repo-scoped entries survive as nearest-scope-wins name-resolution overrides, never required, never removing a required injection (same-name collisions concatenate). This is the live requirement the missing repo tier would carry."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "// Band 1: required tenant-level meta-specs. Global entries apply to",
            "detail": "assemble_prompt_set is the only prompt-assembly path feeding agent system prompts; its band structure (required Global/Tenant meta-specs, required Workspace meta-specs, then spec-level pinned bindings) resolves from state.meta_specs and state.meta_spec_bindings — the amended injection model. The workspace meta_spec_sets store is never consulted for injection (the set persists as the provenance/drift/reconciliation-trigger construct)."
          },
          {
            "file": "crates/gyre-ports/src/meta_spec_set.rs",
            "quote": "/// Repository for workspace meta-spec sets (M34 Slice 5).",
            "detail": "The meta-spec-set persistence layer defines exactly two repositories — MetaSpecSetRepository (keyed by workspace id) and TenantMetaSpecSetRepository (keyed by tenant id). No repo-level set repository, handler, or port exists anywhere in the codebase (inspected the api/mod.rs route table and the reconciliation.rs scope-resolution helpers), so the repo-overrides-workspace rule of the scope cascade is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let mut effective = merge_meta_spec_sets(&tenant_set, &ws_set);",
            "detail": "resolve_effective_meta_spec_set merges tenant defaults with the workspace's own set only — the cascade has exactly two tiers. There is no repo-tier resolution step, so a repo-level override of a workspace or tenant pin cannot be expressed, and the §2 scope-resolution ladder stops at workspace."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let changed_paths = crate::reconciliation::diff_meta_spec_set(&old_set, &set);",
            "detail": "Real, working part: PUT /workspaces/:id/meta-spec-set loads the old set, upserts the new pinned path@sha entries, and the SHA diff is the explicit §6 reconciliation trigger — version pinning as an explicit action is genuinely implemented (task-302's registry-model migration preserves this trigger semantics via required-flag changes and meta-spec approval)."
          }
        ]
      },
      {
        "id": "fc0d2e519b45282ea3eb317d",
        "decision": "satisfied",
        "requirement_quote": "This creates the traceable link: \"this code was produced under persona v3 + architecture v7 + coding-standards v2.\" Without this, blast radius computation is impossible.",
        "detail": "Requires the commit provenance record to capture the active meta-spec set at commit time. Implemented: (a) the authorization-provenance chain attestation's SignedInput content carries meta_spec_set_sha, populated from the workspace's effective set (compute_meta_spec_set_sha, SHA-256 of the canonical set JSON) at spec-approval input creation and in the agent context; (b) at merge time the MergeAttestation records meta_specs_used — the itemized list of every meta-spec the authoring agent ran under (id, kind, content_hash, version, required, scope) — loaded from the prompt-set record persisted at spawn time. The itemized snapshot is keyed by kind per entry (persona/principle/standard/process), which carries the same information as the specced MetaSpecSnapshot struct (single persona string + per-kind lists) in a flat form. The traceable link 'code produced under meta-spec X vN' is real and queryable (the §9 drift check and §10 sweep both read the recorded set SHA back out of provenance).",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub meta_spec_set_sha: String,",
            "detail": "InputContent of the signed work-authorization attestation carries the active meta-spec set SHA — the provenance-chain half of the extension."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::compute_meta_spec_set_sha(&state, &gyre_common::Id::new(&workspace_id))",
            "detail": "Spec approval (api/specs.rs ~759-772) populates the attestation input's meta_spec_set_sha from the workspace's current effective set — the set under which the authorized work runs is recorded in real provenance, not hardcoded."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "let meta_specs_used = match updated_mr.author_agent_id.as_ref() {",
            "detail": "The real merge path (merge_processor.rs ~2812-2836) loads the authoring agent's spawn-time prompt-set record and puts its meta_specs_used (itemized id/kind/content_hash/version/scope per injected meta-spec) onto the signed MergeAttestation — the itemized snapshot is populated in production, with a test asserting the attestation signature verifies over the full payload including meta_specs_used (merge_processor.rs ~10316-10400)."
          }
        ]
      },
      {
        "id": "3662e9dd04c597d5db7d61f0",
        "decision": "gap",
        "requirement_quote": "This is a database query over provenance records + workspace meta-spec set bindings. The blast radius is computed, not guessed.",
        "detail": "Requires blast radius computed from provenance records plus set bindings, with per-repo spec/commit counts under the superseded version and version-diff reasoning (the example skips a workspace 'bound to v2, not affected'). What exists: GET /api/v1/meta-specs/:path/blast-radius does a real binding scan — it lists all workspaces, resolves each one's effective set (tenant defaults merged with workspace overrides), checks whether any personas/principles/standards/process entry references the path, labels the reason workspace_binding vs tenant_inheritance (the §8 amendment), and lists the repos of affected workspaces. What is missing: the provenance half — the handler (inspected in full, meta_specs.rs 650-745) never queries attestation/provenance stores, so there are no per-repo spec counts, no commit counts under the superseded version, and no version discrimination at all: a workspace pinned to any version of the path counts as affected (the 'bound to v2, skipped' case is not skipped), and the response carries only affected repo/workspace ids with a reason string — no totals over provenance. The binding half is real; the 'database query over provenance records' half is absent.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let references_spec = effective.personas.values().any(|e| e.path == spec_path)",
            "detail": "The entire affected-workspace test is a path-membership check on the effective set's pinned entries (|| principles/standards/process any()). No comparison of the pinned sha against the changed version, no provenance lookup — a workspace pinned to an old version is reported affected exactly like one pinned to the new version."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let workspaces = state.workspaces.list().await.unwrap_or_default();",
            "detail": "Handler data sources, inspected across the full body (meta_specs.rs 650-745): workspaces.list, meta_spec_sets.get, tenant_meta_spec_sets.get, repos.list_by_workspace. No chain_attestations / attestation_store / spec ledger provenance query appears anywhere in the handler, so the spec/commit counts and 'commits under v3' figures of the §4 output shape cannot be produced by this endpoint."
          }
        ]
      },
      {
        "id": "c7a4da9ae6a8e82c55fa5db9",
        "decision": "satisfied",
        "requirement_quote": "**Preview mode strips all ceremony from the Ralph loop.** Same agent, same spec, same repo -- but no gates, no MR, no merge queue, no provenance recording.",
        "detail": "Preview mode is a real agent-run path, not an estimate. A preview spawns a genuine agent (Active agent row, real git worktree, short-lived preview JWT) pointed at a real spec in a real repo on a throwaway preview/{preview_id}/{slug} branch; the draft meta-spec reaches the agent as its governing context. Ceremony stripping is enforced in production: the MCP dispatch refuses task/MR/provenance mutations and task-creating tools for preview agents, REST task/MR surfaces refuse preview-agent mutations, agents never go Idle (every monitor exit routes through finish_preview_agent which revokes the token, removes the worktree, releases the budget slot and stops the row), no gates run (no MR is ever created, so no gate pipeline), and nothing reaches refs/agents/* or refs/tasks/*.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "preview runs skip task/MR/provenance ceremony",
            "detail": "MCP tool dispatch returns PERMISSION_DENIED for task/MR/provenance-mutating and task-creating tools when the caller is a preview agent (is_preview_agent discriminator from the preview_agents kv index) — the skip-ceremony rule is enforced at the tool boundary, not documented-only."
          },
          {
            "file": "docker/gyre-agent/agent-runner.mjs",
            "quote": "You are a Gyre autonomous agent running a THROWAWAY preview of a draft meta-spec. Your configuration:",
            "detail": "The preview agent's actual runtime prompt: the draft meta-spec (GYRE_META_SPEC_DRAFT_KIND/CONTENT) is the governing how-to-build context over the target spec (GYRE_TARGET_SPEC_PATH), the agent is told the output is never merged, and preview agents get no assembled set — the draft replaces it."
          }
        ]
      },
      {
        "id": "0bb6a54c4be108e23a2d32e6",
        "decision": "satisfied",
        "requirement_quote": "The draft doesn't need to be committed or approved. It's ephemeral -- the human is editing inline, hitting preview, seeing output. Multiple targets run in parallel.",
        "detail": "The specced endpoint exists with the specced contract: POST /api/v1/meta-specs/preview takes {draft: {kind, content}, targets: [{repo_id, spec_path}]}, validates every target up front (repo exists, caller passes repo ABAC + tenant containment, spec_path readable on the default branch) before creating anything, provisions one agent + branch + worktree per target in one run (parallel targets), and responds 202 with {preview_id, agents: [{agent_id, repo_id, spec_path, branch}]} where branch is preview/{preview_id}/{slug}. A workspace-scoped twin route adds containment. The draft is never committed to the registry — it is passed out-of-band to the agent process.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/meta-specs/preview\",",
            "detail": "Global route registration for POST /api/v1/meta-specs/preview (post_meta_spec_preview), plus GET/DELETE /api/v1/meta-specs/preview/:preview_id on the adjacent route — the specced endpoint surface exists."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "Json(PreviewResponse { preview_id, agents }),",
            "detail": "spawn_preview_run returns StatusCode::ACCEPTED with the spec-shaped response: preview_id plus one agents entry (agent_id, repo_id, spec_path, branch) per resolved target; provisioning is per-target in a loop with rollback-on-failure, so multiple targets run as parallel agents of one preview run."
          }
        ]
      },
      {
        "id": "dcc59e46038859aa4f262c22",
        "decision": "satisfied",
        "requirement_quote": "| Budget accounting | Separate preview budget (configurable, defaults to workspace budget) |",
        "detail": "The skip table's enforceable rows are implemented: spec approval check skipped (targets are validated to exist on the default branch — the run tests the meta-spec, not spec approval); quality gates / MR creation / merge queue / provenance recording skipped (MCP and REST refuse those mutations for preview agents and no MR/gate path exists for preview branches); budget accounting uses a separate configurable preview budget (GYRE_PREVIEW_BUDGET_MAX_CONCURRENT caps concurrent preview agents; when unset, the target workspaces' own spawn budgets govern — the specced 'defaults to workspace budget'); token revocation is normal with short-lived preview JWTs (GYRE_PREVIEW_JWT_TTL_SECS, default 1800s) revoked at run end. One nuance: the target spec's ledger approval status is not verified (only existence on the default branch) — the table's 'the spec is already approved' is assumed of the human's target choice rather than checked.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "if let Some(max) = state.preview_budget_max_concurrent {",
            "detail": "Preview budget cap: when GYRE_PREVIEW_BUDGET_MAX_CONCURRENT is set, the POST fails closed (429) when running preview agents + requested would exceed the cap — a separate preview budget distinct from feature work."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "// Unset: the target workspaces' own budgets govern (server-config.md).",
            "detail": "Budget default: with no separate preview budget configured, the target workspaces' spawn budgets are reserved up front (check_spawn_budget_for per workspace, grouped for multi-target POSTs) — the specced 'defaults to workspace budget'."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "pub fn preview_ttl_secs_from_env() -> u64 {",
            "detail": "Adjacent preview_jwt_ttl_secs_from_env (same file, line 766) configures the short-lived preview token TTL — preview agent tokens are short-lived per the skip table's token row."
          }
        ]
      },
      {
        "id": "1db0da3e4966c73d1f742732",
        "decision": "satisfied",
        "requirement_quote": "Steps 2-6 are the tight loop. Each iteration is one agent run -- minutes, not hours.",
        "detail": "The iteration cycle is realized end-to-end in the UI: the human edits the draft inline (locked editor diffing against the published prompt), selects 1-3 real specs from repos they can access (target selector keyed by repo_id+spec_path pairs), hits Preview (POST spawns agents and the panel polls the run's per-agent status and diffs), reviews the diff of existing code vs preview output (diff viewer updates mid-run, multiple targets as tabs), adjusts, previews again (each run resets prior panel state and spawns fresh agents), and finally Publishes — which PUTs the draft content to the registry (resetting approval to Pending) and then goes through the approval flow that triggers reconciliation fan-out.",
        "evidence": [
          {
            "file": "web/src/components/MetaSpecs.svelte",
            "quote": "const res = await api.previewMetaSpec({ draft, targets });",
            "detail": "startPreview posts the inline draft plus the selected targets and tracks the returned preview_id/agents, polling status; repeated Previews spawn new agents and new output per iteration — the tight loop steps 2-6."
          },
          {
            "file": "web/src/components/MetaSpecs.svelte",
            "quote": "await api.publishPersona(workspaceId, selectedMsId, { content: selectedMsContent });",
            "detail": "Step 8 'Publish': the editor commits the draft via the registry update (content PUT → approval resets to Pending → approval flow), after which the server-side approval trigger fans out reconciliation — publish is wired to the real approval + reconciliation chain, not a UI-only state change."
          }
        ]
      },
      {
        "id": "35803d62e1c49a07e9c37e13",
        "decision": "satisfied",
        "requirement_quote": "The targets must be real specs in real repos.",
        "detail": "The enforceable core of this rationale section is implemented at the target-resolution boundary: every preview target must name a real repo (looked up in the repos store, passing repo ABAC and tenant containment) and a real spec — the spec file must be readable on the repo's default branch via git_ops.read_file, else the POST is rejected 400 before anything is provisioned. Synthetic golden-spec fixtures are not part of the preview path (the preview prompt instructs the agent to read the actual target spec from its checked-out clone).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "// The target spec must exist on the default branch: the agent is told to",
            "detail": "resolve_preview_targets rejects a target whose spec_path cannot be read from the real repo's default branch (Ok(None) → 400 'spec path does not exist'; read error → 400) — preview targets are verified real specs in real repos, enforced server-side before any agent spawns."
          }
        ]
      },
      {
        "id": "370acbe299729aaf1b66b548",
        "decision": "satisfied",
        "requirement_quote": "- Auto-deleted after 24 hours (configurable)",
        "detail": "Preview cleanup is fully implemented: preview runs are torn down by a registered background GC job (meta_spec_preview_gc) whose TTL defaults to 24 hours and is configurable (GYRE_META_SPEC_PREVIEW_TTL_HOURS; sweep interval GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS); the GC kills expired agents, removes worktrees, deletes preview branches, revokes tokens, releases budget slots and drops the kv records, plus an orphan pass for records missing from the index. Manual deletion exists via DELETE /api/v1/meta-specs/preview/{preview_id} (404 on unknown ids). No refs, no provenance, no audit trail are written for preview work, and preview agents are killed on completion (finish_preview_agent on every monitor exit — no idle state).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_preview_gc\".to_string(),",
            "detail": "The preview GC is a registered, enabled background job bound to the real teardown sweep (run_once → meta_specs.rs), with configurable interval — auto-deletion after TTL is a production background behavior, not request-time-only."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "pub fn preview_ttl_secs_from_env() -> u64 {",
            "detail": "Preview TTL (default 24h) read from GYRE_META_SPEC_PREVIEW_TTL_HOURS — the 'configurable' part of the 24-hour auto-delete; DELETE /api/v1/meta-specs/preview/:preview_id (route in api/mod.rs) provides the manual deletion path."
          }
        ]
      },
      {
        "id": "4f5cf5999c3b82110b7e611b",
        "decision": "satisfied",
        "requirement_quote": "- **Action bar:** \"Preview\" (run), \"Clear\" (delete preview branches), \"Publish\" (commit + approve flow)",
        "detail": "The meta-spec editor UI implements the specced layout: an inline editor for the meta-spec content (with a locked-diff against the published prompt during a run), a target-spec selector browsing the workspace's repos and their specs, a diff viewer showing existing code vs preview output that updates as each preview agent completes (per-agent tabs for multiple targets, landing on the first agent with a diff mid-run), and an action bar with Preview (run), Clear (delete the preview run's branches via the DELETE endpoint) and Publish (commit + approve flow) buttons.",
        "evidence": [
          {
            "file": "web/src/components/MetaSpecs.svelte",
            "quote": "<Button variant=\"secondary\" onclick={startPreview} disabled={!canPreview} data-testid=\"preview-btn\">",
            "detail": "The Preview action-bar button starts a run (spawn + poll); the adjacent primary Publish button (same action bar) commits the draft through the registry approval flow."
          },
          {
            "file": "web/src/components/MetaSpecs.svelte",
            "quote": "<Button variant=\"primary\" onclick={publish} disabled={publishSaving}>",
            "detail": "Publish in the same action bar calls the registry publish path — Preview/Clear/Publish all present as specced."
          },
          {
            "file": "web/src/components/MetaSpecs.svelte",
            "quote": "data-testid=\"preview-loop\"",
            "detail": "The workspace editor renders the preview loop in a split layout (split-left editor / split-right target selector) with the diff panel below; previewAgents tabs and agentDiffStats drive the multi-target diff viewer that updates as agents complete."
          }
        ]
      },
      {
        "id": "17d4a552af4fe91dd9d444f9",
        "decision": "satisfied",
        "requirement_quote": "Reconciliation re-runs the Ralph loop with the same specs but new rules, on branches, without touching main.",
        "detail": "Implemented as specced in substance: reconciliation creates tasks (labeled reconciliation/auto-created with wave and meta-spec provenance labels) scoped per repo; Immediate/Rolling strategy tasks carry the Delegation task_type — the repo-orchestrator spawning signal — so agents are spawned through the normal Ralph loop machinery under the current (new) meta-spec versions (prompt assembly injects current approved content); agents work on their own branches and push, going through normal gates and MR/merge-queue completion (auto_merge enqueues reconciliation MRs into the merge queue at agent completion); main is never touched directly. The task description embeds the re-implementation directive and explicitly allows the no-op outcome. Divergence in branch naming (reconcile/{reconciliation-id}/{spec} vs normal agent branches) is assessed under the Flow subheading.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.task_type = if delegation {",
            "detail": "Reconciliation tasks carry TaskType::Delegation (the repo-orchestrator spawning signal) for Immediate/Rolling waves — reconciliation work re-enters the normal agent/Ralph loop rather than a side channel; Background tasks are created without a task type so they stay inert in Backlog."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "no-op outcome (code already conforms) is valid.",
            "detail": "The created task's description directs the agent to re-run affected work under the updated rules and names the no-op MR as a valid outcome — the 'produce a no-op MR if the existing code already conforms' behavior is part of the production task contract."
          }
        ]
      },
      {
        "id": "98710dea3a37ec58ea6978a7",
        "decision": "satisfied",
        "requirement_quote": "Reconciliation is triggered when a workspace's meta-spec set is updated to reference a new version of a meta-spec (typically after the preview loop has validated the change):",
        "detail": "Exactly implemented: PUT /api/v1/workspaces/:id/meta-spec-set (Admin-only) loads the old set before upsert, computes the diff of pinned path@sha entries (only new or re-pinned entries count; an identical re-PUT is a no-op), emits MetaSpecSetUpdated, and calls run_reconciliation with the changed paths. The controller additionally gates on the changed meta-spec being Approved in the registry (a Pending bind is not a rollout — the approval transition owns that wave), and the complementary approval-side trigger (approving changed content fans out to every workspace whose effective set binds the meta-spec) covers the publish path.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let changed_paths = crate::reconciliation::diff_meta_spec_set(&old_set, &set);",
            "detail": "put_meta_spec_set: the set-diff between old and newly pinned SHAs is the reconciliation trigger; non-empty diffs emit MetaSpecSetUpdated and invoke run_reconciliation for the workspace — the trigger is the explicit binding update, as specced."
          }
        ]
      },
      {
        "id": "0f67c6dbc5bb328a54047724",
        "decision": "gap",
        "requirement_quote": "When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.",
        "detail": "The controller's comparison half is real: run_conformance_sweep (§10's background job, daily by default) continuously compares each workspace's desired state (effective set SHA) against the meta_spec_set_sha recorded in provenance attestations, and on drift it ensures a drift-review task exists and notifies members. The gap is the rollout-policy clause: work created by the continuous comparison does not follow the workspace's rollout policy. The sweep's drift-review task is created directly with a hardcoded TaskPriority::Medium, created immediately regardless of strategy (no Immediate/Rolling/Background dispatch, no max_concurrent_agents batching, no auto_merge handling) — the inspected sweep body (reconciliation.rs 1697-1860) never calls get_workspace_rollout_policy. Only the set-update/approval-triggered waves (run_reconciliation) apply the policy. So a workspace whose drift is caught by the continuous controller rather than a binding change gets policy-less work: the specced 'creates reconciliation work according to the workspace's rollout policy' holds for the trigger path but not for the drift-detection path.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "/// Run one conformance sweep pass across all workspaces (§10).",
            "detail": "The continuous comparator: for each workspace it compares the effective set SHA (desired) against provenance-recorded SHAs (actual) and creates drift work — the controller's compare-and-create loop exists as a real background job."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.priority = TaskPriority::Medium;",
            "detail": "The sweep-created drift-review task is hardcoded to Medium priority and created immediately in the sweep body (inspected 1697-1860): no get_workspace_rollout_policy call, no RolloutStrategy dispatch, no batching — drift-caught work bypasses the §7 rollout policy the controller is required to honor."
          }
        ]
      },
      {
        "id": "649ed86870daac6de2032d54",
        "decision": "gap",
        "requirement_quote": "   -> Agents work on reconciliation branches: reconcile/{reconciliation-id}/{spec}",
        "detail": "The flow's spine is implemented (registry records new SHA + approval reset on content edit; set update / approval triggers the controller; tasks are created per repo with reconciliation/auto-created labels plus wave-id and meta-spec@version provenance labels grouping the wave; delegation-typed tasks spawn agents through the normal loop under the new rules; MRs open through normal Ralph completion and merge through the normal merge queue; new provenance records the new set, resolving drift). Three concrete divergences from the specced flow: (1) tasks are created per (repo, changed meta-spec) for EVERY repo in the workspace — not 'per affected spec' identified 'via provenance query' (step 3's blast-radius-via-provenance is absent; run_reconciliation iterates all workspace repos); (2) the created task's spec_path is the changed META-SPEC's name, whereas the specced task carries spec_ref of the affected implementation spec plus meta_spec_ref and a reconciliation_id field — the affected implementation spec is never identified per task; (3) agents work on normal per-agent branches, not reconcile/{reconciliation-id}/{spec} branches (no reconcile/ branch prefix exists in production code; only a test fixture string).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "spec_path: ms.name.clone(),",
            "detail": "The candidate's spec_path is the changed meta-spec's registry name — the created task's spec_path points at the governing meta-spec, not at an affected implementation spec; the specced task fields spec_ref ('same spec, new rules') and meta_spec_ref are not both carried, and per-spec provenance scoping is replaced by whole-repo candidates."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "for repo in &repos {",
            "detail": "Candidate generation iterates every repo in the workspace (repos = list_by_workspace) with dedup only against existing open tasks — no provenance query filters to repos/specs with code under the superseded version; step 3's 'Affected repos and specs identified via provenance query' is not implemented on this path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let jj_workspace_name = format!(\"agent-{}\", agent.id);",
            "detail": "Spawned agents (including reconciliation-task agents) get standard per-agent workspaces/branches; no reconcile/{reconciliation-id}/{spec} branch naming exists in production code (the only 'reconcile/' occurrence in the tree is a test fixture string in reconciliation.rs tests) — reconciliation work rides normal agent branches."
          }
        ]
      },
      {
        "id": "86e5593619bcaf8cc10f3778",
        "decision": "satisfied",
        "requirement_quote": "- A directive: \"This code was produced under [old persona]. You are operating under [new persona]. Update the implementation to conform to your current guidelines.\"",
        "detail": "The re-implement contract is delivered through the production task and spawn machinery: the reconciliation task's description names the changed meta-spec (name, kind, version, content hash) and directs 'Re-run affected work under the updated rules; no-op outcome (code already conforms) is valid' — the agent receives the governing new rules (spawn-time prompt assembly injects the current approved meta-spec content, i.e. the new version), the existing code (the repo checkout the agent works in, enabling refactor rather than rewrite), and the directive to conform. No-op MRs are explicitly valid outcomes in the task contract, and the agent retains the refactor/rewrite/no-op decision.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "no-op outcome (code already conforms) is valid.",
            "detail": "The directive text of every created reconciliation task: identifies the changed meta-spec/version and instructs re-implementation under the updated rules with the no-op outcome legitimate — the specced directive's production equivalent."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let meta_prompt = crate::prompt_assembly::render_system_prompt(&prompt_set.sections);",
            "detail": "The spawned reconciliation agent's system prompt carries the current (new) meta-spec content via the assembled prompt set — the agent operates under the new rules while seeing the existing code in its checkout, satisfying 'the new meta-spec set + the existing code for context'."
          }
        ]
      },
      {
        "id": "6470095233cee073032bef21",
        "decision": "satisfied",
        "requirement_quote": "**Default:** `Rolling` strategy, `Normal` priority, `auto_merge: false` (MRs require gate passage but merge automatically if gates pass).",
        "detail": "Fully implemented: RolloutPolicy {strategy, max_concurrent_agents, priority, auto_merge} with RolloutStrategy (Immediate/Rolling/Background) and ReconciliationPriority (High/Normal/Low) exist in the domain with the specced default (Rolling/Normal/auto_merge false; batch size defaults to 5, which the spec leaves to configuration). Per-workspace persistence via the RolloutPolicyRepository port (SQLite + Postgres, migration 000057) with GET/PUT /api/v1/workspaces/:id/meta-specs/rollout-policy (PUT Admin-only). run_reconciliation dispatches on the strategy: Immediate creates all tasks at once; Rolling creates the first max_concurrent_agents batch and defers the remainder in kv-backed, restart-safe pending state advanced by on_reconciliation_task_terminal as each batch completes; Background creates tasks without a task_type so they sit inert in Backlog for the orchestrator. Priority maps onto TaskPriority; auto_merge enqueues reconciliation MRs into the merge queue at agent completion (gates still enforced by the merge processor). Reconciliation agents consume the same workspace budget as feature agents (normal spawn budget checks), honoring 'reconciliation does not get a blank check'.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/meta_spec.rs",
            "quote": "strategy: RolloutStrategy::Rolling,",
            "detail": "RolloutPolicy::default() implements the specced default exactly (Rolling / Normal / auto_merge: false) with a test pinning it to the spec text; the policy model (strategies, priority, max_concurrent_agents, auto_merge) mirrors the §7 struct."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let batch_size = policy.max_concurrent_agents.max(1) as usize;",
            "detail": "Rolling strategy: the first batch of max_concurrent_agents tasks is created at trigger time and the remainder persisted (kv-backed pending state) for creation as each batch completes — the specced batching semantics, restart-safe, with a mid-rollout strategy switch flushing the remainder."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "crate::reconciliation::maybe_auto_enqueue_reconciliation_mr(&state, &mr, &task).await;",
            "detail": "auto_merge wiring: at agent completion, reconciliation-labeled MRs are enqueued into the merge queue so they merge automatically once gates pass (gate passage still enforced by the merge processor) — the specced auto_merge semantics with default false."
          }
        ]
      },
      {
        "id": "533e9ebcff810b3fd8932c9a",
        "decision": "satisfied",
        "requirement_quote": "A workspace whose own set cannot be read (storage error or corrupt JSON) is skipped too: the override gate never fails open, so an unreadable set is never treated as \"no overrides\"",
        "detail": "The tenant-scope machinery is implemented as amended: the tenant meta-spec set is the unit of binding (GET/PUT /api/v1/tenants/:id/meta-specs/set, Admin-only + caller's tenant must match); the effective set returned by GET /workspaces/:id/meta-spec-set, hashed by the §9 constraint/§10 sweep (compute_meta_spec_set_sha over the merged effective set, byte-identical to the stored workspace JSON for tenant-less workspaces), and resolved by the blast-radius endpoint is always the merge of tenant defaults with workspace overrides (workspace pin wins). run_tenant_reconciliation propagates only paths the tenant set pins (new/re-pinned entries — removals are not triggers), runs one per-workspace §6 wave per non-overriding workspace, skips overriders (their own set-diff trigger owns their rollout), and skips workspaces whose set cannot be read — fail-closed, never treating an unreadable set as no-overrides. The force flag exists on both the tenant-set PUT and the Tenant-scoped registry approval (PUT /api/v1/meta-specs/:id), restricted to admins of the scope tenant, 400 on non-Tenant scopes, and propagates past workspace overrides. The Tenant-scoped registry approval fans out through the same tenant controller; inheriting workspaces reconcile autonomously per §6 (no agent is spawned by the trigger itself — only tasks are created).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "if !force && workspace_overrides_path(&tenant_set, &ws_set, path) {",
            "detail": "Per-path override discrimination in run_tenant_reconciliation: a workspace whose own set pins the changed path is skipped (its own §6 set-diff trigger owns its rollout) unless force — §8 step 3's individual-policy application."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "summary.workspaces_skipped_error += 1;",
            "detail": "The fail-closed override gate: a workspace whose set cannot be loaded (storage error or corrupt JSON) is skipped and counted, never decoded as 'no overrides' — exactly the quoted requirement; the doc comment marks it 'Never fail open the override gate'."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "force is only valid when approving a Tenant-scoped meta-spec",
            "detail": "Force on the registry-approval path is validated to Tenant scopes only and to admins of the scope tenant (an admin of another tenant gets 403) — §8 step 4's admin-restricted urgent propagation; the tenant-set PUT carries the same force flag with the same Admin + tenant-containment gate."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "if auth.tenant_id != tenant_id {",
            "detail": "Tenant containment on the tenant set endpoints: only an Admin of the tenant named in the path may view/update the tenant-wide default binding (GET and PUT both) — the 'Admin-only, caller's tenant must match' requirement."
          }
        ]
      },
      {
        "id": "d0c7637a52d62fe7a81b940e",
        "decision": "gap",
        "requirement_quote": "    /// How many versions behind before blocking (if block_on_drift is true).",
        "detail": "The opinionated default and strict mode are real: the merge processor checks every merge-queue cycle for meta-spec drift (the MR's provenance-attested set SHA vs the workspace's current effective set SHA), records a [WARN] meta-spec-drift gate result and merges (warn, don't block), and a per-workspace opt-in strict policy converts drift into a hard block (entry requeues with the drift reason until reconciled or relaxed) via GET/PUT /workspaces/:id/merge-policy. The gap is the specced MetaSpecPolicy shape: the enforced policy is a single boolean (require_current_meta_spec_set). warn_on_drift is not configurable (warnings are always recorded — acceptable as the default is true and 'always on in strict mode'), but drift_tolerance — how many versions behind before blocking, default 1 — does not exist anywhere in the enforcement path: strict mode blocks on any set-SHA mismatch (tolerance-0 behavior only), and no version-count reasoning is computed. Worse, the workspace settings UI renders the full specced policy (warn_on_drift/block_on_drift/drift_tolerance checkboxes and tolerance input) and saves it via api.updateWorkspace({meta_spec_policy: ...}) — but UpdateWorkspaceRequest has no meta_spec_policy field, so the server silently drops the payload (serde ignores unknown fields) and GET never returns it: the §9 policy UI is theater, and a workspace wanting tolerance-1 blocking has no mechanism. The drift warning also names set SHAs rather than 'persona backend-developer v3 -> v4' (per-meta-spec version naming), since the recorded provenance is a set hash.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_policy.rs",
            "quote": "pub require_current_meta_spec_set: bool,",
            "detail": "The entire enforced §9 policy: one boolean. No warn_on_drift toggle, no block_on_drift separate from the single strict flag, no drift_tolerance — the specced MetaSpecPolicy struct's tolerance semantics ('versions behind before blocking') have no server-side representation."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "if policy.require_current_meta_spec_set {",
            "detail": "The strict-mode enforcement is real (drift → hard block with requeue reason; default warn-and-proceed via record_meta_spec_drift_warning) — the behavior half of §9 works, but only at tolerance 0."
          },
          {
            "file": "web/src/components/WorkspaceSettings.svelte",
            "quote": "warn_on_drift: warnOnDrift,",
            "detail": "The settings UI collects the full specced MetaSpecPolicy (warn_on_drift/block_on_drift/drift_tolerance, lines 131-133 and the form at ~1128-1151) and saveDriftPolicy PUTs it as meta_spec_policy on the workspace — fields the server's UpdateWorkspaceRequest does not define, so the save is silently discarded and drift_tolerance can never take effect."
          },
          {
            "file": "crates/gyre-server/src/api/workspaces.rs",
            "quote": "pub struct UpdateWorkspaceRequest {",
            "detail": "The workspace update request enumerates name/description/budget/max_repos/max_agents_per_repo/trust_level/llm_model/compute_target_id — no meta_spec_policy field (grep over all crates finds no meta_spec_policy anywhere in Rust): the UI's §9 policy payload is dropped by serde's unknown-field tolerance, and the workspace response never carries it back."
          }
        ]
      },
      {
        "id": "8dc04f90bec1c77e6b42f252",
        "decision": "gap",
        "requirement_quote": "A background job (like speculative merging) continuously checks for meta-spec drift across all repos:",
        "detail": "The sweep exists and does the core job: a registered, enabled background job (meta_spec_conformance_sweep, daily by default, interval configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS) runs run_conformance_sweep across all workspaces, compares each workspace's effective set SHA against provenance-recorded set SHAs, creates a drift-review task if missing (deduped per workspace+set-sha), emits MetaSpecDriftDetected, records drift metrics, and notifies workspace members and meta-spec owners — database queries only, no agent spawns. Two concrete shortfalls against the specced sweep: (1) the comparison is windowed — only attestations within the last 30 days (SWEEP_PROVENANCE_WINDOW_SECS = 30*86400) are examined, so code whose provenance is older than 30 days is never compared and its drift is never detected; the section's stated purpose ('ensures the system converges even after disruptions', catching 'a workspace binding was updated while agents were offline') fails for drift older than the window; (2) the pseudocode iterates per spec implemented in each repo, while the implementation compares per repo (any attestation with a mismatched set SHA flags the whole repo) and creates one workspace-level drift-review task rather than per-spec reconciliation tasks. The sweep-created work also bypasses the rollout policy (assessed under §6 The Reconciliation Controller).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_conformance_sweep\".to_string(),",
            "detail": "The conformance sweep is a registered, enabled background job with configurable interval (GYRE_META_SPEC_SWEEP_INTERVAL_SECS, default SWEEP_INTERVAL_SECS = 86400) bound to the real sweep — the 'runs every N hours, configurable' requirement is met."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "const SWEEP_PROVENANCE_WINDOW_SECS: u64 = 30 * 86_400;",
            "detail": "The sweep's provenance query is bounded to a 30-day window: chain_attestations.find_by_repo(repo, since, now) with since = now - 30 days. Provenance older than 30 days is never compared, so drift in code produced before the window is permanently invisible to the sweep — contradicting the section's convergence purpose for exactly the disruption cases it names."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS);",
            "detail": "The windowing call site inside run_conformance_sweep — the only provenance input to the desired-vs-actual comparison; no unbounded or per-spec provenance walk exists on the sweep path."
          }
        ]
      },
      {
        "id": "4ec94a54317e88be7eb5b60b",
        "decision": "gap",
        "requirement_quote": "- **Workspace detail view:** meta-spec set with version indicators (current/drifted), reconciliation progress bar, drift count per repo.",
        "detail": "Events and metrics are largely real; the dashboard is largely missing. Events: MetaSpecChanged (emit_meta_spec_changed on registry content update), MetaSpecSetUpdated (workspace and tenant variants), ReconciliationStarted (custom Event-tier kind on wave creation), ReconciliationCompleted (MessageKind::ReconciliationCompleted with notification fan-out and duration metric), MetaSpecDriftResolved (on last terminal reconciliation/drift task) all exist; MetaSpecDriftDetected is emitted by the conformance sweep but NOT by the merge-gate path — an MR found drifted records only a [WARN] gate result (record_meta_spec_drift_warning saves a GateResult; no event), while the spec's table says the event fires when 'MR or conformance sweep found code under superseded meta-spec'. Metrics: all three specced Prometheus metrics exist (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and are recorded in production paths. Dashboard: the workspace home shows the effective meta-spec set grouped by kind with version numbers and a 'rules reconciling' banner — but no current/drifted indicators, no reconciliation progress bar, no drift count per repo; the tenant admin view (TenantSettings) has no rollout-plan status, per-workspace reconciliation progress, or total drift count (its tabs are Users/Compute Targets/Budget/Audit/Health/Jobs); the repo detail view has no per-spec meta-spec provenance display and no drift badges on commits (grep across web components finds no such surface).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/metrics.rs",
            "quote": "\"gyre_meta_spec_drift_total\",",
            "detail": "All three specced metrics are registered on the Prometheus registry (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and recorded from production paths (record_drift_metrics / record_reconciliation_metrics / wave completion)."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "<span class=\"rule-version\">v{ms.version}</span>",
            "detail": "The workspace home's Agent Rules section renders the effective meta-spec set grouped by kind with version numbers and a reconcile-status banner — the partial realization of the workspace detail view; it carries no current/drifted indicator, no reconciliation progress bar, and no per-repo drift count (inspected the full section template)."
          },
          {
            "file": "web/src/components/TenantSettings.svelte",
            "quote": "Tabs: Users | Compute Targets | Budget | Audit | Health | Jobs",
            "detail": "The tenant administration surface has no meta-spec rollout content at all — no rollout plan status, per-workspace reconciliation progress, or total drift count (inspected the component; no reconciliation/drift/rollout references exist in it)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "async fn record_meta_spec_drift_warning(",
            "detail": "The merge-gate drift path only persists a GateResult (meta-spec-drift, Passed, [WARN] output) — the inspected function body performs no event emission, so MetaSpecDriftDetected never fires for MR-found drift, only for sweep-found drift (reconciliation.rs emits the custom meta_spec_drift_detected kind), contrary to the event table's 'MR or conformance sweep' trigger."
          }
        ]
      },
      {
        "id": "6a567de42f7c1b0e5a7027b1",
        "decision": "context",
        "requirement_quote": "- **platform-model.md** -- meta-spec set is a new workspace-level concept; MCP protocol injection (Section 8) gains meta-spec set resolution",
        "detail": "Cross-reference section describing how this spec extends/depends on other canonical specs. No standalone implementable requirement beyond what the Design sections already require (the injection-extension claim is assessed under §2; the spec-registry kind-field claim under §1; spec-lifecycle trigger coexistence under §6). Classified context per the audit rule against double-counting children's requirements under a parent.",
        "evidence": []
      },
      {
        "id": "10a7e61edf56f45cb9dbeee8",
        "decision": "context",
        "requirement_quote": "This design borrows deliberately from Kubernetes:",
        "detail": "Analogy table mapping Kubernetes concepts (Deployment spec, controller loop, rolling update, admission controller, ConfigMap, canary) onto this design's constructs. Pure rationale/analogy — the mapped Gyre constructs are assessed under their own Design headings.",
        "evidence": []
      },
      {
        "id": "a854e91085ee72f68e6ca854",
        "decision": "gap",
        "requirement_quote": "- **Does not require re-implementation of everything.** Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped. Agents may produce no-op MRs for code that already conforms.",
        "detail": "Four of the five anti-requirements hold: meta-spec sets are never auto-updated on content change (bindings change only via the explicit set PUT or tenant rollout — no code path writes meta_spec_sets on registry update); spec iteration remains the spec lifecycle's job; the default rollout policy is Normal priority interleaved with feature work (reconciliation does not block feature work); deployment/production concerns are out of scope here. The violated anti-requirement is the quoted one: candidates are NOT limited to specs with provenance records under the old version, and unaffected repos are NOT skipped. run_reconciliation treats every repo in the workspace as a reconciliation candidate for each changed meta-spec (its doc comment states 'every repo in the workspace is a reconciliation candidate... the workspace binding governs all repos'), with dedup only against existing open tasks — no provenance query filters unaffected repos, so reconciliation task waves do require re-implementation work for repos with no code under the superseded version. (The no-op-MR clause itself is honored — the task contract allows no-op outcomes.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "for repo in &repos {",
            "detail": "Candidate generation loops over ALL repos of the workspace (list_by_workspace) for each changed meta-spec; there is no provenance-based filter that would skip a repo with no code produced under the superseded version — the anti-requirement 'Unaffected repos are skipped' is not implemented on the reconciliation path."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "/// (the workspace binding governs all repos in the workspace — §2), and",
            "detail": "The controller's own doc comment codifies the divergence: 'For each, every repo in the workspace is a reconciliation candidate (the workspace binding governs all repos in the workspace — §2), and §6 step 2 requires a task per repo' — an explicit design choice that contradicts this section's 'Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped.'"
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "c2ad988fa981b60eae143fd1",
        "decision": "gap",
        "requirement_quote": "Meta-specs get the same SHA-pinned versioning, approval flow, and link graph that regular specs already have. They appear in the spec registry alongside implementation specs.",
        "detail": "Requires meta-specs to be first-class versioned registry artifacts: SHA-pinned versioning, approval flow, link graph, registry presence alongside implementation specs, manifest declaration (kind/scope/auto_create_tasks/auto_reconcile), and per-kind approval modes (meta:persona/meta:principle/meta:process human_only; meta:standard human_and_agent). Registry semantics are amended by agent-runtime.md §2 ('DB-backed, not repo-backed': content-addressable SHA-256 hashes computed from content, full version history in a meta_spec_versions table, all edits stored in the database), which explicitly supersedes this section's git-backed model, and its amendments-table row repeats 'Reconciliation spec defers to this for registry semantics'. Accordingly, the manifest-driven registration surface — the yaml manifest declaring meta-specs with kind/scope/auto_create_tasks/auto_reconcile, and meta-specs 'appearing in the spec registry alongside implementation specs' — is classified as superseded by the DB-backed replacement, not a live gap: the existing meta_specs/meta_spec_versions registry IS the amended model (the four kinds, u32 version + SHA-256 content_hash, immutable version history, full CRUD/approval surface at /api/v1/meta-specs), the manifest's SpecEntry.kind field is vestigial declaration metadata under it (no auto_reconcile flag exists anywhere, and nothing bridges manifest entries into the meta-spec registry — correctly so under 'all edits are stored in the database'), and active task-302 retires the remaining set-based consumers in favor of the required-flags + spec-level-bindings model. What remains missing against the amended model: (1) per-kind approval modes — one approval_status lifecycle applies verbatim to every kind; the kind field is never consulted when an approval is applied (ms.kind is read only for event payloads and task titles), so human_only for meta:persona/meta:principle/meta:process is not enforced by kind — it is only incidentally true because the write gate admits tenant Admins and workspace Owner/Admin members and no agent principal can pass it, which simultaneously makes the specced human_and_agent dual mode for meta:standard unreachable: no kind-conditional agent-admission widening exists anywhere in the approval path; (2) the Repo scope value — agent-runtime.md preserves PersonaScope Repo ('Repo-scoped personas continue to work as nearest-scope-wins overrides ... but cannot be marked required'), but MetaSpecScope is Global/Tenant/Workspace only, so repo-scoped meta-specs have no representation at all; (3) link-graph participation — spec-links operate on the spec ledger only; no link machinery covers meta-specs, and the amendment does not address §1's link-graph clause.",
        "evidence": [
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "This supersedes the git-backed meta-spec model from `meta-spec-reconciliation.md` §1 — the reconciliation spec's registry semantics are replaced by this DB-backed model",
            "detail": "The canonical amendment (agent-runtime.md §2, 'DB-backed, not repo-backed' paragraph): §1's git/manifest-backed registry semantics are explicitly replaced by the DB-backed model, so the manifest-registration surface, the auto_reconcile manifest flag, and 'appear in the spec registry alongside implementation specs' are superseded requirements rather than open gaps — the existing DB registry is the amended model."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "Reconciliation spec defers to this for registry semantics",
            "detail": "The amendments-table row for meta-spec-reconciliation.md in agent-runtime.md confirms the deferral of registry semantics, including the replacement of the set-PUT surface with required flags + spec-level bindings that active task-302 implements."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "if let Some(status_str) = &req.approval_status {",
            "detail": "update_meta_spec_registry applies the caller-supplied approval_status identically for every kind: the block parses the status, stamps approved_by/approved_at/approved_content_hash and assigns ms.approval_status with no kind-conditional branch anywhere in the function (inspected the full update path). meta:persona human_only and meta:standard human_and_agent approval modes from the §1 table are therefore not differentiated."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "only tenant Admin may write Global-scoped meta-specs",
            "detail": "authorize_registry_write gates every registry write (including approval_status PUTs) on tenant Admin role or workspace Owner/Admin membership. Agent principals (UserRole::Agent, no workspace membership, no user_id) receive 403 on every arm, so the specced human_and_agent mode for meta:standard is unreachable: no agent principal can ever record an approval, and nothing kind-conditional widens the approval step for meta:standard."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "invalid scope '{s}'; must be one of: Global, Tenant, Workspace",
            "detail": "parse_scope admits only Global/Tenant/Workspace. agent-runtime.md explicitly preserves PersonaScope Repo for meta-specs ('Repo-scoped personas continue to work as nearest-scope-wins overrides ... but cannot be marked required'), so the Repo scope value is a live requirement under the amended model — and it has no representation in the meta-spec registry."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "kind: entry.kind.clone(),",
            "detail": "The manifest kind field's sole consumption in the spec-registry registration path is being copied onto the ledger entry — vestigial declaration metadata under the DB-backed model: nothing derives approval mode from it or bridges manifest-declared meta-specs into the meta_specs registry that prompt assembly, sets, and reconciliation read from (consistent with the amendment's 'all edits are stored in the database'; no auto_reconcile flag exists anywhere in the codebase)."
          }
        ]
      },
      {
        "id": "815f40a82746fafd22856e09",
        "decision": "gap",
        "requirement_quote": "When an agent is spawned in a workspace, the MCP server injects the resolved meta-spec set into the agent's system prompt (extending the existing protocol injection from platform-model.md Section 8).",
        "detail": "Requires a workspace-level pinned meta-spec set with nearest-scope-wins resolution (repo overrides workspace overrides tenant), SHA-pinned entries whose explicit update triggers reconciliation, and spawn-time injection of the resolved set into the agent system prompt. What exists and is real: MetaSpecSet/TenantMetaSpecSet storage with the specced shape (personas map, principles/standards/process lists of path@sha entries) via port-backed repositories; GET/PUT /api/v1/workspaces/:id/meta-spec-set (Admin-only PUT) and /api/v1/tenants/:id/meta-specs/set; tenant-to-workspace inheritance with workspace-pin-wins (merge_meta_spec_sets / resolve_effective_meta_spec_set); the set-update diff is the explicit reconciliation trigger. The injection clause is amended rather than violated: agent-runtime.md's amendments-table row for meta-spec-reconciliation.md replaces the set as the injection surface ('The PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API'), and spawn-time prompt assembly implements exactly that amended model — required Global/Tenant/Workspace meta-specs (bands 1-2) plus spec-level bindings at pinned versions (band 3) — so the set functioning as an accounting/drift construct (set SHA for provenance attestations, §9 drift checks, §10 sweep, blast-radius resolution) rather than the injection source is the amended design, not a gap. The remaining live gap is the repo tier of the scope cascade: 'Repo-level meta-spec set overrides workspace' is unimplemented. Only workspace and tenant set repositories exist (no repo-level set port, handler, or resolution step — resolve_effective_meta_spec_set merges tenant and workspace sets only), and meta-spec scope has no Repo value, while agent-runtime.md explicitly preserves Repo-scoped personas as nearest-scope-wins overrides that cannot be marked required. Under the amended semantics the repo tier means repo-scoped entries acting as name-resolution overrides that never remove a required workspace/tenant injection (same-name collisions concatenate; dedup only by meta_spec_id) and are never requiredable — none of that exists.",
        "evidence": [
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "The `PUT /api/v1/workspaces/{id}/meta-spec-set` endpoint is replaced by updating `required` flags and spec-level bindings via the meta-spec API",
            "detail": "The canonical amendment (agent-runtime.md amendments-table row for meta-spec-reconciliation.md): the set is replaced as the injection/binding surface by required flags + spec-level bindings, so prompt assembly drawing from required flags and spec-level bindings instead of the workspace set is the amended injection model, not a §2 violation."
          },
          {
            "file": "specs/system/agent-runtime.md",
            "quote": "Repo-scoped personas continue to work as nearest-scope-wins overrides per `platform-model.md` §2, but cannot be marked `required`",
            "detail": "The amended repo-tier semantics: repo-scoped entries survive as nearest-scope-wins name-resolution overrides, never required, never removing a required injection (same-name collisions concatenate). This is the live requirement the missing repo tier would carry."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "// Band 1: required tenant-level meta-specs. Global entries apply to",
            "detail": "assemble_prompt_set is the only prompt-assembly path feeding agent system prompts; its band structure (required Global/Tenant meta-specs, required Workspace meta-specs, then spec-level pinned bindings) resolves from state.meta_specs and state.meta_spec_bindings — the amended injection model. The workspace meta_spec_sets store is never consulted for injection (the set persists as the provenance/drift/reconciliation-trigger construct)."
          },
          {
            "file": "crates/gyre-ports/src/meta_spec_set.rs",
            "quote": "/// Repository for workspace meta-spec sets (M34 Slice 5).",
            "detail": "The meta-spec-set persistence layer defines exactly two repositories — MetaSpecSetRepository (keyed by workspace id) and TenantMetaSpecSetRepository (keyed by tenant id). No repo-level set repository, handler, or port exists anywhere in the codebase (inspected the api/mod.rs route table and the reconciliation.rs scope-resolution helpers), so the repo-overrides-workspace rule of the scope cascade is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let mut effective = merge_meta_spec_sets(&tenant_set, &ws_set);",
            "detail": "resolve_effective_meta_spec_set merges tenant defaults with the workspace's own set only — the cascade has exactly two tiers. There is no repo-tier resolution step, so a repo-level override of a workspace or tenant pin cannot be expressed, and the §2 scope-resolution ladder stops at workspace."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let changed_paths = crate::reconciliation::diff_meta_spec_set(&old_set, &set);",
            "detail": "Real, working part: PUT /workspaces/:id/meta-spec-set loads the old set, upserts the new pinned path@sha entries, and the SHA diff is the explicit §6 reconciliation trigger — version pinning as an explicit action is genuinely implemented (task-302's registry-model migration preserves this trigger semantics via required-flag changes and meta-spec approval)."
          }
        ]
      },
      {
        "id": "3662e9dd04c597d5db7d61f0",
        "decision": "gap",
        "requirement_quote": "This is a database query over provenance records + workspace meta-spec set bindings. The blast radius is computed, not guessed.",
        "detail": "Requires blast radius computed from provenance records plus set bindings, with per-repo spec/commit counts under the superseded version and version-diff reasoning (the example skips a workspace 'bound to v2, not affected'). What exists: GET /api/v1/meta-specs/:path/blast-radius does a real binding scan — it lists all workspaces, resolves each one's effective set (tenant defaults merged with workspace overrides), checks whether any personas/principles/standards/process entry references the path, labels the reason workspace_binding vs tenant_inheritance (the §8 amendment), and lists the repos of affected workspaces. What is missing: the provenance half — the handler (inspected in full, meta_specs.rs 650-745) never queries attestation/provenance stores, so there are no per-repo spec counts, no commit counts under the superseded version, and no version discrimination at all: a workspace pinned to any version of the path counts as affected (the 'bound to v2, skipped' case is not skipped), and the response carries only affected repo/workspace ids with a reason string — no totals over provenance. The binding half is real; the 'database query over provenance records' half is absent.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let references_spec = effective.personas.values().any(|e| e.path == spec_path)",
            "detail": "The entire affected-workspace test is a path-membership check on the effective set's pinned entries (|| principles/standards/process any()). No comparison of the pinned sha against the changed version, no provenance lookup — a workspace pinned to an old version is reported affected exactly like one pinned to the new version."
          },
          {
            "file": "crates/gyre-server/src/api/meta_specs.rs",
            "quote": "let workspaces = state.workspaces.list().await.unwrap_or_default();",
            "detail": "Handler data sources, inspected across the full body (meta_specs.rs 650-745): workspaces.list, meta_spec_sets.get, tenant_meta_spec_sets.get, repos.list_by_workspace. No chain_attestations / attestation_store / spec ledger provenance query appears anywhere in the handler, so the spec/commit counts and 'commits under v3' figures of the §4 output shape cannot be produced by this endpoint."
          }
        ]
      },
      {
        "id": "0f67c6dbc5bb328a54047724",
        "decision": "gap",
        "requirement_quote": "When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.",
        "detail": "The controller's comparison half is real: run_conformance_sweep (§10's background job, daily by default) continuously compares each workspace's desired state (effective set SHA) against the meta_spec_set_sha recorded in provenance attestations, and on drift it ensures a drift-review task exists and notifies members. The gap is the rollout-policy clause: work created by the continuous comparison does not follow the workspace's rollout policy. The sweep's drift-review task is created directly with a hardcoded TaskPriority::Medium, created immediately regardless of strategy (no Immediate/Rolling/Background dispatch, no max_concurrent_agents batching, no auto_merge handling) — the inspected sweep body (reconciliation.rs 1697-1860) never calls get_workspace_rollout_policy. Only the set-update/approval-triggered waves (run_reconciliation) apply the policy. So a workspace whose drift is caught by the continuous controller rather than a binding change gets policy-less work: the specced 'creates reconciliation work according to the workspace's rollout policy' holds for the trigger path but not for the drift-detection path.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "/// Run one conformance sweep pass across all workspaces (§10).",
            "detail": "The continuous comparator: for each workspace it compares the effective set SHA (desired) against provenance-recorded SHAs (actual) and creates drift work — the controller's compare-and-create loop exists as a real background job."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.priority = TaskPriority::Medium;",
            "detail": "The sweep-created drift-review task is hardcoded to Medium priority and created immediately in the sweep body (inspected 1697-1860): no get_workspace_rollout_policy call, no RolloutStrategy dispatch, no batching — drift-caught work bypasses the §7 rollout policy the controller is required to honor."
          }
        ]
      },
      {
        "id": "649ed86870daac6de2032d54",
        "decision": "gap",
        "requirement_quote": "   -> Agents work on reconciliation branches: reconcile/{reconciliation-id}/{spec}",
        "detail": "The flow's spine is implemented (registry records new SHA + approval reset on content edit; set update / approval triggers the controller; tasks are created per repo with reconciliation/auto-created labels plus wave-id and meta-spec@version provenance labels grouping the wave; delegation-typed tasks spawn agents through the normal loop under the new rules; MRs open through normal Ralph completion and merge through the normal merge queue; new provenance records the new set, resolving drift). Three concrete divergences from the specced flow: (1) tasks are created per (repo, changed meta-spec) for EVERY repo in the workspace — not 'per affected spec' identified 'via provenance query' (step 3's blast-radius-via-provenance is absent; run_reconciliation iterates all workspace repos); (2) the created task's spec_path is the changed META-SPEC's name, whereas the specced task carries spec_ref of the affected implementation spec plus meta_spec_ref and a reconciliation_id field — the affected implementation spec is never identified per task; (3) agents work on normal per-agent branches, not reconcile/{reconciliation-id}/{spec} branches (no reconcile/ branch prefix exists in production code; only a test fixture string).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "spec_path: ms.name.clone(),",
            "detail": "The candidate's spec_path is the changed meta-spec's registry name — the created task's spec_path points at the governing meta-spec, not at an affected implementation spec; the specced task fields spec_ref ('same spec, new rules') and meta_spec_ref are not both carried, and per-spec provenance scoping is replaced by whole-repo candidates."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "for repo in &repos {",
            "detail": "Candidate generation iterates every repo in the workspace (repos = list_by_workspace) with dedup only against existing open tasks — no provenance query filters to repos/specs with code under the superseded version; step 3's 'Affected repos and specs identified via provenance query' is not implemented on this path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let jj_workspace_name = format!(\"agent-{}\", agent.id);",
            "detail": "Spawned agents (including reconciliation-task agents) get standard per-agent workspaces/branches; no reconcile/{reconciliation-id}/{spec} branch naming exists in production code (the only 'reconcile/' occurrence in the tree is a test fixture string in reconciliation.rs tests) — reconciliation work rides normal agent branches."
          }
        ]
      },
      {
        "id": "d0c7637a52d62fe7a81b940e",
        "decision": "gap",
        "requirement_quote": "    /// How many versions behind before blocking (if block_on_drift is true).",
        "detail": "The opinionated default and strict mode are real: the merge processor checks every merge-queue cycle for meta-spec drift (the MR's provenance-attested set SHA vs the workspace's current effective set SHA), records a [WARN] meta-spec-drift gate result and merges (warn, don't block), and a per-workspace opt-in strict policy converts drift into a hard block (entry requeues with the drift reason until reconciled or relaxed) via GET/PUT /workspaces/:id/merge-policy. The gap is the specced MetaSpecPolicy shape: the enforced policy is a single boolean (require_current_meta_spec_set). warn_on_drift is not configurable (warnings are always recorded — acceptable as the default is true and 'always on in strict mode'), but drift_tolerance — how many versions behind before blocking, default 1 — does not exist anywhere in the enforcement path: strict mode blocks on any set-SHA mismatch (tolerance-0 behavior only), and no version-count reasoning is computed. Worse, the workspace settings UI renders the full specced policy (warn_on_drift/block_on_drift/drift_tolerance checkboxes and tolerance input) and saves it via api.updateWorkspace({meta_spec_policy: ...}) — but UpdateWorkspaceRequest has no meta_spec_policy field, so the server silently drops the payload (serde ignores unknown fields) and GET never returns it: the §9 policy UI is theater, and a workspace wanting tolerance-1 blocking has no mechanism. The drift warning also names set SHAs rather than 'persona backend-developer v3 -> v4' (per-meta-spec version naming), since the recorded provenance is a set hash.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_policy.rs",
            "quote": "pub require_current_meta_spec_set: bool,",
            "detail": "The entire enforced §9 policy: one boolean. No warn_on_drift toggle, no block_on_drift separate from the single strict flag, no drift_tolerance — the specced MetaSpecPolicy struct's tolerance semantics ('versions behind before blocking') have no server-side representation."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "if policy.require_current_meta_spec_set {",
            "detail": "The strict-mode enforcement is real (drift → hard block with requeue reason; default warn-and-proceed via record_meta_spec_drift_warning) — the behavior half of §9 works, but only at tolerance 0."
          },
          {
            "file": "web/src/components/WorkspaceSettings.svelte",
            "quote": "warn_on_drift: warnOnDrift,",
            "detail": "The settings UI collects the full specced MetaSpecPolicy (warn_on_drift/block_on_drift/drift_tolerance, lines 131-133 and the form at ~1128-1151) and saveDriftPolicy PUTs it as meta_spec_policy on the workspace — fields the server's UpdateWorkspaceRequest does not define, so the save is silently discarded and drift_tolerance can never take effect."
          },
          {
            "file": "crates/gyre-server/src/api/workspaces.rs",
            "quote": "pub struct UpdateWorkspaceRequest {",
            "detail": "The workspace update request enumerates name/description/budget/max_repos/max_agents_per_repo/trust_level/llm_model/compute_target_id — no meta_spec_policy field (grep over all crates finds no meta_spec_policy anywhere in Rust): the UI's §9 policy payload is dropped by serde's unknown-field tolerance, and the workspace response never carries it back."
          }
        ]
      },
      {
        "id": "8dc04f90bec1c77e6b42f252",
        "decision": "gap",
        "requirement_quote": "A background job (like speculative merging) continuously checks for meta-spec drift across all repos:",
        "detail": "The sweep exists and does the core job: a registered, enabled background job (meta_spec_conformance_sweep, daily by default, interval configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS) runs run_conformance_sweep across all workspaces, compares each workspace's effective set SHA against provenance-recorded set SHAs, creates a drift-review task if missing (deduped per workspace+set-sha), emits MetaSpecDriftDetected, records drift metrics, and notifies workspace members and meta-spec owners — database queries only, no agent spawns. Two concrete shortfalls against the specced sweep: (1) the comparison is windowed — only attestations within the last 30 days (SWEEP_PROVENANCE_WINDOW_SECS = 30*86400) are examined, so code whose provenance is older than 30 days is never compared and its drift is never detected; the section's stated purpose ('ensures the system converges even after disruptions', catching 'a workspace binding was updated while agents were offline') fails for drift older than the window; (2) the pseudocode iterates per spec implemented in each repo, while the implementation compares per repo (any attestation with a mismatched set SHA flags the whole repo) and creates one workspace-level drift-review task rather than per-spec reconciliation tasks. The sweep-created work also bypasses the rollout policy (assessed under §6 The Reconciliation Controller).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "name: \"meta_spec_conformance_sweep\".to_string(),",
            "detail": "The conformance sweep is a registered, enabled background job with configurable interval (GYRE_META_SPEC_SWEEP_INTERVAL_SECS, default SWEEP_INTERVAL_SECS = 86400) bound to the real sweep — the 'runs every N hours, configurable' requirement is met."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "const SWEEP_PROVENANCE_WINDOW_SECS: u64 = 30 * 86_400;",
            "detail": "The sweep's provenance query is bounded to a 30-day window: chain_attestations.find_by_repo(repo, since, now) with since = now - 30 days. Provenance older than 30 days is never compared, so drift in code produced before the window is permanently invisible to the sweep — contradicting the section's convergence purpose for exactly the disruption cases it names."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS);",
            "detail": "The windowing call site inside run_conformance_sweep — the only provenance input to the desired-vs-actual comparison; no unbounded or per-spec provenance walk exists on the sweep path."
          }
        ]
      },
      {
        "id": "4ec94a54317e88be7eb5b60b",
        "decision": "gap",
        "requirement_quote": "- **Workspace detail view:** meta-spec set with version indicators (current/drifted), reconciliation progress bar, drift count per repo.",
        "detail": "Events and metrics are largely real; the dashboard is largely missing. Events: MetaSpecChanged (emit_meta_spec_changed on registry content update), MetaSpecSetUpdated (workspace and tenant variants), ReconciliationStarted (custom Event-tier kind on wave creation), ReconciliationCompleted (MessageKind::ReconciliationCompleted with notification fan-out and duration metric), MetaSpecDriftResolved (on last terminal reconciliation/drift task) all exist; MetaSpecDriftDetected is emitted by the conformance sweep but NOT by the merge-gate path — an MR found drifted records only a [WARN] gate result (record_meta_spec_drift_warning saves a GateResult; no event), while the spec's table says the event fires when 'MR or conformance sweep found code under superseded meta-spec'. Metrics: all three specced Prometheus metrics exist (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and are recorded in production paths. Dashboard: the workspace home shows the effective meta-spec set grouped by kind with version numbers and a 'rules reconciling' banner — but no current/drifted indicators, no reconciliation progress bar, no drift count per repo; the tenant admin view (TenantSettings) has no rollout-plan status, per-workspace reconciliation progress, or total drift count (its tabs are Users/Compute Targets/Budget/Audit/Health/Jobs); the repo detail view has no per-spec meta-spec provenance display and no drift badges on commits (grep across web components finds no such surface).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/metrics.rs",
            "quote": "\"gyre_meta_spec_drift_total\",",
            "detail": "All three specced metrics are registered on the Prometheus registry (gyre_meta_spec_drift_total{workspace,repo}, gyre_reconciliation_tasks_total{workspace,status}, gyre_reconciliation_duration_seconds{workspace}) and recorded from production paths (record_drift_metrics / record_reconciliation_metrics / wave completion)."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "<span class=\"rule-version\">v{ms.version}</span>",
            "detail": "The workspace home's Agent Rules section renders the effective meta-spec set grouped by kind with version numbers and a reconcile-status banner — the partial realization of the workspace detail view; it carries no current/drifted indicator, no reconciliation progress bar, and no per-repo drift count (inspected the full section template)."
          },
          {
            "file": "web/src/components/TenantSettings.svelte",
            "quote": "Tabs: Users | Compute Targets | Budget | Audit | Health | Jobs",
            "detail": "The tenant administration surface has no meta-spec rollout content at all — no rollout plan status, per-workspace reconciliation progress, or total drift count (inspected the component; no reconciliation/drift/rollout references exist in it)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "async fn record_meta_spec_drift_warning(",
            "detail": "The merge-gate drift path only persists a GateResult (meta-spec-drift, Passed, [WARN] output) — the inspected function body performs no event emission, so MetaSpecDriftDetected never fires for MR-found drift, only for sweep-found drift (reconciliation.rs emits the custom meta_spec_drift_detected kind), contrary to the event table's 'MR or conformance sweep' trigger."
          }
        ]
      },
      {
        "id": "a854e91085ee72f68e6ca854",
        "decision": "gap",
        "requirement_quote": "- **Does not require re-implementation of everything.** Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped. Agents may produce no-op MRs for code that already conforms.",
        "detail": "Four of the five anti-requirements hold: meta-spec sets are never auto-updated on content change (bindings change only via the explicit set PUT or tenant rollout — no code path writes meta_spec_sets on registry update); spec iteration remains the spec lifecycle's job; the default rollout policy is Normal priority interleaved with feature work (reconciliation does not block feature work); deployment/production concerns are out of scope here. The violated anti-requirement is the quoted one: candidates are NOT limited to specs with provenance records under the old version, and unaffected repos are NOT skipped. run_reconciliation treats every repo in the workspace as a reconciliation candidate for each changed meta-spec (its doc comment states 'every repo in the workspace is a reconciliation candidate... the workspace binding governs all repos'), with dedup only against existing open tasks — no provenance query filters unaffected repos, so reconciliation task waves do require re-implementation work for repos with no code under the superseded version. (The no-op-MR clause itself is honored — the task contract allows no-op outcomes.)",
        "evidence": [
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "for repo in &repos {",
            "detail": "Candidate generation loops over ALL repos of the workspace (list_by_workspace) for each changed meta-spec; there is no provenance-based filter that would skip a repo with no code produced under the superseded version — the anti-requirement 'Unaffected repos are skipped' is not implemented on the reconciliation path."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "/// (the workspace binding governs all repos in the workspace — §2), and",
            "detail": "The controller's own doc comment codifies the divergence: 'For each, every repo in the workspace is a reconciliation candidate (the workspace binding governs all repos in the workspace — §2), and §6 step 2 requires a task per repo' — an explicit design choice that contradicts this section's 'Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped.'"
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "specs/system/agent-runtime.md",
        "sha256": "ba4c441a5379ec0315a539ab35083e0a594ee84be2bbbffc71a677b81a9d9dc5"
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "sha256": "bb05b89a43e4e750a70d446a6afc8f9a5e7c213e87433f9416d94bf3eb532108"
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "sha256": "033b2dd34cdcd2c436b4391868c33fb8b1db37adc9c71d3d87faf1061bcea3b6"
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "sha256": "f02c2ec7b852145682f0e9c356de0be3098e336954d48aa7c368cfb98b6b8000"
      },
      {
        "file": "crates/gyre-ports/src/meta_spec_set.rs",
        "sha256": "6086ce557a4270a4b664d047c786dea5fb0cc6fd14c6ec580fd55bec34b3e314"
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "sha256": "d0cd8b2b083925c18caca5ecbda2ad58b86d3e1662a5f4e1843e8fa7235976c3"
      },
      {
        "file": "crates/gyre-common/src/attestation.rs",
        "sha256": "553d0077d12eba5b623be0d6ebae2be835325d40577d65e8bb824521b9044419"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "f6b1f8cd14524591bdca669b2c5ee44c355c3b09ec53d6accea4e5b0e1ceb94c"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "a7e02b2014b6acda7c6bdc11f463ac2c6707d23f5b47cf6167025f8cdd80dfea"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "7172cd8e5748f5baa065ddca60b1fc8f45d369ed891767deb436ad0a9949c23e"
      },
      {
        "file": "docker/gyre-agent/agent-runner.mjs",
        "sha256": "f959c3508b7dcf94b44b480efc3614cd29580c02995f79c3771ef4c95ce96870"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "b38ff786cd1af7a60305965df54869d498c1dc977be645d0046124466709422a"
      },
      {
        "file": "web/src/components/MetaSpecs.svelte",
        "sha256": "2663b1810fb16143c352b9c0b371bf1ece2541fa4529025e5829c4077bd60616"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "922f3de73788564bd7c4b4ca48b065f2bba6149c7b651aaf108c02b667c559b4"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "e9b39651de638b5d06e807f033513af75a924e0f1227dd5e776ccae52e53d04d"
      },
      {
        "file": "crates/gyre-domain/src/meta_spec.rs",
        "sha256": "354f5db5c971cd6b59f1c543126d10fd5412f88e0f7e55b74d59546cfb678da4"
      },
      {
        "file": "crates/gyre-server/src/merge_policy.rs",
        "sha256": "0dd67390924df60ce7bc0992f9a92cc0ab684d5d158c67fd3fb9e5223083bdc7"
      },
      {
        "file": "web/src/components/WorkspaceSettings.svelte",
        "sha256": "852a9a4929d96852a45a1ae1e70afb8b620cb6e723506b8565454580641c13c3"
      },
      {
        "file": "crates/gyre-server/src/api/workspaces.rs",
        "sha256": "e26892e8549bf9eac6e5f66e91fa3e227d15a872eaf7ca614cdf224a09400e14"
      },
      {
        "file": "crates/gyre-server/src/metrics.rs",
        "sha256": "9421cb80d2ab16086895c835a70be53a9cef2e7e1b6cdca8dc1bda02dffdc401"
      },
      {
        "file": "web/src/components/WorkspaceHome.svelte",
        "sha256": "b589439a0c7061a509163fe09fa3e3e6a8b4b33e57b5aefcd997f6560f4cf0c3"
      },
      {
        "file": "web/src/components/TenantSettings.svelte",
        "sha256": "c6fa6e94a629583f89495cb82d7605b7153551716cfc44362332704183513806"
      }
    ]
  },
  "plan": {
    "audit": "571abb21992ae5d4697246c8e56d549d4048a0f5d20c6aae02fd8410d9d34062",
    "tasks": [
      {
        "key": "meta-spec-registry-parity",
        "title": "Meta-spec registry parity: per-kind approval modes (human_only enforced by kind, dual human_and_agent for meta:standard with kind-conditional agent admission), Repo scope, and link-graph participation",
        "covers": [
          "c2ad988fa981b60eae143fd1"
        ],
        "required_behavior": "Per meta-spec-reconciliation.md §1, meta-specs get the same SHA-pinned versioning, approval flow, and link graph as regular specs, with per-kind approval modes. REGISTRY-MODEL BASELINE (review-feedback correction, matching the revision-2 audit): agent-runtime.md §2 'DB-backed, not repo-backed' explicitly supersedes §1's git-backed registry semantics ('This supersedes the git-backed meta-spec model from meta-spec-reconciliation.md §1'), and its amendments-table row repeats 'Reconciliation spec defers to this for registry semantics'. The existing DB registry (meta_specs + meta_spec_versions tables, content-addressable SHA-256 hashes, immutable version history, DB-API-only writes) IS the amended model; accordingly the manifest-driven registration surface (yaml manifest declaring meta-specs, the auto_reconcile flag, meta-specs 'appearing in the spec registry alongside implementation specs') is SUPERSEDED, not a deliverable: do NOT build a manifest-to-registry bridge, do NOT add a manifest auto_reconcile flag, and leave SpecEntry.kind as vestigial declaration metadata. What remains missing against the amended model (the audit's three live sub-gaps): (1) Per-kind approval modes enforced BY KIND — today update_meta_spec_registry applies caller-supplied approval_status verbatim for every kind (meta_specs.rs:9171-9183 has no kind-conditional branch; kind is read only for event payloads and task titles). meta:persona, meta:principle, meta:process are human_only — only human principals may approve; meta:standard is human_and_agent with DUAL-approval semantics mirroring the spec-registry's resolve_approval_status (spec_registry.rs:301-326): the entry reaches approval_status Approved only when at least one valid human approval AND at least one valid agent approval of the SAME content (SHA-matched, approver-type-discriminated approval events) are recorded; a lone agent approval leaves the entry Pending exactly as a lone human approval does; Rejected remains a direct transition. Approval becomes event-based: the update path records typed approval events (approver_type human|agent, approved content hash) and derives approval_status per kind instead of assigning the caller's string verbatim; a single human PUT {approval_status: 'Approved'} on a human_only kind must remain observably identical to today (shipped UI flow preserved), and the existing reconciliation trigger on Approved (required-flag / approval handler) must fire exactly when the DERIVED status becomes Approved, never on a lone half-approval. (2) Agent admission — the human_and_agent mode must be REACHABLE without granting agents general registry write access: a kind-conditional widening of the approval step ONLY. An agent principal (UserRole::Agent) may record an approval event on a meta:standard entry; agent principals remain forbidden from every other registry mutation (content/name/scope/required writes → 403 with RBAC denied recorded, exactly as authorize_registry_write enforces today) and from approving human_only kinds (403). (3) Repo scope — parse_scope accepts Repo (scope_id = repo id) with the same containment rigor as Workspace scope (repo must exist and belong to the caller's tenant; writer must be tenant Admin or the repo's workspace Owner/Admin; cross-tenant writes 403); agent-runtime.md §2 preserves PersonaScope Repo as nearest-scope-wins overrides that 'cannot be marked required', so required=true on a Repo-scoped entry is rejected; scope-enum widening goes through a dialect-portable migration (both SQLite and PostgreSQL, per check-migration-sql-portability.sh) using the next unused sequence number. (4) Link-graph participation — meta-spec registry entries participate in the spec link graph: links may reference meta-specs as endpoints pinned to the meta-spec's content_hash, under the same SHA-pinning rule task-286 enforces for spec links (no link enters or stays without a target SHA); meta-spec links are listable via the links surface (e.g. GET /api/v1/meta-specs/:id/links); every new route gets an ABAC RouteResourceMapping entry (AGENTS.md route-registry invariant — unregistered routes get no policy evaluation). COORDINATION: task-286 (active, spec-links SHA pinning) reshapes the link store this task extends — build on its result, do not modify its contract. task-302 (active, bindings-registry-cascade) reshapes the consumers of the same PUT /api/v1/meta-specs/:id approval path this task makes kind-conditional (required-flag reconciliation trigger, meta_specs_used drift basis) — compose with its result: the dual-approval derivation and the required-flag trigger must work together (trigger fires on derived Approved), and this task must not reintroduce retired set stores.",
        "acceptance": [
          "Tenant Admin PUT {approval_status: 'Approved'} on a meta:persona → Approved (human_only behavior unchanged); the same PUT on a freshly edited meta:standard with no agent approval recorded → approval_status stays Pending (dual approval unmet) with the human approval event recorded and SHA-stamped.",
          "An agent principal records an approval on that meta:standard → with the human half present the entry becomes Approved and the existing reconciliation trigger fires for required entries; the same agent principal PUT {approval_status: 'Approved'} on a meta:persona → 403; the same agent principal PUT {prompt: '<new content>'} (content write) on the meta:standard → 403 — the widening covers the approval step of meta:standard only.",
          "parse_scope('Repo') is accepted with scope_id containment enforced: an Admin of another tenant cannot write a Repo-scoped entry for the target repo (403); required=true on a Repo-scoped entry is rejected (agent-runtime.md: repo-scoped entries cannot be marked required).",
          "A link whose endpoint is a meta-spec pins that meta-spec's content_hash; a meta-spec link without a target SHA is rejected; links are listable per meta-spec via an ABAC-mapped route (scripts/check-abac-route-registry.sh stays green with no new exemptions)."
        ],
        "negative_probe": "Arm 1 (missing kind discrimination — use a principal that passes the write gate, not a role-gate 403): as tenant Admin, edit a meta:standard's content then PUT {approval_status: 'Approved'}; GET must show approval_status Pending because no valid agent approval of the same content_hash exists. At the base this fails: the Admin's PUT flips the entry to Approved verbatim (meta_specs.rs:9171-9183 applies the caller's status for every kind — no kind-conditional branch exists anywhere in the update path). Arm 2 (unreachable human_and_agent): an agent principal PUT {approval_status: 'Approved'} on a meta:standard must record the agent approval half; at the base the same PUT returns 403 from authorize_registry_write's role gate before any kind discrimination runs — the defect it leaves exposed is that NO code path admits an agent principal to any approval transition, so the specced dual mode can never complete; after the change the same PUT succeeds on meta:standard while the same principal's {prompt} PUT and any approval PUT on human_only kinds still 403. Arm 3 (Repo scope absent): declaring a Repo-scoped meta-spec must succeed and required=true on it must be rejected; at the base parse_scope rejects Repo outright ('invalid scope; must be one of: Global, Tenant, Workspace', meta_specs.rs:8653) so the repo tier cannot even be declared.",
        "dependencies": [
          "task-286"
        ],
        "existing_task": null
      },
      {
        "key": "repo-scope-set-cascade-and-injection",
        "title": "§2 repo tier of the scope cascade: repo-scoped meta-specs as nearest-scope-wins name-resolution overrides, injected at spawn and recorded in provenance",
        "covers": [
          "815f40a82746fafd22856e09"
        ],
        "required_behavior": "Per §2, the scope resolution is nearest-scope-wins (repo overrides workspace overrides tenant). Under agent-runtime.md §2's amended injection model (the amendments-table row for meta-spec-reconciliation.md replaces the path@sha set as the injection surface with required flags + spec-level bindings; active task-302 implements that retirement), the repo tier means (review-feedback correction, matching the revision-2 audit): repo-scoped entries act as nearest-scope-wins NAME-RESOLUTION overrides per platform-model.md §2 — a reference/binding for a name resolves to the repo-scoped entry when one exists in the agent's repo — and they NEVER remove a required workspace/tenant injection. The semantics mandated by agent-runtime.md §2, which this task must implement and not contradict: (a) Repo-scoped entries cannot be marked required ('only tenant/workspace admins can enforce meta-specs across all agents'); (b) 'required meta-specs bypass scope resolution entirely — they are always included regardless of whether a lower-scope meta-spec exists with the same name'; (c) same-name collisions CONCATENATE — the agent receives BOTH the required entry (required section first) and the repo-scoped one, as separate prompt sections; dedup is only by meta_spec_id, never by name/path. COORDINATION (real ordering requirement, not serialization): active task-302 replaces the path@sha workspace/tenant meta-spec-set stores and compute_meta_spec_set_sha/resolve_effective_meta_spec_set with the DB-backed registry model (required flags + spec-level bindings; {meta_spec_id, kind, content_hash, version} tuples); this task delivers §2's repo tier on the model present AFTER task-302 and must never reintroduce retired stores. Deliverables: (1) Repo-tier resolution — when an agent is spawned for a task in a repo, a name referenced by a spec-level binding (or persona slot) that has a Repo-scoped entry in that repo resolves to the repo-scoped entry's approved version; sibling repos without the override keep resolving to the workspace/tenant entry; required Global/Tenant/Workspace entries are injected unchanged regardless of same-name repo entries. (2) Injection — spawn-time prompt assembly injects the repo-resolved content at the governing version in the existing kind order (persona → principle → standard → process), deduplicated only by meta_spec_id so the same meta-spec@version is never injected twice while same-name different-id entries both appear; unresolvable pins (a declared repo-scoped entry whose version/content cannot be loaded) fail the spawn loudly with an error naming the entry — never silently skipped (no fail-open resolve). (3) Provenance correctness — the meta_specs_used recorded for an agent in a repo with overrides reflects the versions actually injected (id/kind/content_hash/version), so downstream drift/tolerance reasoning is per-repo correct rather than misjudging every override as drift. (4) API + authorization — the repo-level surface is writable/readable (Repo-scope registry entries per meta-spec-registry-parity, plus any repo-scoped binding surface) with Admin/Owner gating and tenant containment mirroring the workspace surface; new routes registered with ABAC RouteResourceMapping entries.",
        "acceptance": [
          "Workspace-scoped required meta-spec 'coding-standards' at v1 (id X, marker A); a Repo-scoped 'coding-standards' v2 (different id Y, marker B) exists for repo R; a spec in R binds the name: an agent spawned for a task in R has BOTH markers in its system prompt (v1 in the required section, v2 via the repo-resolved binding section) and its prompt-set record's meta_specs_used carries both entries (X@v1 and Y@v2); an agent spawned for a task in sibling repo R2 (no override) receives marker A only.",
          "When a repo-resolved entry and another injected entry are the same meta_spec_id at the same version, the entry is injected once (dedup by meta_spec_id only); required=true on a Repo-scoped entry is rejected (never requiredable).",
          "An Admin of a different tenant cannot declare or write a Repo-scoped entry for R (403, containment enforced); a non-admin member of R's workspace cannot either.",
          "A spec-level binding referencing a name with a Repo-scoped entry resolves to the repo-scoped version for agents in R and to the workspace/tenant version for agents in R2 — nearest-scope-wins name resolution observable per repo; unresolvable repo-scoped pins fail the spawn loudly with an error naming the entry."
        ],
        "negative_probe": "Build the override scenario (workspace-required 'coding-standards' v1 with marker A; Repo-scoped 'coding-standards' v2 with marker B for repo R; a spec in R binding the name) and spawn an agent for a task in R; assert the prompt-set record (kv 'agent_meta_spec_sets') / GYRE_META_SPEC_PROMPT contains BOTH markers — A in the required section and B via the repo-resolved binding — and meta_specs_used records both ids at their respective versions; assert the same spawn in sibling repo R2 shows marker A only. At the base this fails twice over: parse_scope rejects Repo outright ('invalid scope; must be one of: Global, Tenant, Workspace', meta_specs.rs:8653) so the override cannot even be declared, and prompt_assembly resolves only required Global/Tenant/Workspace entries plus spec-level bindings (dedup keyed by meta_spec_id) — it never consults any repo-level layer — so no repo-resolved section can appear, proving the repo tier of the §2 cascade is absent. The probe deliberately asserts CONCATENATION, not replacement: an implementation where the repo-scoped v2 replaces the required v1 in the prompt would itself fail the probe, because agent-runtime.md §2 'Same-name collision' mandates both sections (required first) and 'required meta-specs bypass scope resolution entirely'.",
        "dependencies": [
          "task-302",
          "meta-spec-registry-parity"
        ],
        "existing_task": null
      },
      {
        "key": "blast-radius-provenance",
        "title": "Blast radius computed from provenance: version-discriminated affected workspaces plus per-repo spec/commit counts from real attestation queries",
        "covers": [
          "3662e9dd04c597d5db7d61f0"
        ],
        "required_behavior": "Per §4, the blast radius is a database query over provenance records + bindings — computed, not guessed. Today GET /api/v1/meta-specs/:path/blast-radius (meta_specs.rs:650-745) does a path-membership check on effective set pins only: no provenance store is queried, there are no per-repo spec/commit counts, and no version discrimination (a workspace pinned to any version counts as affected; the spec's 'bound to v2, skipped' case is not skipped). COORDINATION: active task-302 replaces the path@sha set-binding basis with the registry model (required flags + spec-level bindings, versioned tuples); deliver the §4 behavior on the model present after task-302. Deliverables: (1) Version discrimination — the blast-radius request carries (or the registry supplies) the change context: the superseded version/SHA of the changed meta-spec; a workspace is affected when its effective binding requires/pins the changed meta-spec at the SUPERSEDED version; workspaces bound to other versions are reported as skipped/not-affected with their pinned version, matching the spec's example (internal-tools on v2 is skipped for a v3→v4 change). (2) Per-repo counts from provenance — for each affected workspace, per repo: the count of distinct implementation specs with provenance produced under the superseded version (distinct spec identity in provenance records — chain/merge attestations, agent-commit provenance — whose recorded governing meta-spec versions include the superseded version of the changed meta-spec) and the count of commits under it; the response carries per-workspace/per-repo spec and commit counts plus totals (repos, specs, commits), all computed from real storage queries. (3) The existing workspace_binding vs tenant_inheritance reason labels are preserved (§8 amendment). (4) The endpoint stays read-only and ABAC-mapped; counts never fabricated — a repo with no provenance under the superseded version contributes zero/omitted even though it belongs to an affected workspace.",
        "acceptance": [
          "Workspace A requiring the changed meta-spec at v3 (provenance: 2 distinct specs, N commits across 2 repos) and workspace B bound at v2: GET blast-radius for the v3→v4 change reports A affected with per-repo spec/commit counts matching the seeded provenance and correct totals; B appears as skipped/not-affected with pinned v2 and is excluded from totals.",
          "A repo in A with no provenance under v3 contributes no counts and is not listed among affected repos with work — counts derive from provenance records, not workspace repo membership.",
          "Inherited bindings are still labeled tenant_inheritance and own bindings workspace_binding, with counts computed identically for both."
        ],
        "negative_probe": "Seed the A/B scenario and call GET /api/v1/meta-specs/{path}/blast-radius for the v3→v4 change; assert B is absent from affected workspaces and A's repos carry spec/commit counts equal to the seeded provenance. At the base both assertions fail: the handler's entire affected-workspace test is a path-membership check (effective.personas.values().any(|e| e.path == spec_path) || … over principles/standards/process) so B — which pins the path at v2 — is reported affected exactly like A, and the handler's only data sources are workspaces.list, the set stores, and repos.list_by_workspace (no chain_attestations, attestation_store, or agent-commit query anywhere in the body), so no spec or commit count can be produced at all.",
        "dependencies": [
          "task-302"
        ],
        "existing_task": null
      },
      {
        "key": "reconciliation-candidate-scoping",
        "title": "Reconciliation candidates scoped by provenance with per-spec task identity (spec_ref/meta_spec_ref/reconciliation_id) and reconcile/{id}/{spec} branches",
        "covers": [
          "649ed86870daac6de2032d54",
          "a854e91085ee72f68e6ca854"
        ],
        "required_behavior": "Per §6 Flow and the 'What This Does NOT Do' anti-requirement 'Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped.' Today run_reconciliation iterates EVERY repo in the workspace for each changed meta-spec (for repo in &repos over list_by_workspace, dedup only against open tasks) and the created task's spec_path is the changed META-SPEC's own name (reconciliation.rs:591, spec_path: ms.name.clone()). Deliverables: (1) Provenance-scoped candidates — the controller identifies candidates via a provenance query (the query designed in blast-radius-provenance): only implementation specs with provenance records produced under the superseded version of the changed meta-spec become candidates; repos with no such provenance are skipped entirely (no task created for them). (2) Per-spec task shape — one task per affected (repo, spec): title 'Reconcile: {spec_path} under {meta_spec}@{new_sha}'; labels ['reconciliation', 'auto-created'] preserved; the task carries spec_ref = '{spec_path}@{current_sha}' (the AFFECTED implementation spec at its current pinned SHA, resolved from the spec ledger/attestation — not the meta-spec's name), meta_spec_ref = '{meta_spec_path}@{new_sha}', and a reconciliation_id field (Task struct + persistence extension, real fields not just labels) grouping every task in the wave; existing wave labels may remain for backward compatibility. (3) Reconciliation branches — agents spawned for reconciliation tasks work on branches named reconcile/{reconciliation-id}/{spec-slug} (wired through the spawn/worktree provisioning path; today agents get generic per-agent branches and the only 'reconcile/' string in the tree is a test fixture, reconciliation.rs:3407). (4) Preserve the satisfied machinery — §7 strategy dispatch (Immediate all-at-once, Rolling batches with kv-backed restart-safe pending state advanced on terminal status, Background tasks inert in Backlog), ReconciliationPriority mapping, auto_merge enqueue at agent completion, dedup (now per repo+spec identity, still only non-terminal tasks suppress), §8 tenant waves (one wave per non-overriding workspace, override/unreadable-set skips, force semantics), the no-op-MR validity contract in the task description, and the §6 trigger gating on Approved entries. COORDINATION: task-302 changes the binding/trigger basis (required flags + spec-level bindings instead of path@sha set diffs); the candidate-scoping, per-spec identity, and branch requirements are model-independent and must survive that migration.",
        "acceptance": [
          "Workspace with repo R1 (spec S has provenance under the superseded version) and repo R2 (no provenance at all): after the binding update to the new version, tasks exist for (R1, S) — carrying spec_ref 'S@{current sha}', meta_spec_ref '{meta-spec}@{new sha}', and a shared reconciliation_id — and NO task exists for R2.",
          "The agent spawned for the (R1, S) task works on a branch named reconcile/{reconciliation-id}/{spec-slug}.",
          "Rolling strategy still batches: with max_concurrent_agents = 1 and two affected specs, the first task is created at trigger and the second only after the first reaches terminal state (kv-backed, restart-safe).",
          "The task description retains the re-implement directive including no-op validity ('no-op outcome (code already conforms) is valid').",
          "A workspace whose only repos lack provenance under the superseded version produces zero tasks (unaffected repos skipped) while a provenance-bearing workspace in the same tenant wave produces its per-spec tasks."
        ],
        "negative_probe": "Set up R1 (spec S with provenance under the old version) and R2 (no provenance), trigger the reconciliation wave, then assert: a task exists for (R1, S) with spec_ref/meta_spec_ref/reconciliation_id fields, its agent's branch matches reconcile/{id}/{s}, and R2 has no task. At the base every assertion fails: the controller creates a task for EVERY repo — R2 included — whose spec_path is the changed meta-spec's own registry name (reconciliation.rs:591, spec_path: ms.name.clone()), the Task struct has no meta_spec_ref or reconciliation_id fields (waves are grouped only by labels), and spawned agents get standard per-agent branches (no reconcile/ prefix exists in production code; the only occurrence is a test fixture at reconciliation.rs:3407) — the base behavior violates the NOT-DO anti-requirement by requiring re-implementation work from repos with no code under the superseded version.",
        "dependencies": [
          "blast-radius-provenance"
        ],
        "existing_task": null
      },
      {
        "key": "merge-gate-drift-policy",
        "title": "§9 MetaSpecPolicy in full: warn_on_drift/block_on_drift/drift_tolerance with per-meta-spec version-count reasoning, real server-side policy storage, and version-named drift warnings",
        "covers": [
          "d0c7637a52d62fe7a81b940e"
        ],
        "required_behavior": "Per §9, the merge-gate drift policy is the MetaSpecPolicy struct, not a single boolean — agent-runtime.md's amendments-table row explicitly retains it ('The MetaSpecPolicy (§9 of reconciliation spec) is retained — it controls merge-time drift enforcement and is stored as a workspace-level setting. Drift detection compares the meta_specs_used array in merge attestations against current required meta-spec versions.'). Today the enforced policy is require_current_meta_spec_set (merge_policy.rs) — strict mode blocks on any set-SHA mismatch (tolerance-0 only), no version-count reasoning exists anywhere, and the workspace settings UI renders the full specced policy and saves it via api.updateWorkspace({meta_spec_policy}) but UpdateWorkspaceRequest has no such field, so serde silently drops the payload and GET never returns it — the §9 policy UI is theater. COORDINATION: active task-302 migrates merge-time drift comparison to the meta_specs_used array (already recorded per merge in MergeAttestation from the authoring agent's prompt-set record — MetaSpecUsed {id, kind, content_hash, version}); build the policy on that versioned basis. Deliverables: (1) Policy shape — MetaSpecPolicy {warn_on_drift: bool = true, block_on_drift: bool = false, drift_tolerance: u32 = 1} persisted per workspace; migration/compat maps existing require_current_meta_spec_set: true to block_on_drift: true with drift_tolerance: 0 (preserving today's block-on-any-drift behavior for existing strict workspaces); both the merge-policy endpoint and the workspace settings surface expose the full shape. (2) warn_on_drift — false suppresses the [WARN] meta-spec-drift gate result (default true keeps today's always-warn); in strict mode (block_on_drift: true) warnings are always recorded regardless of warn_on_drift ('Always on in strict mode'). (3) drift_tolerance version-count reasoning — the merge-queue drift check compares the MR's provenance-recorded governing versions (meta_specs_used: id + version) against the current registry versions per meta-spec: versions_behind = current_version − recorded_version; the MR blocks only when some meta-spec is MORE than drift_tolerance versions behind (0 = block on any drift); within tolerance → warn and proceed. Legacy records with no per-meta-spec versions (hash-only) cannot prove currency: under block_on_drift they are treated as unverifiable and blocked (never fail-open); under warn they warn. (4) Warning naming — the [WARN] output and block reason name the per-meta-spec version transition(s) ('meta-spec-drift: {kind} {name} v{old} -> v{new}') derived from meta_specs_used vs current versions, not only set SHAs. (5) UI wiring fixed — UpdateWorkspaceRequest gains meta_spec_policy (warn_on_drift/block_on_drift/drift_tolerance, tolerance validated ≥ 0), persisted and returned by GET workspace, so the existing WorkspaceSettings save round-trips for real.",
        "acceptance": [
          "PUT /api/v1/workspaces/{id} with {meta_spec_policy: {warn_on_drift: true, block_on_drift: true, drift_tolerance: 1}} then GET the workspace returns exactly that policy (no silent drop); invalid tolerance (< 0) is rejected 400.",
          "With that policy, an MR whose authoring agent's meta_specs_used records a persona at v3 where the registry is at v4 (one version behind) merges with a [WARN] gate result naming 'persona … v3 -> v4'; an MR recorded at v2 (two versions behind) is blocked and requeued with the drift reason.",
          "A workspace migrated from require_current_meta_spec_set: true blocks a one-version-behind MR (tolerance 0 compatibility); a default workspace (no policy) warns and merges on any drift.",
          "warn_on_drift: false (non-strict) suppresses the [WARN] gate result; warn_on_drift: false with block_on_drift: true still records the warning alongside the block."
        ],
        "negative_probe": "Arm 1 (round-trip): PUT the workspace with meta_spec_policy then GET it — at the base the field is silently discarded (UpdateWorkspaceRequest has no meta_spec_policy field; grep finds no meta_spec_policy anywhere in Rust) while the web UI (WorkspaceSettings.svelte:131-143,899) renders and saves it, so GET returns nothing and the probe fails. Arm 2 (tolerance semantics): with block_on_drift + drift_tolerance 1, enqueue the one-version-behind MR and assert it merges with a [WARN]; at the base the strict boolean (require_current_meta_spec_set) blocks BOTH the one-behind and two-behind MRs on any set-SHA mismatch — tolerance cannot be expressed anywhere in the enforcement path (merge_policy.rs holds a single bool; no version-count reasoning exists), so the one-version-behind MR is wrongly blocked and the probe fails.",
        "dependencies": [
          "task-302"
        ],
        "existing_task": null
      },
      {
        "key": "conformance-sweep-convergence",
        "title": "Conformance sweep convergence: unbounded provenance comparison, per-spec drift detection and task creation, and rollout-policy-governed work on the drift path",
        "covers": [
          "8dc04f90bec1c77e6b42f252",
          "0f67c6dbc5bb328a54047724"
        ],
        "required_behavior": "Per §10 and the §6 controller clause 'When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.' The sweep job exists (meta_spec_conformance_sweep, daily, configurable via GYRE_META_SPEC_SWEEP_INTERVAL_SECS) but: (a) the provenance comparison is windowed to 30 days (SWEEP_PROVENANCE_WINDOW_SECS = 30*86400 at reconciliation.rs:119, feeding find_by_repo(repo, since, now) at :1729) so drift older than the window is permanently invisible, defeating the section's convergence purpose ('a workspace binding was updated while agents were offline'); (b) it compares per repo (any mismatched attestation flags the whole repo) and creates one workspace-level drift-review task rather than iterating per spec implemented in each repo; (c) the sweep-created task is hardcoded TaskPriority::Medium and created immediately — the sweep body (reconciliation.rs:1697-1860) never calls get_workspace_rollout_policy, so drift-caught work bypasses the §7 rollout policy that only the trigger path honors. Deliverables: (1) Unbounded comparison — remove the 30-day window: the sweep examines ALL provenance records per repo (every attestation regardless of age), still database queries only with no agent spawns. (2) Per-spec comparison — iterate per (repo, spec): group provenance by spec identity; a spec is drifted when its MOST RECENT provenance record's governing meta-spec versions differ from the current effective governing collection (desired state); replace the whole-repo flag and single workspace-level drift-review task with per-spec drift state. (3) Per-spec reconciliation work — for each drifted spec, ensure a reconciliation task exists (create if missing, deduped per repo+spec against open reconciliation tasks), carrying the per-spec task identity from reconciliation-candidate-scoping (spec_ref/meta_spec_ref/reconciliation_id). (4) Rollout-policy conformance — work created by the continuous comparison goes through the same policy dispatch as trigger-path waves: strategy handling (Immediate all-at-once, Rolling batches via the kv-backed pending state, Background left inert in Backlog without a spawning task_type), priority from the policy's ReconciliationPriority mapping (no hardcoded Medium), auto_merge handling identical — the sweep path reads get_workspace_rollout_policy. COORDINATION: active task-302 migrates the sweep's desired-vs-actual basis to the registry model (required versions / meta_specs_used tuples); the window, per-spec, and policy requirements are model-independent and must survive that migration. When a repo-level override governs a repo (repo-scope-set-cascade-and-injection), the desired state for that repo's specs is the repo-effective collection.",
        "acceptance": [
          "A repo whose newest attestation for spec S carries a superseded governing version with created_at 40 days in the past is flagged drifted by the sweep and gets a reconciliation task for S — the 30-day window no longer hides it.",
          "A repo with spec S1 drifted and spec S2 current yields drift state for S1 only and a task scoped to S1's spec_ref — not a whole-repo flag or a workspace-level review task.",
          "A workspace with rollout strategy Background: sweep-created drift work sits in Backlog without a spawning task_type and with the policy-mapped priority; with strategy Rolling and max_concurrent_agents 1, drift work for two specs is batched; the sweep path provably reads the workspace's rollout policy (priority comes from ReconciliationPriority, never a hardcoded constant).",
          "The sweep still spawns no agents and emits MetaSpecDriftDetected / records drift metrics for the per-spec findings."
        ],
        "negative_probe": "Three arms, all failing at the base. (1) Stale drift: seed a repo whose newest attestation for spec S records a superseded governing version with created_at 40 days ago (fixture clock); run the sweep and assert S is flagged and a task ensured — at the base the query is bounded (let since = now.saturating_sub(SWEEP_PROVENANCE_WINDOW_SECS) feeding find_by_repo(repo, since, now), reconciliation.rs:1729), so the record is invisible and no work is created. (2) Per-spec granularity: with S1 drifted and S2 current in one repo, assert drift state exists for S1 only — at the base any mismatched attestation flags the entire repo and one workspace-level drift-review task is created regardless of which spec drifted. (3) Policy: with the workspace's strategy set to Background, assert sweep-created work is inert in Backlog — at the base the sweep creates its task immediately with task.priority = TaskPriority::Medium hardcoded in the sweep body, which never calls get_workspace_rollout_policy, so policy-less work is the observable defect.",
        "dependencies": [
          "task-302",
          "repo-scope-set-cascade-and-injection",
          "reconciliation-candidate-scoping"
        ],
        "existing_task": null
      },
      {
        "key": "meta-spec-observability-surfaces",
        "title": "§11 observability completion: MetaSpecDriftDetected on the merge-gate path plus workspace/tenant/repo dashboard surfaces (drift indicators, reconciliation progress, per-spec provenance, commit badges)",
        "covers": [
          "4ec94a54317e88be7eb5b60b"
        ],
        "required_behavior": "Per §11. Events and metrics are largely real; two areas are missing. (1) Event completion — the merge-gate drift path emits MetaSpecDriftDetected: record_meta_spec_drift_warning (merge_processor.rs:802, called at :4730 and :11110) and the strict-mode block path emit the same event/message kind the conformance sweep already emits, so the event fires for BOTH 'MR or conformance sweep' triggers per the §11 table (today an MR found drifted records only a [WARN] GateResult and no event is emitted). (2) Dashboard surfaces, served by real data: (a) workspace detail view — the workspace surface's Agent Rules section gains current/drifted version indicators per governing entry (governing version vs registry current — drifted badge when they differ), a reconciliation progress bar (per active wave: done/total tasks grouped by reconciliation_id), and drift count per repo (from the sweep's per-repo drift state); (b) tenant admin view — the tenant administration surface gains the rollout/reconciliation view: rollout plan status, per-workspace reconciliation progress, and total drift count across the tenant's workspaces; (c) repo detail view — per-spec meta-spec provenance (the governing versions recorded for each spec's most recent provenance record) and drift badges on commits (the commit's attested governing versions vs current). Backend support: drift/reconciliation state is served through real endpoints (workspace drift-status / reconciliation-progress and repo-level provenance views), each registered with an ABAC RouteResourceMapping entry (AGENTS.md route-registry invariant — unregistered routes get no policy evaluation); all numbers derive from the sweep's drift state, wave tasks, and provenance records — nothing fabricated. Metrics (gyre_meta_spec_drift_total, gyre_reconciliation_tasks_total, gyre_reconciliation_duration_seconds) already exist and must keep recording from these paths.",
        "acceptance": [
          "Driving the merge queue with a drifted MR (attested governing version superseded) records a MetaSpecDriftDetected event/message in addition to the [WARN] GateResult — the same kind the sweep emits.",
          "A workspace whose sweep flagged repo R drifted renders a drifted indicator on the affected governing entry and a per-repo drift count ≥ 1 for R; entries at current versions show current indicators; the reconciliation progress bar reflects done/total wave tasks grouped by reconciliation_id.",
          "The tenant admin view shows per-workspace reconciliation progress and a total drift count across the tenant's workspaces; the repo detail view shows per-spec governing-version provenance and drift badges on commits whose attested versions are superseded.",
          "Every new endpoint is present in the ABAC RouteResourceMapping registry (scripts/check-abac-route-registry.sh passes with no new exemptions)."
        ],
        "negative_probe": "Arm 1 (event): enqueue a drifted MR through the merge queue and assert a MetaSpecDriftDetected event is recorded — at the base record_meta_spec_drift_warning's body persists only a GateResult (meta-spec-drift, Passed, [WARN]) and emits no event; only the sweep path emits the kind, so the MR-triggered half of the §11 event table never fires and the probe fails. Arm 2 (dashboard): with a sweep-flagged drifted repo, assert the workspace view renders the drifted indicator and per-repo drift count and the tenant admin view shows reconciliation progress and total drift — at the base the Agent Rules section renders only version numbers (rule-version spans) with no drift state, TenantSettings contains no reconciliation/drift/rollout content at all (its tabs are Users/Compute Targets/Budget/Audit/Health/Jobs), and no repo-detail per-spec provenance or commit drift badge exists anywhere under web/src — the component-level assertions fail.",
        "dependencies": [
          "reconciliation-candidate-scoping",
          "merge-gate-drift-policy",
          "conformance-sweep-convergence"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "meta-spec-registry-parity": "task-325",
    "repo-scope-set-cascade-and-injection": "task-326",
    "blast-radius-provenance": "task-327",
    "reconciliation-candidate-scoping": "task-328",
    "merge-gate-drift-policy": "task-329",
    "conformance-sweep-convergence": "task-330",
    "meta-spec-observability-surfaces": "task-331",
    "_publication": "task-332",
    "_base": "52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
