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
commits: ["b453e571043f857c7b86091ba3be12a3ff90e2a3", "7c79870cca8f13e2c249d6766d6aeb135ba51964", "bf462e6c1f187f2bd3ca880942e6ee18f2ffb607", "5d428e7c4b856d48e1796e2fbbf01b3ce7e52b1a"]
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

## Shipped

All §2 acceptance criteria are met at this branch HEAD. Review round 6
raised three code findings; all three are closed — the fixes themselves
landed in the recovered checkpoint commit `bf462e6c`, and this session
added the two missing durable regression tests that had existed only as
the reviewer's (removed) probes.

- **Kinds & storage:** `MetaSpec` domain entity (domain/meta_spec.rs) with
  Persona/Principle/Standard/Process kinds and Global/Workspace scopes;
  SQLite + Postgres + mem adapters implementing `MetaSpecRepository`
  (migration 000032: meta_specs, meta_spec_versions, meta_spec_bindings).
- **Versioning:** every update bumps `version`, recomputes the SHA-256
  `content_hash`, archives the prior row into immutable
  `meta_spec_versions`, and resets `approval_status` to Pending (adapter
  test `update_archives_version`); pinned versions resolve from history
  via `prompt_at_version`.
- **Prompt assembly** (`crates/gyre-server/src/prompt_assembly.rs`):
  required tenant → required workspace → spec-level bindings at pinned
  versions, kind-ranked (persona→principle→standard→process) with stable
  tiebreak; same-`meta_spec_id` dedup keeps the required section;
  unresolvable pins are skipped with a warn. **Round-6 fix 1:** only
  `approval_status == Approved` meta-specs inject in any band — required
  bands pass through `filter_approved`, and band 3 fail-closes on the
  entity's CURRENT approval status (an edited-to-Pending meta-spec's
  pinned versions do not inject until re-approved). Tests:
  `pending_required_meta_spec_excluded`,
  `pending_bound_meta_spec_skipped`.
- **Agent delivery:** `spawn.rs` injects `GYRE_META_SPEC_PROMPT` into the
  container env on all three compute backends;
  `docker/gyre-agent/agent-runner.mjs` prepends it to the LLM prompt —
  guarded by `agent-runner.test.mjs` (real runner as child process against
  a stubbed SDK), wired into CI job `gyre-agent-tests`.
- **Attestation:** `merge_processor.rs` populates `meta_specs_used` in the
  `MergeAttestation` bundle from the spawn-time prompt-set record (by
  `author_agent_id`); spawn responses expose `meta_spec_set_sha` from the
  real assembled set.
- **Stale pin detection:** hourly `meta_spec_stale_pin_check` job
  (jobs.rs) calls `detect_stale_pins`, creating priority-6
  `MetaSpecDrift` notifications for workspace Admin/Owner members, deduped
  per (spec, meta-spec) via kv; binding replacement clears dedup keys.
  Test `stale_pin_detection_creates_notification` passes.
- **Bootstrap:** `seed_builtin_meta_specs` (lib.rs, invoked at main.rs)
  seeds the 9 spec'd defaults idempotently when the table is empty, with
  real SHA-256 content hashes.
- **API:** flat `/api/v1/meta-specs` CRUD + `?scope/?scope_id/?kind/?required`
  filters, `:id/versions` + `:id/versions/:version` history, and
  `/api/v1/specs/:path/meta-spec-bindings` (PUT/GET), with
  `RouteResourceMapping` ABAC entries; all registry writes (POST/PUT/DELETE)
  are scope-admin gated (tenant Admin for Global, workspace Owner/Admin for
  Workspace; agent tokens 403); DELETE returns 409 while bindings reference
  the meta-spec. **Round-6 fix 2:** `put_spec_meta_spec_bindings` requires
  membership in the workspace owning the spec (global Admin OR user
  identity with Owner/Admin/Developer membership; unscoped specs
  admin-only; agent tokens 403) and validates that each bound meta-spec is
  visible from that workspace (Global anywhere, Workspace only in its
  own) — closing the cross-workspace prompt-injection hole. Test:
  `spec_bindings_require_membership_in_owning_workspace` (non-member
  Developer 403, agent token 403, owning-workspace Developer 200).
- **Mem adapter port contract** (round-6 fix 3):
  `MemMetaSpecRepository::delete` enforces the port-documented binding
  guard against the shared binding store (same store the binding repo
  writes through, mirroring the DB adapters' single-storage wiring in
  `lib.rs`). Test:
  `mem::meta_spec_binding_contract_tests::delete_fails_when_binding_references_meta_spec`.

**Test evidence this session** (at HEAD after the two test additions,
`SKIP_WEB_BUILD=1`; full log:
`/tmp/stage/review-evidence/task116-round6-repairs.md`): meta_spec suites
26/26 (api 18 + mem contract 2 + assembly/stale/related 6), prompt_assembly
10/10, mem:: 2/2, spawn 36/36; check-arch, check-abac-route-registry,
check-mem-port-contracts all OK. Cold test build took ~21 min (empty target
cache explains the prior session's build timeouts — not a code defect).
`cargo test --all` is owned by the verification stage per assignment
constraints; focused suites above are the smallest relevant probes.
Infrastructure restrictions (npm registry unreachable, TCP listener probe
errno 95) recorded in the evidence file — exact-head GitHub CI remains the
authority for transport-level checks.

## Agent Instructions

Read `specs/system/agent-runtime.md` §2 (Meta-Spec Prompt Assembly) in its entirety. The existing Persona model is in `gyre-domain/src/` — grep for `Persona`. Meta-spec API stubs may exist in `gyre-server/src/api/meta_specs.rs`. The persona scope enum is in `gyre-domain`. Agent spawn is in `gyre-server/src/api/spawn.rs`. MCP prompt delivery is in `gyre-server/src/mcp.rs` — look for `system://persona`. Bootstrap seeding patterns: grep for `seed` or `bootstrap` in the server startup code.
