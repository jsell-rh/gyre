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
commits: ["a293e2998889f272af1be1afbf0ee265db02d50d"]
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

All §2 acceptance criteria are met at this branch HEAD (merge commit
`633674c16`, task-116 work rebased onto upstream `72ad2a128`).

- **Kinds & storage:** `MetaSpec` domain entity (domain/meta_spec.rs) with
  Persona/Principle/Standard/Process kinds and Global/Workspace scopes;
  SQLite + Postgres + mem adapters implementing `MetaSpecRepository`
  (migration 000032: meta_specs, meta_spec_versions, meta_spec_bindings).
- **Versioning:** every content update bumps `version`, recomputes the
  SHA-256 `content_hash`, archives the prior row into immutable
  `meta_spec_versions`, and resets `approval_status` to Pending (adapter
  test `update_archives_version`); pinned versions resolve from history
  via `prompt_at_version`.
- **Prompt assembly** (`crates/gyre-server/src/prompt_assembly.rs`):
  required tenant → required workspace → spec-level bindings at pinned
  versions, kind-ranked (persona→principle→standard→process) with stable
  tiebreak; same-`meta_spec_id` dedup keeps the required section;
  unresolvable pins are skipped with a warn. Only
  `approval_status == Approved` meta-specs inject in any band — required
  bands pass through `filter_approved`, and band 3 fail-closes on the
  entity's CURRENT approval status. Tests:
  `pending_required_meta_spec_excluded`, `pending_bound_meta_spec_skipped`.
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
- **API:** registry CRUD + filters, version history, and spec-level
  bindings (`/api/v1/specs/:path/meta-spec-bindings` PUT/GET) with
  `RouteResourceMapping` ABAC entries (the 4 registry routes were also
  moved OUT of the frozen ABAC exemption file into the resolver — shrink
  direction). Bindings PUT requires membership in the workspace owning
  the spec and validates that each bound meta-spec is visible from that
  workspace (Global anywhere, Workspace only in its own). Test:
  `spec_bindings_require_membership_in_owning_workspace`.
- **Mem adapter port contract**:
  `MemMetaSpecRepository::delete` enforces the port-documented binding
  guard against the shared binding store. Test:
  `mem::meta_spec_binding_contract_tests::delete_fails_when_binding_references_meta_spec`.

### Rebase repair round 4 (upstream 72ad2a128, merge 633674c16)

Upstream was merged into this branch a fourth time. Source-conflict
resolution decisions:

- **Route naming:** kept main's `/api/v1/meta-specs-registry` CRUD routes
  (they avoid colliding with the pre-existing
  `/api/v1/meta-specs/:path/blast-radius` namespace; the web client,
  e2e-flow.sh, docs, and upstream tests all use `-registry`). The
  branch's earlier flat `/api/v1/meta-specs` rename was reverted
  everywhere. Our new spec-bindings route does not collide and is kept.
- **Registry write authorization:** adopted main's stricter task-209
  per-handler Admin-role-only gates (plus the workspace-membership gate
  on workspace-scoped LIST); removed our superseded `check_scope_admin`
  gate and its Owner-can-write test. The round-4 cross-workspace
  prompt-injection hole stays closed — strictly, since non-Admins cannot
  write any registry entry. The agent-token 403 regression test
  (`required_gate_rejects_non_admin`) is retained with adapted URIs.
- **Jobs:** both registrations kept — our hourly
  `meta_spec_stale_pin_check` (§2 stale pin detection) and upstream's
  `meta_spec_conformance_sweep` (meta-spec-reconciliation §10); ours now
  awaits the (newly async) `JobRegistry::register`.
- **Struct literal updates:** upstream's new
  `MetaSpec.approved_content_hash` field added to our test fixtures.
- **web/dist:** conflicts resolved by side selection only; no dist
  rebuild in this assignment (executor restores from the assignment
  base; final integration rebuilds and publishes remotely).

Post-merge probes (`SKIP_WEB_BUILD=1`): full gyre-server lib suite —
1418 passed, 0 failed; full gyre-adapters lib suite — 370 passed,
0 failed; filtered suites meta_spec (31), meta_specs (23),
prompt_assembly (10), spawn (43), merge_processor (67), attestation
(55), mem:: (14, incl. the binding-contract test), stale_pin (1) — all
passed; `vitest run src/__tests__/MetaSpecs.test.js` — 102 passed;
docker/gyre-agent `npm test` — 12 passed; workspace `cargo fmt --check`
clean; check-arch, check-abac-route-registry,
check-abac-exempt-handlers, check-mem-port-contracts,
check-migration-versions, check-dead-message-kinds,
check-byte-slice-truncation, check-relative-path-defaults,
check-fail-open-ref-resolution, check-scope-literal-defaults,
check-inert-enforcement, check-lossy-secret-conversion,
check-forwarded-header-trust, check-in-memory-state-stores,
check-unbounded-external-http, check-forged-scope-fields,
check-migration-sql-portability, check-mcp-write-tools,
check-task-commit-attribution — all OK. Evidence:
`/tmp/stage/review-evidence/task116-rebase-72ad2a128.md`.

`cargo test --all` and exact-head GitHub CI are owned by the verification
stage per assignment constraints.
