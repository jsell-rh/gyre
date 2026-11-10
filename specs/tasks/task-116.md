---
title: "Implement meta-spec prompt assembly"
spec_ref: "agent-runtime.md §2 Meta-Spec Prompt Assembly"
depends_on: []
progress: complete
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
- **API:** registry CRUD at `/api/v1/meta-specs-registry` (flat routes
  with `?scope/?scope_id/?kind/?required` filters, `:id/versions` +
  `:id/versions/:version` history) and
  `/api/v1/specs/:path/meta-spec-bindings` (PUT/GET), with
  `RouteResourceMapping` ABAC entries — the four registry routes were
  moved out of the frozen ABAC exemption file into the resolver (shrink
  direction) in rebase round 4. Registry writes (POST/PUT/DELETE) are
  Admin-role-only per upstream task-209 (non-Admin roles and agent
  tokens 403; tests `registry_writes_reject_non_admin`,
  `required_gate_rejects_non_admin`); DELETE returns 409 while bindings
  reference the meta-spec. **Round-6 fix 2:** `put_spec_meta_spec_bindings` requires
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

### Rebase repair rounds 1–3 (dist-only conflicts)

Upstream was merged into this branch three times; each time the only
conflicts were committed `web/dist` build artifacts where each side's
bundle predated the other's `web/src` changes, and each was resolved the
same way — rebuild dist from the merged `web/src` with the locked
toolchain (`npm ci` then `vite build`); no source changes.
- Round 1 (merge `15b451be`): upstream `75ada1c4`. Post-merge probes
  (`SKIP_WEB_BUILD=1`): gyre-server lib tests filtered to meta_spec /
  prompt_assembly / spawn / mem:: — 84 passed, 0 failed. Evidence:
  `/tmp/stage/review-evidence/task116-rebase-75ada1c4.md`.
- Round 2 (merge `6c327592`): upstream `3c39ab1f`. Post-merge probes
  (`SKIP_WEB_BUILD=1`): same filters — 88 passed, 0 failed (includes
  `stale_pin_detection_creates_notification`, the approval-gating and
  binding-membership regression tests, and the mem binding-contract
  test); `vitest run src/__tests__/MetaSpecs.test.js` — 102 passed;
  check-arch, check-abac-route-registry, check-mem-port-contracts all OK.
  Evidence: `/tmp/stage/review-evidence/task116-rebase-3c39ab1f.md`.
- Round 3 (merge `f68e2a29`): upstream `9bef21a4` (task-197 default-Tenant
  bootstrap; coexists with `seed_builtin_meta_specs` at startup, both
  verified). Post-merge probes (`SKIP_WEB_BUILD=1`): same filters —
  88 passed, 0 failed (26 meta_spec + 10 prompt_assembly + 41 spawn +
  11 mem::), including `stale_pin_detection_creates_notification`, the
  approval-gating, binding-membership, and mem binding-contract regression
  tests, plus upstream's `bootstrap_default_tenant` idempotency test;
  `vitest run src/__tests__/MetaSpecs.test.js` — 102 passed; check-arch,
  check-abac-route-registry, check-mem-port-contracts,
  check-migration-versions, check-task-commit-attribution all OK.
  Evidence: `/tmp/stage/review-evidence/task116-rebase-9bef21a4.md`.

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

### Assignment-base merge and contract repair (base 98a5299b9, merge 0be374088)

The assignment base was merged into the branch (upstream task-088/120
user-management, SCIM, and auth work). `mem.rs` carries both sides'
changes with the round-6 mem binding guard and its contract test
intact; the meta-spec registry routes, resolver entries, and prompt
assembly are unchanged by the merge. The executor restored the assigned
task document — re-adding the Agent Instructions section that
checkpoint `47368bac2` had dropped from the protected task text — and
this commit restores that contract with no source changes.

Post-merge probes at `0be374088` (`SKIP_WEB_BUILD=1`): gyre-server lib
tests filtered to meta_spec / meta_specs / prompt_assembly / spawn /
mem:: — 94 passed, 0 failed, including all round-6 regression tests
(`pending_required_meta_spec_excluded`, `pending_bound_meta_spec_skipped`,
`spec_bindings_require_membership_in_owning_workspace`,
`registry_writes_reject_non_admin` / `required_gate_rejects_non_admin`,
`stale_pin_detection_creates_notification`,
`mem::...::delete_fails_when_binding_references_meta_spec`);
gyre-adapters `update_archives_version` and meta_spec filters — pass;
check-arch, check-abac-route-registry, check-mem-port-contracts,
check-task-commit-attribution — all OK. Evidence:
`/tmp/stage/review-evidence/task116-contract-repair.md`.

