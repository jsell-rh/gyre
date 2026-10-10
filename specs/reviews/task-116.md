# Review — task-116: Implement meta-spec prompt assembly

Round: 5 (rounds 4–5 re-reviewed the registry-write governance gate).
Reviewer scope: `git diff 66422bd..9205a9e` for `crates/gyre-server/src/api/meta_specs.rs`
(rounds 4–5 delta over the round-3 base); all other task files verified byte-identical
between the round-3-tested HEAD (`2cbca13`) and current HEAD (`9205a9e`), so round-3
evidence carries over. Mutation evidence persisted at
`/tmp/stage/review-evidence/task116-round5-mutation-evidence.md`.

## Round 4/5 verdict

`progress: complete`. Round 4 extended the scope-admin gate from "`required` flag
only" to **all registry writes** (POST/PUT/DELETE on `/api/v1/meta-specs`), closing a
cross-workspace prompt-injection hole (any authenticated caller could create a
workspace-scoped meta-spec in a workspace it had no membership in; assembly band 2
injects it into every agent spawned there). Round 5 independently re-verified:

- **Gate correctness** (`api/meta_specs.rs:978-1026`): Global → tenant Admin only;
  Workspace → Owner/Admin membership of the entry's own workspace (global Admin
  passes); agent tokens (no user identity) always 403. Applied unconditionally on
  create (against the caller-supplied scope/scope_id), and on update/delete against
  the loaded entry's own scope — a Developer in workspace A cannot PUT/DELETE into
  workspace B.
- **Mutation-verified**: (1) reverting to the pre-round-4 conditional gate fails
  both new tests at the create assertion (201 vs 403); (2) removing update+delete
  gates only, the PUT prompt-rewrite assertion fails (200 vs 403); (3) removing
  the delete gate only, the DELETE assertion fails (204 vs 403). Each gate
  assertion bites independently.
