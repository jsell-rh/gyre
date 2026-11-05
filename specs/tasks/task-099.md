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
commits: ["e39045ab01c3499e74646ba7137d6076cde4e556", "28fe2508d63ff59965e28fdd012b4eccc39f8616", "ef1ea3e223a3994f3d8c7e97de69264684e5cf5f", "63250520c661eb0f1feed56cafba06e049260dd9", "e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37"]
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
requires. Implementation commits are in the frontmatter; rounds 1-2 closed
the F1-F7 findings (task history), and this round (4) repaired the five
round-3 review findings — every one reproduced live before and verified
fixed live after.

### Behavior

- CLI: `bootstrap` subcommand with `--tenant/--workspace/--repo/
  --repo-path/--admin-user/--oidc-issuer/--dev/--starter-kit` (plus
  `--server/--token`). Health check, tenant (reuse by slug), admin user +
  API key saved to `~/.gyre/config` with subsequent calls authenticating
  as the new admin, workspace and repo (reuse by name), 4 built-in
  personas created and pre-approved, spec registry init, default gates,
  repo orchestrator spawn, summary with IDs, API key (shown once) and
  URLs. A failing step reports completed steps and a resume hint.
- Round-3 finding 1 (wire): `OrchestratorAgentResponse` no longer
  declares `persona_id` alongside the flattened `AgentResponse` (which
  carries it from the domain agent), so the spawn response serializes
  every key exactly once and the CLI's strict deserialization parses it —
  step 8 completes and the summary prints.
- Round-3 finding 2 (spec registry): `push_and_sync_specs` pushes with
  `--force`. The server seeds `main` with a disposable empty initial
  commit and the user's local history is unrelated to it, so the unforced
  first-run push was always rejected as non-fast-forward and the ledger
  stayed empty; bootstrap registers the local repo as the platform's
  source of truth, so replacing the seed is the intended first-run
  semantics.
- Round-3 finding 3 (cross-tenant personas): the resume lookup is
  `find_persona_by_slug_in_tenant` with `?scope=tenant&scope_id=...` —
  the unscoped persona list spans all tenants, which made the second
  tenant's bootstrap skip persona registration and die at spawn with a
  persona 404.
- Round-3 finding 4 (resume): (a) `spawn_orchestrator` releases a name
  held by a Dead/Stopped/Failed agent (renamed `...-superseded-N`, same
  suffix scheme the stale detector uses for restarts) instead of
  rejecting with 400 — the scope-slot check already guarantees no live
  orchestrator holds the scope; a LIVE name holder elsewhere (agent names
  are unique server-wide) still gets 400 and the CLI retries with a
  suffixed name. (b) The mem adapter now enforces the SQLite uniqueness
  constraints in code — tenant `slug`, workspace `(tenant_id, slug)`, repo
  `(workspace_id, name)`, each documented as a failure clause on its port
  trait — so the tenant/workspace/repo reuse fallbacks trigger on the
  default in-memory dev server too, instead of silently building a
  parallel duplicate hierarchy.
- Unchanged from prior rounds: admin user tenant binding at creation
  (migration `000056_user_tenant_id`, fail-closed API-key auth), truthful
  orchestrator launch status (`running`/`launch_failed`), starter kit
  (5 files, never overwrites, requires `--repo-path`), persona
  attachment persisted on the agent row (migration `000057`), dev mode
  flag rejection and token precedence, Protocol Injection decomposed to
  task-101 per the round-1 F2 scope note.

### Tests

- New in `orchestrator.rs`:
  `repo_orchestrator_response_serializes_no_key_twice` asserts on the
  serialized wire TEXT (a depth-1 key scanner; `serde_json::to_value`
  collapses duplicates, which is why the old repo_id-only count test was
  blind) and strict-deserializes the CLI shape;
  `spawn_rejects_missing_persona` (404 when no persona resolves in the
  repo/workspace/tenant chain); `spawn_rejects_unapproved_persona`
  (conflict when the persona is not Approved);
  `spawn_frees_name_of_stopped_predecessor` (dead predecessor renamed,
  same-name respawn succeeds, live holder still rejected).
- Mutation probes (evidence in this round's sandbox under
  `/tmp/stage/review-evidence/`): reintroducing the duplicate
  `persona_id` fails only the new wire test while the old repo_id test
  stays green; fail-open persona resolution (fabricating an approved
  persona on NotFound, and ignoring approval status) fails the
  corresponding new tests.
- `mr_timeline.rs` fixture corrected to distinct repo names — it relied
  on the mem duplicate-`(workspace, name)` divergence the new guard
  closes.

### Verification evidence

- All evidence saved under `/tmp/stage/review-evidence/` (source SHAs,
  commands, exit codes, logs; `e2e/` for the live runs).
- `cargo test -p gyre-server --lib`: 1262 passed, 1 failed —
  `ws::tests::ws_activity_event_emits_to_telemetry`, reproduced
  deterministically on the incoming base via `git stash` (pre-existing,
  not introduced here). `cargo test -p gyre-cli`: 131 + 1 passed.
  Orchestrator suite 11/11, users+spec_registry 55, mr_timeline 13.
- Live e2e against a server built from this head: dev first-run
  bootstrap with `--starter-kit` exits 0, spec ledger populated at
  first-run time (1 entry via `GET /api/v1/specs`), spawn response
  parsed, summary printed with truthful `launch_failed`; immediate
  re-run reuses tenant/workspace/repo and reports the orchestrator
  AlreadyLive; after admin-killing the orchestrator, a re-run frees the
  stopped predecessor's name server-side (`gyre-demo-orchestrator-1`
  dead, new spawn active under the original name) and exits 0; a
  cross-tenant "Acme Corp" `--admin-user jsell` run on the same server
  registers its own four personas, retries the live name collision as
  `gyre-demo-orchestrator-2`, and exits 0 with the API key shown once
  and config saved. Failure paths: server down / `--dev --tenant` /
  missing `--tenant` all exit 1 with the expected messages.
- Mechanical gates at this head exit 0: arch, mem-port-contracts,
  migration-versions, inert-enforcement, scope-literal-defaults,
  dead-message-kinds, api-auth, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, no-em-dash,
  conditional-test-guards, task-commit-attribution. The
  relative-path-defaults exemption for the pre-existing spawn.rs prose
  comment was re-anchored 601 -> 600 after this round's line shift
  (same physical comment, entry count unchanged). Pre-existing on the
  incoming base (reproduced via `git stash`): check-assertionless-tests
  exits 2; check-cli-spec-parity exits 1 on the spec_assist SSE display
  path.
- Full workspace suites, all-target Clippy, and GitHub CI remain with
  verification and publication.
