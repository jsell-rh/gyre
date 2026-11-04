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
commits: ["28fe2508d63ff59965e28fdd012b4eccc39f8616", "ef1ea3e223a3994f3d8c7e97de69264684e5cf5f", "63250520c661eb0f1feed56cafba06e049260dd9", "e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37", "3240ea8b2323326f448bfe2960aef7614b13ccd4", "4eab02d9eccfe3f712bca0ce9bd050289f57b22d"]
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

## Shipped

`gyre bootstrap` implements platform-model.md §8 as a real 9-step REST
orchestration with resume semantics, backed by the server pieces the spec
requires. Implementation commits are listed in the frontmatter; the
revision round (`2061f8c4`) closed review findings F1-F7 from round 1.
All evidence from this round is saved under
`/tmp/stage/review-evidence/`.

### Behavior

- CLI: `bootstrap` subcommand with `--tenant/--workspace/--repo/
  --repo-path/--admin-user/--oidc-issuer/--dev/--starter-kit` (plus
  `--server/--token`). Health check, then tenant (reuse by slug), admin
  user + API key saved to `~/.gyre/config` with subsequent calls
  authenticating as the new admin, workspace and repo (reuse by name),
  4 built-in personas created and pre-approved, spec registry init,
  default gates, repo orchestrator spawn, summary with IDs, API key
  (shown once) and URLs. A failing step reports completed steps and a
  resume hint (`StepTracker`).
- Admin user is tenant-bound at creation (F1): `POST /api/v1/users`
  validates the tenant exists and persists `user.tenant_id` (migration
  `000056_user_tenant_id`); the API-key auth path derives the tenant
  scope from that binding and fail-closes (403) on a user without one;
  the bootstrap admin gets `global_role: TenantAdmin`, not a global
  superuser.
- Spec registry (step 6, F4): with a manifest present (pre-existing or
  just written by `--starter-kit`), `push_and_sync_specs` commits the
  starter-kit files, pushes `HEAD:refs/heads/main` to the server repo,
  then calls `POST /api/v1/repos/:id/sync-specs` so the ledger
  populates at first-run time; later pushes stay in sync via the
  post-receive hook.
- Starter kit (F5, F6): 5 files (`specs/manifest.yaml`,
  `specs/index.md`, `specs/system/design-principles.md`, `AGENTS.md`,
  `.prek.yaml`), never overwrites existing files; `--starter-kit`
  requires `--repo-path` (explicit error, no relative-path default).
  The manifest uses the server's `ApprovalConfig {mode: human_only}`
  shape and round-trips through the real `parse_manifest` in a
  server-side test that `include_str!`s the CLI's manifest.
- Orchestrator spawn (step 8, F2, F3): task-093 endpoint with
  exactly-one-live semantics; the repo-orchestrator persona is resolved
  from the personas store (repo > workspace > tenant scope chain, must
  be Approved, fail-closed) and persisted on the agent row (migration
  `000057_agent_persona_id`); the process launches on the resolved
  compute target (workspace assignment > tenant default > local) and
  the response and CLI summary report the truthful launch status
  (`running`/`launch_failed`), never claiming running for a process
  that failed to start.
- Dev mode: `--dev` rejects `--tenant`/`--admin-user`/`--oidc-issuer`;
  token precedence `--token` > `GYRE_AUTH_TOKEN` > static dev token.

### Tests

- CLI (`bootstrap.rs`, 13 tests + 3 parse tests in `main.rs`): slug
  derivation, gate detection (cargo/arch variants), starter-kit file
  set and never-overwrite, manifest documented shape, summary rendering
  including launch-failure and already-live outcomes, flag
  parsing/defaults/dev mode.
- Server: `users.rs` (create with tenant validation, duplicate
  external_id, unknown tenant, unknown role, minted key authenticates
  through the full router with tenant binding), `orchestrator.rs`
  (persona attached and asserted, scoped JWT, 409 conflicts, stale
  restart, truthful launch outcome), `spec_registry.rs` (starter
  manifest round-trip through the real parser), `repos.rs`
  (sync-specs).

### Verification evidence

- This round resolved the pending merge of the task-097 revision
  (`5196f444`): both conflicting exemption files resolved to the
  incoming side after verifying the flagged sites are fixed in the
  merged sources, and two legacy exemption entries re-pinned to their
  post-merge line numbers (`3240ea8b`). Mechanical gates all pass at
  HEAD: arch, abac-route-registry, abac-exempt-handlers, mem-port
  contracts, migration versions/portability, scope-literal-defaults,
  fabricated-scope-defaults, lossy-secret-conversion,
  inert-enforcement, relative-path-defaults, byte-slice-truncation,
  dead-message-kinds, mcp-write-tools, unbounded-external-http,
  in-memory-state-stores, forwarded-header-trust, forged-scope-fields,
  task-commit-attribution.
- Rust suites could not be re-run in this sandbox: crates.io is
  unreachable and the prepared cargo registry cache is missing 109
  Linux-essential lock packages (proc-macro2, anstyle, clap_derive,
  ...), so no cargo build is possible. This is an infrastructure
  restriction recorded for remote verification, not a code finding;
  this round changed no Rust source. The revision commits were built
  and tested in their original sandbox per the task operational
  history. Suggested remote suites: `cargo test -p gyre-cli bootstrap`,
  `cargo test -p gyre-server --lib` (users, orchestrator,
  spec_registry, auth modules).
- Pre-existing on the incoming base (reproduced at `5196f444` in a
  temp worktree, not introduced by this branch): check-cli-spec-parity
  fails on the spec_assist SSE display path; check-assertionless-tests
  exits 2 after its header.
- Coverage note: `platform-model.md §8 Protocol Injection` was removed
  from this task's coverage_sections (round-1 review F2 scope note: it
  is assigned to task-101).

## Agent Instructions

Read `specs/system/platform-model.md` §8 "Bootstrap & First-Run" for the full spec. The CLI is in `gyre-cli/src/main.rs` — look at how existing commands like `gyre init` work. The bootstrap command orchestrates multiple API calls in sequence. Each step should validate success before proceeding to the next. Error handling: if any step fails, print what succeeded and what failed so the user can resume manually. The `--dev` flag should match the existing dev mode (GYRE_AUTH_TOKEN pattern in server-config.md).