- **No collateral breakage**: all 18 `api::meta_specs` tests pass at HEAD
  (incl. `put_meta_spec_set_requires_admin`, preview JWT tests); e2e script uses
  the admin token; `publishPersona` (web UI) now correctly requires workspace
  Owner/Admin — consistent with NEW-26 (the same principle enforced on
  `PUT /workspaces/:id/meta-spec-set` in commit 6694d94: "agents could rewrite
  their governance rules"). Spec-level bindings route keeps its separate
  Developer/Admin gate (spec-authorship level — bindings select registry content,
  which is itself now admin-gated).
- **NEW-26 citation verified**: real security finding (commit 6694d94), not a
  fabricated reference.

Non-blocking: the tightened gate is stricter than §2's literal text ("Only
scope-level admins can set `required`") — it governs the whole write surface on
the NEW-26 rationale. This is the safe direction (the registry IS injected prompt
content; restricting who can write prompts is strictly tighter than who can set
the flag) and consistent with the enforcement the repo already shipped for the
workspace meta-spec-set route. If the spec owner wants the literal reading, amend
the spec via lifecycle rather than weakening the gate.

## Round 3 and earlier
Round 3 (ready-for-review → complete)
Reviewer scope: commits `9b15d3e`, `9a7079b` + this round's repair commit.

### Verdict (round 3)

One material gap found and repaired that round
(agent-side delivery of the assembled prompt); everything else checked out
against `agent-runtime.md` §2 with file-level evidence below.

The previous rounds wired assembly, storage, attestation, and env injection,
but the last hop was missing:

- `crates/gyre-server/src/api/spawn.rs:659` injects `GYRE_META_SPEC_PROMPT`
  (rendered from the assembled set) into `container_env`.
- Env passthrough is real on all three compute backends
  (`local.rs:15` `.envs()`, `container.rs:186` `--env`, `ssh.rs:188` env prefix).
- But `docker/gyre-agent/agent-runner.mjs` built `taskPrompt` only from
  `GYRE_TASK_PROMPT`/default and passed it to `query()` — `GYRE_META_SPEC_PROMPT`
  was never read. Nothing in `docker/` consumes it (grep across the runner,
  cred-proxy, entrypoint). Result: the registry was stored, injected, and
  attested, but never shaped agent behavior — the spec's core claim
  ("the meta-spec registry IS the prompt configuration") was unfulfilled, and
  every `meta_specs_used` attestation attested prompts that were never sent.
  This is exactly the hollow-implementation class the goal statement forbids.

Repair (this round):

- `agent-runner.mjs`: `fullPrompt = GYRE_META_SPEC_PROMPT + "\n---\n" + taskPrompt`
  (prepended — injection order is part of the attested configuration; task
  prompt follows). No meta-specs → prompt unchanged, no separator.
- `agent-runner.test.mjs` (new): runs the REAL runner as a child process with a
  stubbed `@anthropic-ai/claude-agent-sdk` that captures the prompt handed to
  `query()`. Three tests: (a) meta-spec block leads, separator present, task
  prompt follows; (b) unset env → task prompt leads unchanged, no separator;
  (c) `GYRE_TASK_PROMPT` honored as the tail after the meta-spec block.
  Bug-kill verified: reverting the runner to `query({ prompt: taskPrompt })`
  fails 2 of 3 tests (`# pass 1 / # fail 2`), restored fix passes 3/3.
- `docker/gyre-agent/package.json` test script extended; new CI job
  `gyre-agent-tests` in `.github/workflows/ci.yml` (the suite previously ran
  nowhere — `node --test` only existed for cred-proxy, invoked by nobody).
- Chose prompt-prepending over an SDK `systemPrompt` option because npm
  registry access is unavailable in this environment and the pinned SDK
  (`^0.2.81`) option surface can't be verified offline; a wrong option name
  would fail silently — the exact failure class this round closed.

### 2. Confirmed-correct (spot-checked with evidence)

- **Assembly order + dedup** (`crates/gyre-server/src/prompt_assembly.rs:107-223`):
  required tenant (Global) → required workspace (scope_id-filtered) → bindings
  at pinned versions; kind rank persona→principle→standard→process with stable
  name/id tiebreak; dedup by `meta_spec_id` keeps the required section and
  skips the redundant binding. Pinned-version lookup reads immutable history
  (`prompt_at_version`), and an unresolvable pin is skipped with a warn rather
  than injecting the wrong version. Tests cover order, dedup, cross-workspace
  exclusion, historical pin content, and unresolvable-pin skip (8/8 pass).
- **Versioning** (`crates/gyre-adapters/src/sqlite/meta_spec.rs:270-335` and
  postgres twin): update archives the current row into `meta_spec_versions`
  before overwriting; `update_archives_version` adapter test passes; API
  `PUT` bumps version + recomputes `content_hash` (`update_bumps_version`).
  DELETE is guarded by binding count (409) in both adapters — port contract
  mirrored in mem.
- **Scope-admin gate** (`api/meta_specs.rs` `check_scope_admin`) [superseded
  by rounds 4–5: the gate now applies to ALL registry writes, not just the
  `required` flag — see Round 4/5 verdict above]: Global →
  tenant Admin only; Workspace → workspace Owner/Admin membership (global
  Admin passes); agent tokens (no user identity) 403. Gated on create-with-
  `required` and on any `required` flip in update (both directions).
  `required_gate_rejects_non_admin` exercises 403 for agent token on create
  and update, and 200 for admin.
- **Stale pin detection** (`prompt_assembly.rs:320-371` + `jobs.rs` +
  `main.rs`): hourly job scans all bindings (new `list_all` on the port,
  implemented in sqlite/postgres/mem), compares pin vs current version,
  creates priority-6 `MetaSpecDrift` notifications for workspace Admin/Owner
  members, deduped per (spec, meta-spec) via kv; binding replacement clears
  the dedup keys so a new drift re-notifies.
  `stale_pin_detection_creates_notification` asserts priority 6, content, and
  dedup on second run.
- **Bootstrap** (`lib.rs:1370-1476`): 9 defaults seeded when the table is
  empty, exactly the spec table (default-worker, workspace-orchestrator,
  repo-orchestrator, spec-reviewer, accountability, security,
  conventional-commits [Principle, required=true], reconciliation,
  test-coverage [Standard]); real SHA-256 content hashes.
- **Attestation** (`merge_processor.rs:1586-1601`): merge attestation loads the
  spawn-time prompt-set record by `author_agent_id` and populates
  `meta_specs_used` (previously hardcoded `vec![]`). Spawn response exposes
  `meta_spec_set_sha` from the real assembled set (previously `None`).
- **Routes/ABAC**: flat `/api/v1/meta-specs` + `:id` + versions + version
  detail + `/api/v1/specs/:path/meta-spec-bindings` registered
  (`api/mod.rs:740-764`) with `RouteResourceMapping` entries
  (`abac_middleware.rs:443-465`); the four legacy `meta-specs-registry`
  exemption-file entries were REMOVED (exemption count shrinks — the frozen
  baseline only blocks growth). e2e script and `web/src/lib/api.js` updated to
  the new route names.

### 3. Non-blocking observations (recorded, no action required)

- SSH compute target in `container_mode` drops all env vars
  (`spawn.rs:957` `env: HashMap::new()`) — pre-existing M19.5 behavior
  affecting every env var (auth token included), not introduced by this task.
  Out of task scope; flagged for a future task.
- `assemble_prompt_set` uses `unwrap_or_default()` on repo listing errors —
  a storage failure would silently inject no meta-specs. Acceptable for prompt
  assembly (agents must still spawn), but the merge attestation then records
  an empty set rather than a wrong one, which is the safe direction.
- Stale-pin notification for a Global meta-spec bound by a spec with no ledger
  workspace is skipped with a warn (no addressable workspace) — reasonable
  degradation, logged.

### 4. Test-quality audit (goal rule 2)

- Delivery tests kill the real regression (verified by mutation: reverting the
  runner consumption fails them).
- `required_gate_rejects_non_admin` fails if the 403 gate is removed
  (agent-token path would get 201).
- `stale_pin_detection_creates_notification` fails if detection stops
  creating notifications or dedup breaks (second-run count assertion).
- Assembly tests fail on order/dedup/scope/pin-content regressions.
- No assertionless or conditional-guard tests found in the new suites.

## Verification run this round

- `cd docker/gyre-agent && npm test` → 12 pass / 0 fail (9 cred-proxy + 3 new
  delivery tests); mutation check 1 pass / 2 fail with reverted runner.
- `node --check agent-runner.mjs` → OK.
- `cargo test -p gyre-server --lib prompt_assembly` → 8 pass.
- `cargo test -p gyre-server --lib meta_specs` → 17 pass;
  `registry_tests` → 8 pass.
- `cargo test -p gyre-adapters --lib meta_spec::` → 5 pass;
  `meta_spec_set` → 9 pass (total 14).
- `cargo test -p gyre-server --lib spawn` → 36 pass.
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh` → all OK.
- `web/dist` rebuilt (was stale: old bundle still served
  `/meta-specs-registry` routes); new bundle contains the flat routes only.

## Shipped

- Prompt assembly pipeline: required tenant → required workspace → spec-level
  bindings at pinned versions, kind-ordered and deduped, with real
  pinned-version resolution from immutable history and skip-with-warn on
  unresolvable pins.
- Agent-side delivery: `GYRE_META_SPEC_PROMPT` prepended to the LLM prompt in
  the runner, guarded by child-process tests against a stubbed SDK (fails when
  delivery breaks), wired into a new CI job.
- Meta-spec registry CRUD at flat `/api/v1/meta-specs` routes with version
  history endpoints, binding management (`PUT/GET
  /api/v1/specs/:path/meta-spec-bindings`), scope-admin `required` gate,
  ABAC mappings, and legacy exemption entries removed.
- Stale pin detection (hourly job, priority-6 drift notifications with dedup)
  and merge attestation recording `meta_specs_used` from the spawn-time
  prompt-set record; 9 default meta-specs seeded at first startup.

## Round 6

Round: 6 (independent full re-review of candidate `63d66b469f22a153f56253bbc152f4bc2904f170`
against base `8c2d177505852b3e39cd77f4f782fb355de245aa`; product delta = the task's
single `commits:` entry `5d428e7c`).

### Verdict (round 6): NOT approved

Two confirmed spec violations (both demonstrated by focused probes, both on the exact
production path), plus one inherited adapter-contract divergence. Prior rounds' findings
remain valid and were re-verified; the two new holes are on surfaces earlier rounds did
not probe.

#### Defect A (blocking) — unapproved meta-specs are injected into agents

`agent-runtime.md` §2: "editing a meta-spec's content resets `approval_status` to
`Pending`, and **the new version cannot be used by agents until re-approved by a
human**."

`assemble_prompt_set` (`crates/gyre-server/src/prompt_assembly.rs:107-223`) has no
`approval_status` filter on any band: required tenant, required workspace, or
spec-level binding resolution. The lifecycle half is real — `put_meta_spec`
(`api/meta_specs.rs:1155-1161`) does reset approval to `Pending` on prompt edit —
so the enforcement half is the missing one. Chain: admin PUTs a prompt edit →
status resets to Pending → the next agent spawned in that scope injects the edited,
unreviewed prompt (and attests it in `meta_specs_used`).

Probe (temp test, removed after run): create a Global meta-spec with
`approval_status: Pending`, `required: true`; call `assemble_prompt_set(&state, &task)`;
it returns 1 injected section containing the unapproved prompt. Output:
`PROBE-A: Pending required meta-spec produced 1 injected section(s)` — assertion
`sections.is_empty()` failed. Evidence:
`/tmp/stage/review-evidence/probe-approval-gating.log`.

Required repair: filter required-band collection AND binding resolution to
`approval_status == Approved`, skipping with a warn (consistent with the existing
unresolvable-pin handling) so unapproved content never reaches `PromptSection` or
`MetaSpecUsed`.

#### Defect B (blocking) — bindings route has no workspace-scope authorization

`put_spec_meta_spec_bindings` (`api/meta_specs.rs:1297-1316`) checks only that the
caller's JWT roles contain Admin or Developer. It never resolves the workspace that
owns the target spec, so a Developer with zero workspace memberships can bind
meta-specs to a spec owned by another workspace. Assembly band 3 then injects the
bound prompt (at its pinned version) into every agent spawned for tasks under that
spec — the identical cross-workspace prompt-injection threat rounds 4–5 closed for
registry CRUD via `check_scope_admin`. The route is also exempt from middleware scope
resolution, so nothing downstream re-checks.

Probe (temp test, removed after run): Developer JWT (`sub: "dev-sub"`,
`realm_access.roles: ["developer"]`, no memberships), spec ledger entry
`system/victim.md` with `workspace_id: Some("ws-b")`, Global meta-spec `ms-x`;
`PUT /api/v1/specs/system%2Fvictim.md/meta-spec-bindings` with
`{"bindings":[{"meta_spec_id":"ms-x","pinned_version":1}]}` → **200 OK** where 403 is
required. Evidence: `/tmp/stage/review-evidence/probe-binding-scope.log`
(`PROBE-B: developer binding PUT status = 200 OK`).

Required repair: resolve the spec's workspace via `state.spec_ledger.find_by_path`
(entry carries `workspace_id: Option<String>`), require caller membership in that
workspace (Developer+ / Admin, mirroring `check_scope_admin` membership semantics),
and validate each bound meta-spec is visible in the spec's scope (Global or same
workspace).

#### Defect C (reportable, inherited) — mem adapter ignores delete binding guard

Port contract (`gyre-ports/src/meta_spec_repository.rs:41-42`): "Delete a meta-spec
by ID. Returns an error if bindings reference it." `MemMetaSpecRepository::delete`
(`mem.rs:3894-3897`) retains unconditionally and returns `Ok(())`; SQLite enforces
the guard (`sqlite/meta_spec.rs:343-353`). Identical code exists at base
`8c2d1775`, so this predates the task — but the task modified this same trait impl
(added `list_all`) without closing the divergence, and `check-mem-port-contracts.sh`
audits create-side guards only, so CI cannot catch it. Repair: mirror the sqlite
binding-count guard in mem before retaining.

### Confirmed-correct this round (re-verified)

- Product diff vs base is exactly `5d428e7c`'s content; process-only commits
  (`8c8d24d3`, `68cab408`, `63d66b46`) touch no product surface.
- Focused suites pass at candidate: `prompt_assembly` 8/8, `api::meta_specs` 18/18,
  adapters `meta_spec` 9/9, `api::spawn` 29/29, agent-runner node tests 12/12. All 11
  applicable mechanical invariant scripts pass (logs under
  `/tmp/stage/review-evidence/`).
- Assembly order/dedup/pinned-version resolution, stale-pin job, bootstrap seeding
  (9 entries, idempotent, real SHA-256), merge attestation population, env delivery
  on local/container/ssh backends — all still as validated in rounds 1–5.

### Test-quality note

The existing suites are genuinely strong where they exist (mutation-verified in
rounds 3 and 5), but the two confirmed holes are exactly where tests are absent:
no test covers approval gating of injection, and no test covers bindings-route
scope. The probes that failed this round are the missing tests.

### Verification run this round

- `cargo test -p gyre-server --lib probe_pending_required_meta_spec_is_injected` →
  FAILED at the gap assertion (defect A evidence).
- `cargo test -p gyre-server --lib probe_binding_route_scope` → FAILED at the gap
  assertion (defect B evidence).
- Both temp probes removed; working tree pristine at `63d66b46`
  (`git status --porcelain` empty after restore).
