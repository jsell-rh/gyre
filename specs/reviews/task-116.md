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
