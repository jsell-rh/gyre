---
title: "Platform Model Bootstrap Command"
spec_ref: "platform-model.md §8 Bootstrap & First-Run"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-099.md
coverage_sections:
  - "platform-model.md §8 Bootstrap & First-Run"
  - "platform-model.md §8 gyre bootstrap CLI Command"
  - "platform-model.md §8 What It Does"
  - "platform-model.md §8 Starter Kit"
  - "platform-model.md §8 Protocol Injection"
commits: ["28fe2508d63ff59965e28fdd012b4eccc39f8616", "ef1ea3e223a3994f3d8c7e97de69264684e5cf5f", "63250520c661eb0f1feed56cafba06e049260dd9", "e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37"]
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
requires. Implementation commits are listed in the frontmatter; the revision
round closed review findings F1-F7 from round 1, and this round repaired the
task document back to the assigned contract (coverage_sections and contract
body restored verbatim) without changing any Rust source.

### Behavior

- CLI: `bootstrap` subcommand with `--tenant/--workspace/--repo/
  --repo-path/--admin-user/--oidc-issuer/--dev/--starter-kit` (plus
  `--server/--token`). Health check, then tenant (reuse by slug), admin
  user + API key saved to `~/.gyre/config` with subsequent calls
  authenticating as the new admin, workspace and repo (reuse by name),
  4 built-in personas created and pre-approved, spec registry init,
  default gates, repo orchestrator spawn, summary with IDs, API key
  (shown once) and URLs. A failing step reports completed steps and a
  resume hint (`StepTracker`); tenant/workspace/repo/persona reuse and
  the saved-key admin path make a failed run resumable, and a 409 on
  orchestrator spawn becomes `AlreadyLive`.
- Admin user is tenant-bound at creation (F1): `POST /api/v1/users`
  validates the tenant exists and persists `user.tenant_id` (migration
  `000056_user_tenant_id`); the API-key auth path derives the tenant
  scope from that binding and fail-closes (403) on a user without one;
  the bootstrap admin gets `global_role: TenantAdmin`, not a global
  superuser.
- Spec registry (step 6, F4, F5): with a manifest present (pre-existing
  or just written by `--starter-kit`), `push_and_sync_specs` commits the
  starter-kit files, pushes `HEAD:refs/heads/main` to the server repo,
  then calls `POST /api/v1/repos/:id/sync-specs` so the ledger
  populates at first-run time; later pushes stay in sync via the
  post-receive hook. The starter manifest uses the server's
  `ApprovalConfig {mode: human_only}` shape and round-trips through the
  real `parse_manifest` in a server-side test that `include_str!`s the
  CLI's manifest.
- Starter kit (F6): 5 files (`specs/manifest.yaml`, `specs/index.md`,
  `specs/system/design-principles.md`, `AGENTS.md`, `.prek.yaml`),
  never overwrites existing files; `--starter-kit` requires
  `--repo-path` (explicit error, no relative-path default).
- Orchestrator spawn (step 8, F2, F3): task-093 endpoint with
  exactly-one-live semantics; the repo-orchestrator persona is resolved
  from `state.personas` (repo > workspace > tenant scope chain, must be
  Approved, fail-closed) and persisted on the agent row (migration
  `000057_agent_persona_id`); the process launches on the resolved
  compute target (workspace assignment > tenant default > local) and
  the response and CLI summary report the truthful launch status
  (`running`/`launch_failed`), never claiming running for a process
  that failed to start.
- Dev mode: `--dev` rejects `--tenant`/`--admin-user`/`--oidc-issuer`;
  token precedence `--token` > `GYRE_AUTH_TOKEN` > static dev token.
- Protocol Injection (coverage section retained per assigned contract):
  the persona-attachment clause of step 8 is implemented as above; the
  full MCP prompt-injection layer (protocol norms, context,
  constraints) is decomposed to task-101 per the round-1 review F2
  scope note and coverage row 52 (task-101 not-started).

### Tests

- CLI (`bootstrap.rs`, 13 tests + 3 parse tests in `main.rs`): slug
  derivation, gate detection (cargo/arch variants), starter-kit file
  set and never-overwrite, manifest documented shape, summary rendering
  including launch-failure and already-live outcomes, flag
  parsing/defaults/dev mode.
- Server: `users.rs` (create with tenant validation, duplicate
  external_id, unknown tenant, unknown role, minted key authenticates
  through the full router with `Bearer <raw key>` and tenant binding),
  `orchestrator.rs` (persona attached and asserted, scoped JWT, 409
  conflicts, stale restart, truthful launch outcome), `spec_registry.rs`
  (starter manifest round-trips through the real parser).

### Verification evidence

- All evidence from this round is saved under `/tmp/stage/review-evidence/`
  (source SHAs, commands, exit codes, output tails).
- This round (contract-only repair, no Rust source changed), sandbox at
  HEAD `56803714`: `cargo test -p gyre-cli bootstrap` — 16 passed, 0
  failed; `cargo test -p gyre-server --lib -- users:: orchestrator::` —
  18 passed, 0 failed; `cargo test -p gyre-server --lib -- spec_registry`
  — 62 passed, 0 failed. Mechanical gates all exit 0 at HEAD: arch,
  task-commit-attribution, inert-enforcement, relative-path-defaults,
  scope-literal-defaults, migration-versions.
- Pre-existing on the incoming base `75ada1c4` (reproduced in a temp
  worktree and again at branch HEAD, not introduced by this branch):
  check-cli-spec-parity exits 1 on the spec_assist SSE display path
  (`main.rs:1235`); check-assertionless-tests exits 2 after its header.
- Revision-round commits were additionally built and tested in their
  original sandbox per the task operational history; full workspace
  suites, all-target Clippy, and GitHub CI remain with verification and
  publication.
