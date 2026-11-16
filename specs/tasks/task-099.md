---
title: "Platform Model Bootstrap Command"
spec_ref: "platform-model.md §8 Bootstrap & First-Run"
depends_on: []
progress: complete
review: specs/reviews/task-099.md
coverage_sections:
  - "platform-model.md §8 Bootstrap & First-Run"
  - "platform-model.md §8 gyre bootstrap CLI Command"
  - "platform-model.md §8 What It Does"
  - "platform-model.md §8 Starter Kit"
  - "platform-model.md §8 Protocol Injection"
commits: ["851101503e92cba4ddace147403251d39970c701", "93e70ce969237e6bc2622caf38dcbe151f797379", "e555ed7881f6657a9db166eed62ab286001027e0", "14c20424d4ef8a06aba1e857038fe347a5fba911", "6131513c4a6972daa827ec025741cd4d70a0c790", "5d487f44cef3dffadb85626575a4fa34daf4e61d", "e39045ab01c3499e74646ba7137d6076cde4e556", "28fe2508d63ff59965e28fdd012b4eccc39f8616", "ef1ea3e223a3994f3d8c7e97de69264684e5cf5f", "63250520c661eb0f1feed56cafba06e049260dd9", "e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37"]
---

## Spec Excerpt

### `gyre bootstrap` CLI Command

```bash
gyre bootstrap \
  --tenant "Acme Corp" \
  --workspace "Platform Team" \
  --repo gyre \
  --repo-path /home/user/code/gyre \
  --admin-user jsell \
  --oidc-issuer https://keycloak.example.com/realms/acme
```

### What It Does

1. CREATE TENANT — with default budget, configure OIDC issuer
2. CREATE ADMIN USER — register admin, generate API key, save to ~/.gyre/config
3. CREATE WORKSPACE — under tenant, default budget
4. ADD REPOSITORY — initialize or link existing repo
5. REGISTER BUILT-IN PERSONAS — workspace-orchestrator, repo-orchestrator, accountability, security
6. INITIALIZE SPEC REGISTRY — parse specs/manifest.yaml or create starter
7. CONFIGURE DEFAULT GATES — cargo test, cargo clippy, check-arch.sh
8. SPAWN REPO ORCHESTRATOR — with repo-orchestrator persona
9. PRINT SUMMARY — IDs, API key, URLs

### Dev Mode

```bash
gyre bootstrap --dev
```

Skips OIDC. Uses static auth tokens. Single tenant, single workspace.

### Starter Kit

`gyre bootstrap --starter-kit` creates: specs/, AGENTS.md, .prek.yaml

### Protocol Injection

When any agent is spawned, the MCP server injects:
1. Persona prompt (versioned, approved)
2. Protocol norms (Ralph loop, MCP tools, escalation, handoff)
3. Context (task, spec refs, acceptance criteria, worktree, budget)
4. Constraints (repo scope, budget limits)

## Implementation Plan

1. **`gyre bootstrap` CLI command:**
   - Add `bootstrap` subcommand to `gyre-cli/src/main.rs`
   - Parameters: `--tenant`, `--workspace`, `--repo`, `--repo-path`, `--admin-user`, `--oidc-issuer`, `--dev`, `--starter-kit`
   - Validates server is running (health check)

2. **Bootstrap orchestration:**
   - Step 1: `POST /api/v1/tenants` (create tenant)
   - Step 2: `POST /api/v1/users` (create admin user + API key in one call)
   - Step 3: `POST /api/v1/workspaces` (create workspace under tenant)
   - Step 4: `POST /api/v1/repos` (register repo)
   - Step 5: Create built-in personas via `POST /api/v1/personas` (workspace-orchestrator, repo-orchestrator, accountability, security) — auto-approve each
   - Step 6: If `specs/manifest.yaml` exists in repo, call spec registration. Else create starter.
   - Step 7: Configure default gates based on detected project type (Cargo.toml → cargo test/clippy)
   - Step 8: Spawn repo orchestrator via `POST /api/v1/agents/spawn` with repo-orchestrator persona
   - Step 9: Print summary with all IDs and URLs

