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
commits: ["aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37"]
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

**Original round (`c903a80b`):** `gyre bootstrap` CLI subcommand + server
`POST /api/v1/users` (Admin-gated, CSPRNG key, only the SHA-256 hash
persisted, `external_id = "local:{username}"`), pure logic in
`gyre-cli/src/bootstrap.rs`, orchestration in `main.rs::run_bootstrap` with
`StepTracker`. Resumability added in `c2755e1b` (find-by-slug reuse, saved
credential reuse, 409 on orchestrator spawn kept as AlreadyLive).

**Revision round (this round, F1-F7):**

- **F1 (admin tenant binding):** `CreateUserRequest` gained a required
  `tenant_id`, the handler loads the tenant and rejects an unknown one, and
  the user row carries the binding (migrations `2026-10-09-000056_user_tenant_id`
  + adapters/schema). The auth extractor's API-key path now resolves the
  tenant from `user.tenant_id` and fail-closes (403) on an unbound user;
  OIDC `find_or_create_user` binds the validated tenant claim at creation.
  Server-originated messages (`emit_event`/`emit_telemetry`) resolve the
  tenant from the workspace record instead of fabricating `"default"`.
- **F2 (persona attachment):** `Agent.persona_id` (migration
  `2026-10-09-000057_agent_persona_id`) is set at orchestrator spawn.
  `resolve_orchestrator_persona` consults `state.personas` (the store
  bootstrap step 5 populates) with nearest-scope-wins Repo > Workspace >
  Tenant, and a missing or unapproved persona REJECTS the spawn (Result,
  consumed — not a warn-logged discard).
- **F3 (truthful launch):** `spawn_orchestrator` launches a real process via
  `launch_orchestrator_process` (compute-target priority workspace → tenant
  default → local; server-controlled command only) and returns a
  `LaunchOutcome` surfaced in the REST/MCP response (`launch_status`
  `running`/`launch_failed` + detail). The CLI summary reports exactly what
  happened; `launch_failed` never prints "running". Auto-restart + the stale
  detector remain the recovery path.
- **F4/F5 (spec registry):** step 6 now pushes the local tree to the server
  repo (committing uncommitted starter-kit files first, starter-kit paths
  only) and calls the new `POST /api/v1/repos/:id/sync-specs`, which re-runs
  `sync_spec_ledger` against the default-branch HEAD at first-run time.
  `STARTER_MANIFEST` now emits `approval: {mode: human_only}` — parseable by
  the server's `SpecEntry` schema (tested).
- **F6 (starter-kit path):** `--starter-kit` without `--repo-path` is
  rejected with an explanation instead of writing to a relative
  `./<repo-name>/` path.
- **F7 (auth-boundary test):** `api_key_authenticates_through_full_router_with_tenant_binding`
  exercises the minted key through the full router (`GET /api/v1/users/me`,
  `Authorization: Bearer <raw key>`) and asserts identity + tenant binding.
  These notes describe the shipping design (previous notes described the
  superseded `c903a80b` task+spawn flow).

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
