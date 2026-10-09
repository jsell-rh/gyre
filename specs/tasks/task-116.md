---
title: "Implement meta-spec prompt assembly"
spec_ref: "agent-runtime.md §2 Meta-Spec Prompt Assembly"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-runtime.md §2. Meta-Spec Prompt Assembly"
  - "agent-runtime.md §Meta-Specs Are Prompts"
  - "agent-runtime.md §Registry Levels"
  - "agent-runtime.md §Required vs Optional"
  - "agent-runtime.md §Spec-Level Binding"
  - "agent-runtime.md §Injection Order"
  - "agent-runtime.md §Versioning and Attestation"
  - "agent-runtime.md §Stale Pin Detection"
  - "agent-runtime.md §Bootstrap"
  - "agent-runtime.md §API"
commits: ["bcfd79cdc4c6efddae90ff2b918e344be2961bc2", "53516e392bb70a6732586f5c0d64500a1330eb50", "ddddf2e5abacdf888b354c62a66a85c9b112914f", "0de2143486e9782de2ae213519e77d613508fe8f"]
---

## Spec Excerpt

From `agent-runtime.md` §2:

**Meta-Specs Are Prompts.** Personas, principles, standards, process norms are instructions passed to agents. The meta-spec registry IS the prompt configuration.

**Registry Levels:** Tenant registry (org-wide) → Workspace registry (team) → Spec-level bindings. Each entry has: `prompt`, `version` (auto-incremented), `content_hash` (SHA-256), `required` (bool).

**Required vs Optional:** Required meta-specs always injected into every agent. Optional ones selected via spec-level bindings. Only scope-level admins can set `required`.

**Injection Order:**
1. All REQUIRED tenant meta-specs (ordered by kind: persona → principle → standard → process)
2. All REQUIRED workspace meta-specs (same ordering)
3. Spec-level bindings (at pinned versions)

**Versioning:** Each edit creates new version with new content_hash. Old versions retained in `meta_spec_versions` table. Specs pin specific versions.

**Stale Pin Detection:** Background job detects when a spec pins an old meta-spec version. Creates Inbox priority 6 notification.

**Bootstrap:** Default meta-specs seeded at first startup (default-worker, workspace-orchestrator, repo-orchestrator, spec-reviewer, accountability, security, conventional-commits, reconciliation, test-coverage).

**API:** Flat routes at `/api/v1/meta-specs` with scope/scope_id filtering. Full CRUD plus version history.

## Implementation Plan

1. **Extend meta-spec domain model:**
   - Add `MetaSpec` struct for non-persona kinds (Principle, Standard, Process) in `gyre-domain`
   - Add `MetaSpecKind` enum: `Persona`, `Principle`, `Standard`, `Process`
   - Add version tracking fields to existing Persona model: `version: u32`, `required: bool`
   - Add `MetaSpecVersion` struct for version history
   - Add `SpecBinding` struct for spec-level meta-spec bindings

2. **Port traits:**
   - Extend `PersonaRepository` or create `MetaSpecRepository` for unified CRUD across all kinds
   - `MetaSpecVersionRepository` — store/retrieve version history
   - `SpecBindingRepository` — manage spec-to-meta-spec bindings

3. **Database migrations:**
   - `meta_spec_versions` table (id, meta_spec_id, version, content, content_hash, created_at)
   - `spec_bindings` table (spec_path, repo_id, meta_spec_id, pinned_version)
   - Add `version`, `required`, `kind` columns to existing personas table (or create unified meta_specs table)

4. **Prompt assembler:**
   - `PromptAssembler` service in `gyre-server` that builds the full prompt set for an agent
   - Input: task_id (to resolve spec_ref → spec bindings), workspace_id
   - Steps: collect required tenant meta-specs → required workspace meta-specs → spec-level bindings at pinned versions
   - Deduplication: if required meta-spec appears in spec bindings, include only once
   - Output: ordered list of prompt sections

5. **Wire into agent spawning:**
   - In `spawn_agent`, call `PromptAssembler` to build the prompt set
   - Include meta-spec SHAs in merge attestation bundle

6. **Stale pin detection:**
   - Background job or on-access check: compare spec's pinned version against current version
   - Create priority-6 notification on mismatch

7. **Bootstrap seeding:**
   - On first startup (empty meta-specs table), seed the 9 default meta-specs from spec

8. **API endpoints** at `/api/v1/meta-specs`:
   - Full CRUD with scope/scope_id filtering
   - Version history endpoints
   - Register routes and ABAC mappings

## Acceptance Criteria

- [ ] `MetaSpec` entity supports Persona, Principle, Standard, Process kinds
- [ ] Version tracking: each edit creates new version, old versions retained
- [ ] Spec-level binding: specs can pin meta-specs at specific versions
- [ ] `PromptAssembler` builds ordered prompt set (required tenant → required workspace → spec bindings)
- [ ] Deduplication of required + bound meta-specs
- [ ] Stale pin detection creates notifications
- [ ] 9 default meta-specs seeded at bootstrap
- [ ] Full CRUD API at `/api/v1/meta-specs` with version history
- [ ] Meta-spec SHAs recorded in merge attestation
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §2 (Meta-Spec Prompt Assembly) in its entirety. The existing Persona model is in `gyre-domain/src/` — grep for `Persona`. Meta-spec API stubs may exist in `gyre-server/src/api/meta_specs.rs`. The persona scope enum is in `gyre-domain`. Agent spawn is in `gyre-server/src/api/spawn.rs`. MCP prompt delivery is in `gyre-server/src/mcp.rs` — look for `system://persona`. Bootstrap seeding patterns: grep for `seed` or `bootstrap` in the server startup code.

## Shipped

- **Prompt assembly pipeline** (`crates/gyre-server/src/prompt_assembly.rs`):
  required tenant (Global) → required workspace → spec-level bindings at
  pinned versions, kind-ordered (persona → principle → standard → process)
  and deduped by meta_spec_id; pinned versions resolved from the immutable
  `meta_spec_versions` history, unresolvable pins skipped with a warning.
- **Agent-side delivery** (`docker/gyre-agent/agent-runner.mjs`): the
  assembled set injected as `GYRE_META_SPEC_PROMPT` at spawn is prepended to
  the prompt handed to the Claude Agent SDK, guarded by child-process tests
  against a stubbed SDK (mutation-verified to fail when delivery breaks) and
  a new `gyre-agent-tests` CI job.
- **Registry + bindings API**: flat `/api/v1/meta-specs` CRUD with version
  history and per-version lookup, `PUT/GET /api/v1/specs/:path/meta-spec-bindings`
  with pin validation (409-guarded deletes, version-bumping updates with
  archival), scope-admin gate on the `required` flag, ABAC route mappings
  registered and legacy exemptions removed.
- **Provenance + maintenance**: merge attestation records `meta_specs_used`
  from the spawn-time prompt-set record; `meta_spec_set_sha` returned from
  spawn; hourly stale-pin detection creates priority-6 `MetaSpecDrift`
  notifications (deduped per spec+meta-spec); 9 default meta-specs seeded at
  first startup per the spec table.