3. **Dev mode (`--dev`):**
   - Skip OIDC configuration
   - Use GYRE_AUTH_TOKEN for auth
   - Auto-create single tenant "dev" and single workspace "default"
   - No admin user creation (use dev token)

4. **Starter kit (`--starter-kit`):**
   - Create `specs/manifest.yaml` with default spec policy
   - Create `specs/index.md` template
   - Create `specs/system/design-principles.md` template
   - Create `AGENTS.md` with entry point
   - Create `.prek.yaml` with pre-commit hooks (cargo fmt, clippy, conventional commits)

5. **Save config:**
   - Write `~/.gyre/config` with server URL, API key, default workspace
   - Same format as existing `gyre init` config

## Acceptance Criteria

- [x] `gyre bootstrap` creates tenant, workspace, repo, admin user in sequence
- [x] Built-in personas registered and auto-approved
- [x] Spec registry initialized from manifest or starter kit
- [x] Default gates configured based on project type
- [x] Repo orchestrator spawned on completion
- [x] Summary printed with all IDs and URLs
- [x] `--dev` mode works without OIDC
- [x] `--starter-kit` creates spec directory structure
- [x] Config saved to `~/.gyre/config`
- [x] `cargo test --all` passes

## Implementation Notes

- **Server**: `POST /api/v1/users` added in `crates/gyre-server/src/api/users.rs::create_user`
  — Admin-only (per-handler role check), mints an authenticating API key in the
  same call (stored via `state.api_keys`, the store the auth extractor consults;
  only the SHA-256 hash is persisted). `external_id = "local:{username}"` gives
  stable duplicate detection. Route registered in `api/mod.rs`.
- **CLI**: `crates/gyre-cli/src/bootstrap.rs` holds pure logic (persona prompt
  registry via `include_str!`, slug derivation, gate detection, starter-kit
  writer, summary renderer); orchestration lives in `main.rs::run_bootstrap`
  with a `StepTracker` reporting completed steps + resume hint on failure.
- **Client**: 10 new `GyreClient` methods (`health`, `create_tenant`,
  `create_user`, `create_workspace`, `create_repo`, `create_persona`,
  `approve_persona`, `create_gate`, `create_task`, `spawn_agent`).
- **Personas**: `specs/personas/repo-orchestrator.md` authored (referenced by
  platform-model.md §3 but previously missing); all four prompts embedded in
  the CLI at `crates/gyre-cli/src/bootstrap/personas/`.
- **Spec registry step**: report-only — no REST registration endpoint exists;
  the spec ledger syncs on push to the default branch (stated in output).
- **Orchestrator spawn**: creates task first with `task_type: "implementation"`
  (spawn rejects tasks without it), then `POST /api/v1/agents/spawn`.
- Tests: 3 server handler tests (key authenticates as new user, duplicate
  rejected, unknown role rejected) + 11 CLI tests (parse/flags, slug, gates,
  starter kit, manifest shape, summary rendering, persona coverage).

## Verification

- `cargo build --all` — zero warnings
- `cargo test --all` — all pass (server: 1159, cli: 93+1)
- Check scripts: arch, cli-spec-parity, assertionless-tests, no-em-dash,
  api-auth, type-discriminator-values, notification-priority all pass
  (dead-components exit 0 with pre-existing findings only)
- End-to-end smoke test against a live server:
  - `gyre bootstrap --dev --repo gyre-demo --repo-path ... --starter-kit`:
    tenant dev, workspace default, repo, 4 personas pre-approved, 3 gates
    detected from Cargo.toml + check-arch.sh, starter kit written,
    orchestrator spawned, summary printed, exit 0
  - `gyre bootstrap --tenant "Acme Corp" --admin-user jsell`: admin user
    created, API key minted and shown once, config saved to `~/.gyre/config`,
    subsequent steps authenticated as the new admin, exit 0
  - Failure paths: server down → exit 1; `--dev --tenant` → rejected;
    missing `--tenant` without `--dev` → rejected

