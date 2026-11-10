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
commits: ["7d5702c72c72091fe0f19081cfdcca0f2a137e92", "206f7d9dfc31d7072ef2ffd04f1122507b4d4661", "a293e2998889f272af1be1afbf0ee265db02d50d"]
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

- [x] `MetaSpec` entity supports Persona, Principle, Standard, Process kinds
- [x] Version tracking: each edit creates new version, old versions retained
- [x] Spec-level binding: specs can pin meta-specs at specific versions
- [x] `PromptAssembler` builds ordered prompt set (required tenant → required workspace → spec bindings)
- [x] Deduplication of required + bound meta-specs
- [x] Stale pin detection creates notifications
- [x] 9 default meta-specs seeded at bootstrap
- [x] Full CRUD API at `/api/v1/meta-specs` with version history
- [x] Meta-spec SHAs recorded in merge attestation
- [x] `cargo test --all` passes

## Shipped

All agent-runtime.md §2 (Meta-Spec Prompt Assembly) acceptance criteria are
implemented and regression-tested at this branch HEAD. The work completed
seven independent review rounds (three round-6 code findings, two round-7
unguarded-deliverable findings — all closed with durable mutation-checked
tests) and survived five upstream/base merges.

### Behavior

- **Kinds & storage:** `MetaSpec` domain entity (`gyre-domain/src/meta_spec.rs`)
  with Persona/Principle/Standard/Process kinds and Global/Workspace scopes;
  SQLite + Postgres + mem adapters implement `MetaSpecRepository`
  (migration 000032: `meta_specs`, `meta_spec_versions`, `meta_spec_bindings`).
- **Versioning:** every update bumps `version`, recomputes the SHA-256
  `content_hash`, archives the prior row into immutable `meta_spec_versions`,
  and resets `approval_status` to Pending (adapter test
  `update_archives_version`); pinned versions resolve from history via
  `prompt_at_version`.
- **Prompt assembly** (`gyre-server/src/prompt_assembly.rs`): required tenant →
  required workspace → spec-level bindings at pinned versions, kind-ranked
  (persona→principle→standard→process) with stable tiebreak; same-id dedup
  keeps the required section; unresolvable pins are skipped with a warn; only
  `approval_status == Approved` meta-specs inject in any band (required bands
  filter, band 3 fail-closes on the entity's current approval status).
- **Agent delivery:** `spawn.rs` injects `GYRE_META_SPEC_PROMPT` /
  `GYRE_META_SPEC_SET_SHA` into the container env at the unified
  `container_env` construction (all compute backends route through it under
  the task-117 compute-target model); `docker/gyre-agent/agent-runner.mjs`
  prepends the prompt to the LLM call (guarded by `agent-runner.test.mjs`,
  CI job `gyre-agent-tests`).
- **Attestation:** `merge_processor.rs` populates `meta_specs_used` in the
  signed `MergeAttestation` bundle from the spawn-time prompt-set record (by
  `author_agent_id`); spawn responses expose `meta_spec_set_sha` from the real
  assembled set.
- **Stale pin detection:** hourly `meta_spec_stale_pin_check` job creates
  priority-6 `MetaSpecDrift` notifications for workspace Admin/Owner members,
  deduped per (spec, meta-spec) via kv; binding replacement clears dedup keys.
- **Bootstrap:** `seed_builtin_meta_specs` (invoked at startup in `main.rs`)
  idempotently seeds the 9 spec'd defaults with real SHA-256 content hashes.
- **API:** registry CRUD at `/api/v1/meta-specs-registry` (flat routes with
  `?scope/?scope_id/?kind/?required` filters plus `:id/versions[/:version]`
  history) and `/api/v1/specs/:path/meta-spec-bindings` (PUT/GET), all with
  `RouteResourceMapping` ABAC resolver entries. Registry writes are
  Admin-role-only; spec-binding writes require membership in the workspace
  owning the spec and validate bound meta-spec visibility from that workspace
  (closes the cross-workspace prompt-injection hole); DELETE returns 409 while
  bindings reference the meta-spec.
- **Mem adapter port contract:** `MemMetaSpecRepository::delete` enforces the
  port-documented binding guard against the shared binding store.

### Test evidence

Regression tests (all present and passing): `pending_required_meta_spec_excluded`,
`pending_bound_meta_spec_skipped`,
`spec_bindings_require_membership_in_owning_workspace`,
`registry_writes_reject_non_admin`, `required_gate_rejects_non_admin`,
`stale_pin_detection_creates_notification`,
`mem::meta_spec_binding_contract_tests::delete_fails_when_binding_references_meta_spec`,
`merge_attestation_records_author_prompt_set_meta_specs` (asserts the signed
bundle verifies against the server's real Ed25519 key),
`spawn_delivers_meta_spec_prompt_to_agent_env` (asserts the dispatched child
env's set SHA matches the spawn response and carries the rendered prompt).
Frontend: `src/__tests__/MetaSpecs.test.js` (102 tests); agent runner:
`docker/gyre-agent` `npm test` (12 tests).

### Requirements-changed repair (base dbf9edae, merge e848d990e)

The retained source checkpoint (`1aaae3116`, carrying all work through review
round 7) was merged with assignment base `dbf9edae` (upstream task-117 compute
target model). The base already contains the full task-116 lineage (integrated
upstream via `99a70bbe1`), so the merge resolves to a tree identical to the
base — no source-conflict repair was needed and no source changes were made.
Upstream task-117 adapted the spawn-delivery test to the real kubernetes
dispatch (`1ffe1397f`); the meta-spec env injection now sits at the unified
`container_env` site shared by all compute backends. Post-merge probes
(`SKIP_WEB_BUILD=1`): gyre-server lib suites filtered to meta_spec (39),
prompt_assembly (10), stale_pin (1), api::spawn::tests (52), mem:: (14),
plus both named round-7 tests and gyre-adapters meta_spec (10) — all passed,
0 failed; check-arch, check-abac-route-registry, check-mem-port-contracts,
check-migration-versions, check-task-commit-attribution, check-clippy-diff
and check-rustfmt-diff against base `dbf9edae` — all clean. Evidence:
`/tmp/stage/review-evidence/task116-requirements-repair-dbf9edae.md`.
Full `cargo test --all`, vitest, and exact-head GitHub CI are owned by the
verification stage per assignment constraints.

## Agent Instructions

Read `specs/system/agent-runtime.md` §2 (Meta-Spec Prompt Assembly) in its entirety. The existing Persona model is in `gyre-domain/src/` — grep for `Persona`. Meta-spec API stubs may exist in `gyre-server/src/api/meta_specs.rs`. The persona scope enum is in `gyre-domain`. Agent spawn is in `gyre-server/src/api/spawn.rs`. MCP prompt delivery is in `gyre-server/src/mcp.rs` — look for `system://persona`. Bootstrap seeding patterns: grep for `seed` or `bootstrap` in the server startup code.