`cargo test --all` and exact-head GitHub CI are owned by the verification
stage per assignment constraints.

### Verification repair (clippy changed-lines gate, commit 206f7d9df)

The verification run (`bash /tmp/stage/checks.sh`, exit 1) failed on one
gate: `scripts/check-clippy-diff.py` against assignment base `c5deca801`
flagged two lints on changed lines in `prompt_assembly.rs` —
`clippy::ptr_arg` on `sort_by_kind` (`&mut Vec<MetaSpec>` → `&mut
[MetaSpec]`; both call sites coerce) and
`clippy::cloned_ref_to_slice_refs` in `set_sha_changes_with_content_and_order`
(`&[s1.clone()]` → `std::slice::from_ref(&s1)`; identical hashed bytes).
Both fixes are semantics-preserving — the same 2-line diff, no behavior
change. The run's stale-exemption WARN for the four
`/api/v1/meta-specs-registry*` routes is upstream baseline noise (base
`c5deca801` carries the exemption lines while also resolving the routes;
this branch removed them and the base merge `7b647b5fe` keeps the
removal) and is non-fatal — `check-abac-route-registry.sh` exits 0 at
HEAD.

Post-fix probes: `check-clippy-diff.py c5deca801` — exit 0, "changed
lines clean (13 Rust files, 1020 existing warnings outside changes)";
`check-rustfmt-diff.py c5deca801` — clean; `cargo test -p gyre-server
--lib prompt_assembly` — 10 passed, 0 failed. Evidence:
`/tmp/stage/review-evidence/task116-clippy-repair.md`.

### Assignment-base merge (base fefde2946, merge b327102a8)

The assignment base (upstream task-089 HSI Explorer Generated Views,
secret rotation, rollout policies) was merged into the branch with three
source conflicts, all in `gyre-server`:

- **jobs.rs / main.rs:** both job registrations kept — the branch's hourly
  `meta_spec_stale_pin_check` (§2 stale pin detection) and the base's
  5-minute `secret_rotation_check` (platform-model.md §7). Both are
  registered in `start_job_registry` and spawned at startup.
- **mem.rs:** the branch's `agent_inbox_max`/`user_workspace_state` kept;
  the base's new `rollout_policies` field adopted
  (`MemRolloutPolicyRepository`, wired via `store!` in auto-merged
  `lib.rs`); the base's `default()` meta-spec store initializers dropped
  because the branch wires `meta_specs`/`meta_spec_bindings`/
  `meta_spec_sets` after `message_dispatch_tx` against the shared
  `meta_spec_binding_store` (round-6 mem port-contract fix wiring). No
  duplicate field initializers remain.

Post-merge probes at `b327102a8` (`SKIP_WEB_BUILD=1`): `cargo check` lib+
 bins exit 0; filtered suites meta_spec (37), prompt_assembly (10),
`mem::` (14), spawn (45), secret_rotation+stale_pin (4), rollout (9) —
all passed, including all six named round-6 regression tests
(`pending_required_meta_spec_excluded`, `pending_bound_meta_spec_skipped`,
`spec_bindings_require_membership_in_owning_workspace`,
`registry_writes_reject_non_admin`/`required_gate_rejects_non_admin`,
`stale_pin_detection_creates_notification`); `cargo fmt --all --check`
clean; check-arch, check-abac-route-registry,
check-mem-port-contracts, check-migration-versions,
check-task-commit-attribution — all OK; `check-clippy-diff.py
fefde2946` — exit 0. web/dist merged cleanly (no rebuild performed,
per assignment constraints). Evidence:
`/tmp/stage/review-evidence/task116-rebase-fefde2946.md`.

### Review repair round 7 (findings d34a2353, commit 7d5702c72)

The round-7 review flagged two unguarded §2 deliverables — reverting
the merge-attestation `meta_specs_used` wiring or the spawn-side
`GYRE_META_SPEC_PROMPT` env injection kept the entire suite green.
Commit `7d5702c72` adds the two missing durable regression tests (no
production-code changes; the diff is test-module-only):