## Agent Instructions

Read `specs/system/platform-model.md` §8 "Bootstrap & First-Run" for the full spec. The CLI is in `gyre-cli/src/main.rs` — look at how existing commands like `gyre init` work. The bootstrap command orchestrates multiple API calls in sequence. Each step should validate success before proceeding to the next. Error handling: if any step fails, print what succeeded and what failed so the user can resume manually. The `--dev` flag should match the existing dev mode (GYRE_AUTH_TOKEN pattern in server-config.md).

## Shipped

`gyre bootstrap` implements platform-model.md §8 as a real 9-step REST
orchestration with resume semantics, backed by the server pieces the spec
requires. Implementation commits are in the frontmatter. Rounds 1-10
closed the F1-F7 review findings, the round-9 merge, and the round-10
exemption-drift verification findings; round 11 (`93e70ce96`) repaired
the round-11 review findings F11/F12; the round-12 rebase onto
`fefde2946` (`6e679bc8b`) resolved three conflicts: auth.rs JWT
`tenant_id` claim (adopted upstream task-103's serde schema-default
`default_tenant_claim` — behaviorally identical to this branch's
`system_principal_tenant` resolution: absent claim → the bootstrapped
"default" tenant; exemption stays eliminated), and the
fabricated-scope-defaults script/exemptions pair (kept this branch's
stricter FROZEN_EXEMPTION_COUNT=2, which matches the 2 entries present
on both sides).

### Behavior

- CLI: `bootstrap` subcommand with `--tenant/--workspace/--repo/
  --repo-path/--admin-user/--oidc-issuer/--dev/--starter-kit` (plus
  `--server/--token`). Health check, tenant (reuse by slug), admin user +
  API key saved to `~/.gyre/config` with subsequent calls authenticating
  as the new admin, workspace and repo (reuse by name), 4 built-in
  personas created and pre-approved, spec registry init, default gates,
  repo orchestrator spawn, summary with IDs, API key (shown once) and
  URLs. A failing step reports completed steps and a resume hint.
- Server pieces: `POST /api/v1/users` (admin-only, tenant-contained,
  mints an authenticating API key, binds the admin to the tenant,
  fail-closed API-key auth), persona attachment persisted on the agent
  row, spawn wire without duplicate keys, first-run spec-ledger push,
  mem adapter enforcing the SQLite uniqueness constraints, truthful
  orchestrator launch status, starter kit (5 files, never overwrites,
  requires `--repo-path`), dev mode flag rejection and token precedence,
  Protocol Injection decomposed to task-101 per the round-1 F2 scope
  note.
- Resume semantics (round-9 repairs, `14c20424d`): create_user tenant
  containment (cross-tenant Admin minting rejected with 403 unless the
  caller is a system principal); Pending-persona 409 distinguished from
  already-live 409 with step-5 resume approving Pending personas; gate
  dedupe (server 409 on duplicate names, step 7 skips existing);
  verified credential reuse (409-duplicate-only fallback; saved token
  verified for user AND tenant before adoption).
- Push safety (round-11 F11 repair, `93e70ce96`): the step-6 push to the
  server's bare repo no longer forces unconditionally. It ls-remotes and
  fetches the server's main first, then: no remote main -> plain push;
  remote main behind HEAD -> plain fast-forward push (resume re-runs
  with new local commits still push); remote main is the server's
  disposable empty seed commit (root commit, empty tree - exactly what
  `create_initial_commit` writes) -> replace it with
  `--force-with-lease` pinned to the inspected seed SHA; anything else
  (diverged, e.g. main advanced with server-side MR merges) -> refuse
  with a pull-or-rebase hint, so a stale checkout can never rewind
  merged MR commits out of main. The same round fixed a latent
  starter-kit auto-commit defect the new tests exposed: `git add`
  aborts when a pathspec matches no file, so manifest-bearing repos
  without the full five-file kit failed step 6; only the kit paths that
  exist are staged now.
