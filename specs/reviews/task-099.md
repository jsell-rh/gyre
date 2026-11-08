# Review — task-099 (Platform Model Bootstrap Command)

Spec: `platform-model.md` §8 "Bootstrap & First-Run" (`gyre bootstrap` CLI Command, What It Does, Dev Mode, Starter Kit, Protocol Injection).
Commits under review: `c903a80b` (main implementation: CLI, server `POST /api/v1/users`, client methods), `e4a1cb6f` (migration renumber fix + regression test), `39daf9a1` (duplicate `repo_id` in spawn response fix + regression test), `c2755e1b` (resumability, token precedence, docs).
Verdict: **needs-revision**.

## Round 1

Test runs (with `CARGO_TARGET_DIR=/tmp/gyre-verify-target`; the shared `target/` is poisoned by sibling-agent worktrees, same artifact documented in task-095/097 reviews):

- `cargo test -p gyre-cli bootstrap` → **14 passed, 0 failed** (81 filtered out).
- `cargo test -p gyre-server --lib users::` → **9 passed, 0 failed** (1169 filtered out) — includes `admin_creates_user_with_authenticating_api_key`, `create_user_rejects_duplicate_external_id`.
- `bash scripts/check-task-commit-attribution.sh` → OK (all 4 commits in frontmatter).
- `bash scripts/check-abac-route-registry.sh` → OK (`/api/v1/users` resolves in the ABAC registry).
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-fabricated-scope-defaults.sh` → OK.
- `bash scripts/check-relative-path-defaults.sh` → OK — but see F6: the check does not cover the starter-kit call site, so its pass is not evidence for that path.

Verified working (no findings):

- The 9-step orchestration is real end-to-end over REST: health check, tenant create (or reuse-by-slug), admin user + API key, workspace, repo, 4 personas, gates, orchestrator spawn, summary (`main.rs:1538-1782`). `StepTracker` reports the failed step, completed steps, and a resume hint — matches the task's error-handling contract.
- Resumability (`c2755e1b`) is genuine: tenant/workspace/repo fall back to find-by-slug/name on create failure, personas skip when their slug exists, the admin step reuses the API key saved in `~/.gyre/config` only when the config's server matches, and a 409 on orchestrator spawn becomes `AlreadyLive` and is kept. New `find_*` client methods are straightforward GET wrappers.
- Token precedence: `--token` > `GYRE_AUTH_TOKEN` > static dev token, matching the documented server-config precedence.
- Dev mode: `--dev` rejects `--tenant`, and non-dev mode rejects a missing `--tenant` — the flag validation the spec's two-mode shape requires.
- Workspace lands in the created tenant: the client's `create_workspace` sends `tenant_id` in the body (`client.rs`), and the bootstrap admin holds `UserRole::Admin`, so the tenant override in the workspace-create handler applies.
- `POST /api/v1/users` (`users.rs:744-820`) is the real server gap-closure: per-handler Admin-role check (deliberately in the handler, commented, so it holds regardless of route-registry state), CSPRNG key with only the SHA-256 hash persisted, stable `local:{username}` external_id for duplicate detection, roles parsed with unknown-role rejection. 9 handler tests pass.
- `derive_slug` mirrors the server's slug logic; duplicate-username detection via external_id plus the 409-resume path is coherent.
- Starter-kit file set matches the spec tree exactly (`specs/manifest.yaml`, `specs/index.md`, `specs/system/design-principles.md`, `AGENTS.md`, `.prek.yaml` with fmt/clippy/conventional-commit hooks), and the writer never overwrites existing files (tested).
- Gate detection covers cargo test / cargo clippy (from `Cargo.toml`) and arch-lint (from `check-arch.sh`) — command strings are enhancements of the spec's representative commands, not violations.
- Summary render covers all step-9 items (IDs, admin key shown once, server URL, clone URL, orchestrator line).
- Both fix commits are legitimate and carry regression tests: `e4a1cb6f` renumbers the migration to the next unused sequence (with a no-duplicate-prefix test), `39daf9a1` stops `OrchestratorAgentResponse` from serializing `repo_id` twice (strict deserializers rejected the wire JSON).

Findings:

- [-] [process-revision-complete] **F1 (major): the bootstrap admin user is never bound to the created tenant.** Spec line 47: "Every user belongs to exactly one tenant"; §8 step 2: "Register admin user". `create_user` builds the user via `User::new` (`users.rs:800`), which leaves `tenant_id: None` (`gyre-domain/src/user.rs:126`), and `CreateUserRequest` has no tenant field at all — so no caller can supply one. Worse, the API-key path in the auth extractor hardcodes `tenant_id: "default"` for every API-key user (`auth.rs:577-588`), so the bootstrap admin authenticates as tenant "default" in perpetuity. Steps 3-8 still succeed only because the Admin role satisfies the workspace-create tenant override and the priority-900 `builtin-admin-all-operations` policy (`abac_middleware.rs:530-553`, condition `subject.global_role == "Admin"`, actions `*`, resources `*`) allows everything regardless of tenant. Net effect: tenant isolation for the primary human identity of a fresh install is absent — the admin is a global superuser on every path the extractor's `"default"` tenant reaches, and can never be constrained to the tenant bootstrap created. The spec's model (user → exactly one tenant → tenant admins manage that tenant) is not what ships. **Process fixes:** implementation checklist item #172 (entity with spec-mandatory scope must be created WITH that scope), new verifier flaw-class bullet (construction-time scope fabrication), and `scripts/check-scope-literal-defaults.sh` (pre-commit + CI; 24 legacy sites frozen in `scripts/scope-literal-defaults-exemptions.txt` with the auth.rs/lib.rs sites owned by this task's revision round).
- [-] [process-revision-complete] **F2 (major): step 8's "with repo-orchestrator persona" is not implemented — the spawn path attaches no persona, and the personas bootstrap registers are never consulted by it.** Spec §8 step 8: "Spawn repo orchestrator agent with repo-orchestrator persona". `spawn_repo_orchestrator_core` (`orchestrator.rs:252-298`) calls `validate_persona(state, "repo-orchestrator")` — but that helper reads `state.meta_specs` (server-seeded persona meta-spec stubs, `orchestrator.rs:56-70`), a different store from the `state.personas` table the bootstrap CLI populates, its return value is discarded (`:283`), and it only warn-logs. The `Agent` created by `spawn_orchestrator` (`orchestrator.rs:76-152`) carries name/type/workspace/repo and nothing persona- or prompt-related; `gyre_domain::Agent` has no such field. The four persona prompts embedded in the CLI (`bootstrap.rs:11-56`) are write-only data: nothing reads them back after registration. Scope note for fairness: the §8 "Protocol Injection" prompt-injection machinery is decomposed to task-101 (not started), so the missing injection layer itself is not counted against this task — but the persona *attachment* clause of step 8 is in the "What It Does" section this task declares, and it does not exist. Relatedly, the task's `coverage_sections` lists "platform-model.md §8 Protocol Injection", over-declaring coverage of a section assigned elsewhere. **Process fixes:** implementation checklist item #173 (validator must return Result, be consumed, and consult the store the spec names) and `scripts/check-inert-enforcement.sh` extended to flag statement-position `validate_*` calls whose result is discarded (this site seeded in `scripts/inert-enforcement-exemptions.txt` pending the revision round).
- [-] [process-revision-complete] **F3 (major): step 8's "Orchestrator reads specs/index.md, observes backlog, begins Ralph loop" — nothing runs.** `spawn_orchestrator` persists the agent row, mints a repo-scoped JWT, bootstraps a signing keypair, bumps budgets, and returns (`orchestrator.rs:76-152`). No compute target resolution, no container or process launch — contrast `spawn_agent` in `api/spawn.rs`, which resolves compute targets and launches a workload. The printed summary claims "Your repo orchestrator is running" (spec's step-9 wording), but the artifact is a database row. The spec's bootstrap promise — a *running platform* — is met only for the control plane. **Process fixes:** implementation checklist item #174 (a status claim requires the underlying workload — a persisted row is not "running") and the matching verifier flaw-class bullet.
- [-] [process-revision-complete] **F5 (major): the starter-kit manifest is schema-invalid for the server's manifest parser, so the starter-kit path silently produces an empty spec registry.** Spec §8 step 6: "If not, create a starter manifest with default policies"; Starter Kit section: `specs/manifest.yaml` "# Default manifest with design-principles spec". `STARTER_MANIFEST` (`bootstrap.rs:142-150`) emits `approval: human` as a bare string, but the server's `SpecEntry.approval` requires `ApprovalConfig { mode: human_only|agent_only|human_and_agent, ... }` (`spec_registry.rs:97-151`). `parse_manifest` therefore fails on a starter-kit manifest, and the push-side `sync_spec_ledger` warn-and-returns on parse failure (`spec_registry.rs:346-352`) — the user's spec registry stays empty with only a log line. The repo's own `specs/manifest.yaml` and the server's `SAMPLE_MANIFEST` both use the correct `approval: {mode: human_only}` form, so this is a client-side authoring defect, not a schema ambiguity. The CLI's manifest-shape test checks generic YAML structure only and cannot catch it. **Process fixes:** implementation checklist item #176 (embedded templates must round-trip through the consumer's real parser in a test) and the matching verifier flaw-class bullet.
- [-] [process-revision-complete] **F4 (moderate): step 6 "INITIALIZE SPEC REGISTRY" is report-only, and the starter-manifest branch is skipped entirely.** Spec §8 step 6: "If repo contains specs/manifest.yaml, parse and register specs; If not, create a starter manifest with default policies". The implementation only prints "ledger syncs on push" / "registry stays empty until specs are pushed" (`main.rs:1726-1733`). No parsing, no registration. For manifest-less repos, the "create a starter manifest" branch is simply skipped unless `--starter-kit` is passed — and `--starter-kit` is a separate opt-in that writes to the local repo path, not the platform registry. Counter-context: `spec-lifecycle.md` documents push-based server-side sync ("When a push lands on the default branch that modifies files in specs/... server-side post-receive hook"), so deferring registration to push has architectural grounding and the implementation notes state this openly. But the acceptance criterion "Spec registry initialized from manifest or starter kit" is checked `[x]` while neither branch registers anything at bootstrap time, and the starter-manifest branch does not exist. Checked criterion without the behavior. **Process fixes:** implementation checklist item #175 (a step implemented as a print statement is unimplemented, not done) and the matching verifier flaw-class bullet.
- [-] [process-revision-complete] **F6 (moderate): starter kit is written to a relative `./<repo-name>/` path when `--repo-path` is omitted.** `main.rs:1734-1738`: `repo_path.clone().unwrap_or_else(|| PathBuf::from(&repo_name))` — with no `--repo-path`, files land relative to the CLI's cwd. This violates the repo's own mechanical invariant (AGENTS.md: "Never store or pass relative `./...` path literals that a child process or filesystem consumer will resolve"); `check-relative-path-defaults.sh` passes only because it does not cover this site. It is also internally inconsistent: step 6's manifest detection treats a missing `repo_path` as "No spec manifest found" while step 6b silently fabricates a different target for the same missing value. **Process fixes:** implementation checklist item #177 (dynamic path defaults are relative path defaults) and `scripts/check-relative-path-defaults.sh` Check 2 (this site seeded in `scripts/relative-path-defaults-exemptions.txt` pending the revision round).
- [-] [process-revision-complete] **F7 (minor): the minted API key is never exercised through the auth extractor, and the implementation notes describe a superseded design.** The test `admin_creates_user_with_authenticating_api_key` (`users.rs:1184-1218`) verifies only the store round-trip (`state.api_keys.find_user_id(hash)`) — never an HTTP call with `Bearer <raw_key>`. Given F1 (the extractor hardcodes tenant "default" and takes `roles: user.roles` on this path), the extractor is exactly where a bug would live, and the criterion-level claim "key authenticates as the new user" is evidenced one layer below the boundary that matters. Separately, the task notes claim client methods "`create_task`, `spawn_agent`" and a step-8 design of "creates task first with task_type implementation then POST /api/v1/agents/spawn" — that was the `c903a80b` design, replaced in `c2755e1b` by `spawn_repo_orchestrator` (task-093 endpoint, no synthetic task). The notes were not updated and describe code that no longer ships. **Process fixes:** implementation checklist item #178 (auth issuance/verification criteria require a test through the actual auth boundary) and the matching verifier flaw-class bullet.

Summary: the happy path is genuinely good — a real 9-step REST orchestration with thoughtful resumability, a correct admin-gated user-creation endpoint with hashed keys, both fix commits legitimate and regression-tested, and 23 relevant tests green under this review. But the spec's bootstrap promise is undermined at its load-bearing points: the admin identity has no tenant (F1), the spawned orchestrator has neither its persona nor a running process (F2, F3), and the spec-registry step both skips its specced behavior (F4) and ships a starter manifest the server cannot parse (F5) — with the acceptance criterion checked regardless. Revision required.

— Verifier, 2026-09-30

## Round 10 (post round-9 repairs, candidate f7f30e427 on base 414fe9aa1)

Scope: the round-9 merge resolution (`8441d58b1`) plus the four review
repairs (`14c20424d`), re-verified independently after nine prior rounds.
All four round-9 durable findings are closed with live evidence; no new
defects found. Evidence: `/tmp/stage/review-evidence/` (README.md with
full probe logs).

### Round-9 findings — all verified fixed

- **F1 (security) create_user cross-tenant mint** — users.rs:812 rejects
  with 403 unless `is_system_principal() && auth.tenant_id == req.tenant_id`.
  `is_system_principal()` is `agent_id == "system" && user_id.is_none()`;
  API-key and JWT auth always carry a real `user_id` (auth.rs re-read), so
  no user-named-"system" spoof is possible, and API-key auth fail-closes
  on unbound users. LIVE-CONFIRMED: jsell (Admin API-key user of "Acme
  Corp", created by this task's own non-dev bootstrap) POSTing a user into
  the dev tenant gets 403 with the containment message; same-tenant
  creation still gets 201 (probe-api.log).
- **F2 (code) Pending-persona 409 misread as AlreadyLive** — client.rs
  classifies the "approve it before spawning" 409 as `PersonaPending`,
  which fails the step truthfully; step 5's resume approves Pending
  personas instead of skipping them. LIVE-CONFIRMED both branches: a
  tenant-scoped Pending repo-orchestrator persona is approved on re-run
  (exit 0, bootstrap-pending-A.log); a workspace-scoped Pending persona
  with no live orchestrator fails the run with exit 1 and the server
  detail instead of a false "already active" (bootstrap-pending-B2.log).
- **F3 (code) gate duplication on re-run** — server rejects duplicate
  gate names per repo with 409 (gates.rs, tested:
  create_gate_rejects_duplicate_name_per_repo) and step 7 lists existing
  gates and skips configured names. LIVE-CONFIRMED: after two dev
  bootstrap runs the repo holds exactly 3 gates, no duplicates
  (probe-api.log), re-run prints skip lines (bootstrap-dev-2.log).
- **F4 (code) unverified credential reuse** — the step-2 fallback fires
  only on `CreateUserError::Duplicate` (real 409); the saved token is
  verified via GET /users/me and must match both the requested username
  and the target tenant. LIVE-CONFIRMED: normal resume continues as jsell
  after verification (bootstrap-acme-2.log, whoami in probe-api.log); a
  config holding the dev token fails the run ("saved credentials
  authenticate as 'system', not 'jsell'", fail3.log).

### Merge-resolution verification

- `2026-10-09-000056_user_tenant_id` is a documented no-op (the column is
  owned by task-120's earlier-sorting 000058); fresh databases no longer
  abort with "duplicate column name". schema.rs carries both
  users.tenant_id (nullable) and agents.persona_id consistently.
- Task-120 fixture repairs verified by the full server test matrix (lib
  1435, all integration targets green), including
  api_key_auth_grants_access.
- Exemption-file changes are removals/count reductions only
  (fabricated-scope 4→3, scope-literal 24→13, inert-enforcement
  validate_persona sites removed); every frozen-baseline script exits 0.
- The git_http system-principal workspace fallback and repos.rs
  sync_specs both use the hardened `is_system_principal()` gate with
  per-handler tenant containment, tested.

### Probes run

`cargo test -p gyre-cli` (139+1), `-p gyre-server --lib` (1435),
`-p gyre-server --tests` (all targets), `-p gyre-adapters` (375+12
ignored) — all green. 20 mechanical check scripts exit 0;
`check-rustfmt-diff.py 414fe9aa1` clean. Live e2e against a
candidate-built server: dev first-run with --starter-kit (5 files, 3
gates, 4 personas, orchestrator registered with truthful launch-failure
report), dev re-run (full resume/dedupe), non-dev bootstrap with
--admin-user (key saved, verified continuation), cross-tenant 403 probe,
wrong-credential rejection, both Pending-persona branches, and all three
failure paths (server down, --dev --tenant, missing --tenant) — exits 1
as required. Pre-existing non-green scripts re-confirmed unchanged from
base: check-cli-spec-parity (spec_assist display, main.rs:1235 identical
at base), check-dead-components (4 violations, all in web/ — zero web
files changed by this diff), check-assertionless-tests /
check-notification-priority (mawk-vs-gawk environment failures; scripts
unchanged from base).

### Non-blocking observations

- The Pending-persona 409 classification string-matches the server error
  text; the server test asserts only "repo-orchestrator", not the full
  phrase, so a future server-side rewording would silently degrade the
  CLI to AlreadyLive. Currently correct and live-verified; a pinned
  server-side phrase test or a structured error code would harden it.
- Coverage-matrix row 51 still describes the starter-manifest schema
  defect as open; the candidate ships the fix (`approval: {mode:
  human_only}` with a shape regression test). Matrix text is
  auditor-owned.

Verdict: **approved** — the task contract is satisfied by real,
independently exercised behavior; all four round-9 findings closed with
live evidence.

— Round-10 reviewer, 2026-11-08