- `merge_processor::tests::merge_attestation_records_author_prompt_set_meta_specs`:
  stores an `AgentPromptSetRecord` for the MR's author agent via the real
  kv-backed `store_prompt_set_record`, runs `process_next`, and asserts
  the persisted signed `AttestationBundle`'s `meta_specs_used` matches
  the record field-by-field (id, kind, content_hash, version, required,
  scope) and that `verify_bundle` succeeds with the server's real
  Ed25519 public key.
- `api::spawn::tests::spawn_delivers_meta_spec_prompt_to_agent_env`:
  seeds a required approved Global persona into the meta-spec
  repository, spawns through the env-dump compute-target pattern (local
  dispatch, no docker), and asserts the child env's
  `GYRE_META_SPEC_SET_SHA` equals the spawn response's
  `meta_spec_set_sha` (64-hex SHA-256) and that `GYRE_META_SPEC_PROMPT`
  contains the rendered persona section header and prompt text.

Both tests were mutation-checked: reverting `meta_specs_used` to
`vec![]` fails the first; removing the `GYRE_META_SPEC_PROMPT` env
insert fails the second. Post-repair probes (`SKIP_WEB_BUILD=1`):
meta_spec (39), spawn (46), merge_processor::tests (64) — all passed;
`cargo fmt --all --check` clean; check-clippy-diff.py and
check-rustfmt-diff.py against base `fefde2946` clean; check-arch and
check-task-commit-attribution OK. Evidence:
`/tmp/stage/review-evidence/task116-review-repair-d34a2353.md`.

### Contract repair round 8 (findings ee5e8717, base 18adceb68, merge b0dda7896)

The round-8 contract check flagged that the previous checkpoint's task
document had drifted from the assigned protected text — the acceptance
boxes were marked `[x]` (a normative state change that requires separate
spec review) and the Shipped notes / commits list had been rewritten.
The executor restored the assigned task document in this checkout, and
this commit records that restoration: acceptance boxes returned to the
assigned unchecked state, `progress: complete`, and the three-commit
`commits:` list, with the canonical round-1–7 Shipped notes. No
production-source changes were made; notes for this round are appended
under `## Shipped` only.

The assignment base `18adceb68` (upstream task-152/153 search, narrative
generation, graph explorer, pipeline heap fixes) was merged into the
branch as `b0dda7896` with no meta-spec source conflicts — the §2
production fix (domain entity, SQLite/Postgres/mem adapters + migration
`000032_meta_specs`, prompt assembly, registry/spec-binding API with
ABAC resolver entries, hourly stale-pin job, bootstrap seeding, spawn
env delivery, merge attestation) is unchanged by the merge.

Post-merge probes at `b0dda7896` (`SKIP_WEB_BUILD=1`): `cargo check -p
gyre-server --lib` — exit 0; gyre-server lib suites filtered to
meta_spec (39), prompt_assembly (10), spawn (64), mem:: (14), stale_pin
(1) — all passed, including the named round-6/7 regression tests
(`pending_required_meta_spec_excluded`,
`pending_bound_meta_spec_skipped`,
`spec_bindings_require_membership_in_owning_workspace`,
`registry_writes_reject_non_admin`, `required_gate_rejects_non_admin`,
`merge_attestation_records_author_prompt_set_meta_specs`,
`spawn_delivers_meta_spec_prompt_to_agent_env`,
`mem::...::delete_fails_when_binding_references_meta_spec`);
gyre-adapters meta_spec (10) — passed; check-arch,
check-abac-route-registry, check-migration-versions,
check-task-commit-attribution, check-mem-port-contracts — all OK.
Evidence: `/tmp/stage/review-evidence/task116-contract-repair-ee5e8717.md`.
Full `cargo test --all` and exact-head GitHub CI are owned by the
verification stage per assignment constraints.

## Agent Instructions

Read `specs/system/agent-runtime.md` §2 (Meta-Spec Prompt Assembly) in its entirety. The existing Persona model is in `gyre-domain/src/` — grep for `Persona`. Meta-spec API stubs may exist in `gyre-server/src/api/meta_specs.rs`. The persona scope enum is in `gyre-domain`. Agent spawn is in `gyre-server/src/api/spawn.rs`. MCP prompt delivery is in `gyre-server/src/mcp.rs` — look for `system://persona`. Bootstrap seeding patterns: grep for `seed` or `bootstrap` in the server startup code.
