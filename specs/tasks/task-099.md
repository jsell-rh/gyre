---
title: "Platform Model Bootstrap Command"
spec_ref: "platform-model.md §8 Bootstrap & First-Run"
depends_on: []
progress: needs-revision
review: specs/reviews/task-099.md
coverage_sections:
  - "platform-model.md §8 Bootstrap & First-Run"
  - "platform-model.md §8 gyre bootstrap CLI Command"
  - "platform-model.md §8 What It Does"
  - "platform-model.md §8 Starter Kit"
  - "platform-model.md §8 Protocol Injection"
commits: ["e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37"]
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

- **Server**: `POST /api/v1/users` in `crates/gyre-server/src/api/users.rs::create_user`
  — Admin-only (per-handler role check), REQUIRES `tenant_id` referencing an
  existing tenant (the user is scoped at creation; a typo'd or foreign id is
  rejected), mints an authenticating API key in the same call (stored via
  `state.api_keys`, the store the auth extractor consults; only the SHA-256
  hash is persisted). `external_id = "local:{username}"` gives stable
  duplicate detection. Migration `2026-10-09-000056_user_tenant_id` adds the
  `users.tenant_id` column; the auth extractor resolves API-key tenant scope
  from `user.tenant_id` and fail-closes (403) on unbound users instead of
  fabricating "default" (F1).
- **CLI**: `crates/gyre-cli/src/bootstrap.rs` holds pure logic (persona prompt
  registry via `include_str!`, slug derivation, gate detection, starter-kit
  writer, summary renderer); orchestration lives in `main.rs::run_bootstrap`
  with a `StepTracker` reporting completed steps + resume hint on failure.
- **Client**: `GyreClient` methods for every step (`health`, `create_tenant`,
  `create_user`, `create_workspace`, `create_repo`, `create_persona`,
  `approve_persona`, `create_gate`, `find_*` resume lookups, `sync_specs`,
  `spawn_repo_orchestrator`).
- **Personas**: `specs/personas/repo-orchestrator.md` authored (referenced by
  platform-model.md §3 but previously missing); all four prompts embedded in
  the CLI at `crates/gyre-cli/src/bootstrap/personas/`.
- **Spec registry step (F4/F5)**: REAL — `push_and_sync_specs` pushes the
  local checkout to the server's bare repo (committing starter-kit files
  first when `--starter-kit` left them uncommitted), then calls
  `POST /api/v1/repos/:id/sync-specs`, which runs the same
  `sync_spec_ledger` the post-receive hook runs. The starter manifest
  (`bootstrap.rs::STARTER_MANIFEST`) uses `approval: {mode: human_only}` —
  the `ApprovalConfig` shape the server's manifest parser requires.
- **Orchestrator spawn (F2/F3)**: uses the task-093 orchestrator endpoint
  `POST /api/v1/repos/:id/orchestrator/spawn`. The server resolves the
  `repo-orchestrator` persona from `state.personas` (repo > workspace >
  tenant scope chain, fail-closed on missing or unapproved), persists
  `agent.persona_id` (migration `2026-10-09-000057_agent_persona_id`), and
  launches the orchestrator process on the workspace's compute target
  (mirroring `api/spawn.rs`: target config -> `GYRE_ORCHESTRATOR_COMMAND` ->
  `GYRE_AGENT_COMMAND` -> local `/gyre/entrypoint.sh`). The spawn response
  carries `launch_status` ("running"|"launch_failed") + `launch_detail`;
  the CLI summary reports the truthful outcome and never claims "running"
  for a row whose process failed to launch.
- **Starter kit (F6)**: `--starter-kit` requires `--repo-path` (error, not a
  cwd-relative default).
- Tests: server — orchestrator persona attach + launch outcome + scoped JWT
  tests (`api/orchestrator.rs`), `create_user` handler tests incl. full-router
  API-key auth with tenant binding (`api/users.rs`); CLI — parse/flags, slug,
  gates, starter kit, manifest shape (parses as the server's `SpecManifest`),
  summary rendering (running / launch_failed / already-live), persona
  coverage, spawn-response parsing.

## Verification (revision round)

- `cargo test -p gyre-server orchestrator` — persona attach, scoped JWTs,
  409 exactly-one-live, stale-detector restart, escalation, truthful launch
  outcome (all in `api/orchestrator.rs` tests).
- `cargo test -p gyre-server users::` — create_user + API-key auth through
  the real router with tenant binding, duplicate/unknown-tenant/unknown-role
  rejection.
- `cargo test -p gyre-cli bootstrap` — CLI-side pure-logic tests.
- `sqlite3` probe: both new migrations apply cleanly on a fresh database and
  leave `users.tenant_id` / `agents.persona_id` in place (regression caught:
  the first cut of 000056's up.sql accidentally contained the down statement
  and dropped the column it just added; fixed in this round).
- Check scripts affected by the revision: `check-inert-enforcement.sh` F2
  exemptions removed (`validate_persona` is gone), frozen exemption counts
  updated DOWNWARD (scope-literal 24 -> 13; task-099-owned sites removed,
  none added). Full script battery + end-to-end smoke test owned by
  verification.

## Agent Instructions

Read `specs/system/platform-model.md` §8 "Bootstrap & First-Run" for the full spec. The CLI is in `gyre-cli/src/main.rs` — look at how existing commands like `gyre init` work. The bootstrap command orchestrates multiple API calls in sequence. Each step should validate success before proceeding to the next. Error handling: if any step fails, print what succeeded and what failed so the user can resume manually. The `--dev` flag should match the existing dev mode (GYRE_AUTH_TOKEN pattern in server-config.md).
