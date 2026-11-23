---
title: "§2 repo tier of the scope cascade: repo-scoped meta-specs as nearest-scope-wins name-resolution overrides, injected at spawn and recorded in provenance"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / repo-scope-set-cascade-and-injection / b288723d2f73"
depends_on: [task-302, task-325]
progress: not-started
commits: []
---

## Required behavior

Per §2, the scope resolution is nearest-scope-wins (repo overrides workspace overrides tenant). Under agent-runtime.md §2's amended injection model (the amendments-table row for meta-spec-reconciliation.md replaces the path@sha set as the injection surface with required flags + spec-level bindings; active task-302 implements that retirement), the repo tier means (review-feedback correction, matching the revision-2 audit): repo-scoped entries act as nearest-scope-wins NAME-RESOLUTION overrides per platform-model.md §2 — a reference/binding for a name resolves to the repo-scoped entry when one exists in the agent's repo — and they NEVER remove a required workspace/tenant injection. The semantics mandated by agent-runtime.md §2, which this task must implement and not contradict: (a) Repo-scoped entries cannot be marked required ('only tenant/workspace admins can enforce meta-specs across all agents'); (b) 'required meta-specs bypass scope resolution entirely — they are always included regardless of whether a lower-scope meta-spec exists with the same name'; (c) same-name collisions CONCATENATE — the agent receives BOTH the required entry (required section first) and the repo-scoped one, as separate prompt sections; dedup is only by meta_spec_id, never by name/path. COORDINATION (real ordering requirement, not serialization): active task-302 replaces the path@sha workspace/tenant meta-spec-set stores and compute_meta_spec_set_sha/resolve_effective_meta_spec_set with the DB-backed registry model (required flags + spec-level bindings; {meta_spec_id, kind, content_hash, version} tuples); this task delivers §2's repo tier on the model present AFTER task-302 and must never reintroduce retired stores. Deliverables: (1) Repo-tier resolution — when an agent is spawned for a task in a repo, a name referenced by a spec-level binding (or persona slot) that has a Repo-scoped entry in that repo resolves to the repo-scoped entry's approved version; sibling repos without the override keep resolving to the workspace/tenant entry; required Global/Tenant/Workspace entries are injected unchanged regardless of same-name repo entries. (2) Injection — spawn-time prompt assembly injects the repo-resolved content at the governing version in the existing kind order (persona → principle → standard → process), deduplicated only by meta_spec_id so the same meta-spec@version is never injected twice while same-name different-id entries both appear; unresolvable pins (a declared repo-scoped entry whose version/content cannot be loaded) fail the spawn loudly with an error naming the entry — never silently skipped (no fail-open resolve). (3) Provenance correctness — the meta_specs_used recorded for an agent in a repo with overrides reflects the versions actually injected (id/kind/content_hash/version), so downstream drift/tolerance reasoning is per-repo correct rather than misjudging every override as drift. (4) API + authorization — the repo-level surface is writable/readable (Repo-scope registry entries per meta-spec-registry-parity, plus any repo-scoped binding surface) with Admin/Owner gating and tenant containment mirroring the workspace surface; new routes registered with ABAC RouteResourceMapping entries.

## Acceptance Criteria

- [ ] Workspace-scoped required meta-spec 'coding-standards' at v1 (id X, marker A); a Repo-scoped 'coding-standards' v2 (different id Y, marker B) exists for repo R; a spec in R binds the name: an agent spawned for a task in R has BOTH markers in its system prompt (v1 in the required section, v2 via the repo-resolved binding section) and its prompt-set record's meta_specs_used carries both entries (X@v1 and Y@v2); an agent spawned for a task in sibling repo R2 (no override) receives marker A only.
- [ ] When a repo-resolved entry and another injected entry are the same meta_spec_id at the same version, the entry is injected once (dedup by meta_spec_id only); required=true on a Repo-scoped entry is rejected (never requiredable).
- [ ] An Admin of a different tenant cannot declare or write a Repo-scoped entry for R (403, containment enforced); a non-admin member of R's workspace cannot either.
- [ ] A spec-level binding referencing a name with a Repo-scoped entry resolves to the repo-scoped version for agents in R and to the workspace/tenant version for agents in R2 — nearest-scope-wins name resolution observable per repo; unresolvable repo-scoped pins fail the spawn loudly with an error naming the entry.

## Verification intent

Build the override scenario (workspace-required 'coding-standards' v1 with marker A; Repo-scoped 'coding-standards' v2 with marker B for repo R; a spec in R binding the name) and spawn an agent for a task in R; assert the prompt-set record (kv 'agent_meta_spec_sets') / GYRE_META_SPEC_PROMPT contains BOTH markers — A in the required section and B via the repo-resolved binding — and meta_specs_used records both ids at their respective versions; assert the same spawn in sibling repo R2 shows marker A only. At the base this fails twice over: parse_scope rejects Repo outright ('invalid scope; must be one of: Global, Tenant, Workspace', meta_specs.rs:8653) so the override cannot even be declared, and prompt_assembly resolves only required Global/Tenant/Workspace entries plus spec-level bindings (dedup keyed by meta_spec_id) — it never consults any repo-level layer — so no repo-resolved section can appear, proving the repo tier of the §2 cascade is absent. The probe deliberately asserts CONCATENATION, not replacement: an implementation where the repo-scoped v2 replaces the required v1 in the prompt would itself fail the probe, because agent-runtime.md §2 'Same-name collision' mandates both sections (required first) and 'required meta-specs bypass scope resolution entirely'.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
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
  }
]