- Token hygiene (round-11 F12 repair): the bearer token reaches git
  through GIT_CONFIG_COUNT/GIT_CONFIG_KEY_n/GIT_CONFIG_VALUE_n
  environment config (git >= 2.31) instead of `-c http.extraHeader=...`
  child argv, which was readable via ps and /proc/<pid>/cmdline for the
  lifetime of every network-touching git call.

### Tests

- Server: user-creation handler tests (key authenticates as new user,
  duplicate rejected, unknown role rejected, cross-tenant admin
  rejected with 403 while same-tenant creation succeeds), gate
  duplicate-name 409 test, orchestrator wire/persona/spawn-name tests,
  ws activity tests, presence fixtures bound to the caller's tenant.
- CLI: bootstrap parse/flag, slug, gate detection, starter kit,
  manifest shape, summary rendering, persona coverage tests, client
  classification tests (Pending-persona 409 vs already-live 409;
  create_user 409 Duplicate vs 403 Other), four scripted full-flow
  resume tests against a mock HTTP server, and three round-11
  push-safety tests running real git against a real bare "server" repo:
  first-run seed replacement plus refusal-to-rewind after a simulated
  server-side MR merge (mutation-tested - restoring the unconditional
  --force fails it), resume fast-forward when local is ahead, and the
  token-out-of-argv invariant for `git_with_auth`.
- Round-11 focused probes on this head (`93e70ce96`): `cargo test -p
  gyre-cli` 142 + 1 passed, 0 failed; `cargo fmt -p gyre-cli --check`
  clean; `cargo clippy -p gyre-cli --bin gyre` exit 0 (only pre-existing
  gyre-common warnings, identical on the base head); check scripts arch,
  byte-slice-truncation, no-em-dash, relative-path-defaults,
  lossy-secret-conversion all exit 0; check-assertionless-tests exit 2
  pre-existing and byte-identical to the base head. Evidence:
  `/tmp/stage/review-evidence/` (source SHA, logs, summary).
- Prior-round suites: `cargo test -p gyre-server --lib` 1447 passed on
  `e555ed788`; `cargo test -p gyre-cli` 139 + 1 and `cargo test -p
  gyre-adapters` 375 passed, 12 ignored on the merged round-9 tree.

### Verification evidence

- Round-13 (`851101503`) repaired the verification clippy-diff failure:
  `python3 scripts/check-clippy-diff.py fefde2946` (the failing gate)
  now exits 0 ("changed lines clean, 33 Rust files"), as does
  `check-rustfmt-diff.py`. Fixed: async_trait double_must_use spans in
  the three undocumented gyre-ports traits (added the sibling-file
  `#[allow]`), a doc_lazy_continuation in auth.rs, four
  await_holding_lock HOME_GUARD holds in the CLI resume tests (guard is
  now a tokio mutex), and two useless_format URI literals in the
  git_http containment test. No behavior change; `cargo test -p
  gyre-cli bootstrap` 23 passed, the edited server test passed,
  check-arch / check-mem-port-contracts / check-no-em-dash exit 0.
  Evidence: `/tmp/stage/review-evidence/`.
- Round-11 evidence: source SHA `93e70ce96`, test/fmt/clippy/check-script
  logs, mutation-check note (same evidence directory, prior run).
- Pre-existing on the candidate head (not introduced here, unchanged):
  `cargo clippy -p gyre-domain --lib` errors in rust_extractor.rs,
  check-assertionless-tests exit 2, check-cli-spec-parity exit 1 on the
  spec_assist SSE display path, and the argv token pattern in main.rs
  Clone/Push commands (predates task-099; F12 was scoped to
  bootstrap.rs).
- Round-4's live e2e (dev first-run bootstrap with `--starter-kit`,
  cross-tenant `--admin-user jsell`, failure paths) was run against
  round-4's head; the round-9 resume behavior and the round-11 push
  safety are covered by the scripted flow and push-safety tests; a
  fresh live e2e remains with verification.
- Full workspace suites, all-target Clippy, and GitHub CI remain with
  verification and publication.
