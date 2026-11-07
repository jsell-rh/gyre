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
commits: ["6131513c4a6972daa827ec025741cd4d70a0c790", "5d487f44cef3dffadb85626575a4fa34daf4e61d", "e39045ab01c3499e74646ba7137d6076cde4e556", "28fe2508d63ff59965e28fdd012b4eccc39f8616", "ef1ea3e223a3994f3d8c7e97de69264684e5cf5f", "63250520c661eb0f1feed56cafba06e049260dd9", "e3fde3a652fb8b6b5acc876e134050ea5e67a547", "6ed19b4ce8eac208cfb6f173b77923c71358fc09", "4566fc4843802c3a1f9057809078ca2bba615721", "aede618804407b47b16f9e76dd434cac4f8e1646", "e54363c8dd92fa53e3fd2a8727fad0bd7a05f20e", "2061f8c4f7cd289e6328897d2c4ddf48be69970c", "c903a80b64fc25fa2fed19822990c95c53452e37", "30c30ca8f89f131f9764e4742d7e53fe5514608e"]
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
requires. Implementation commits are in the frontmatter. Rounds 1-4 closed
the F1-F7 and round-3 review findings; round 5 repaired the
`80ce5b90` test regression (ws.rs fabricated-workspace fallback and
merge-broken presence fixtures); round 6 repaired the review finding
that candidate-changed lines violated the repo's changed-lines rustfmt
gate (`scripts/check-rustfmt-diff.py` exit 1 in 11 files); this round
(7, commit `30c30ca8`) resolved the rebase onto `4d6759a0` (task-084
plus the task-077 revision), which conflicted in three files.

### Behavior

- CLI: `bootstrap` subcommand with `--tenant/--workspace/--repo/
  --repo-path/--admin-user/--oidc-issuer/--dev/--starter-kit` (plus
  `--server/--token`). Health check, tenant (reuse by slug), admin user +
  API key saved to `~/.gyre/config` with subsequent calls authenticating
  as the new admin, workspace and repo (reuse by name), 4 built-in
  personas created and pre-approved, spec registry init, default gates,
  repo orchestrator spawn, summary with IDs, API key (shown once) and
  URLs. A failing step reports completed steps and a resume hint.
- Server pieces: `POST /api/v1/users` (admin-only, mints an
  authenticating API key, binds the admin to the tenant — migration
  `000056_user_tenant_id`, fail-closed API-key auth), persona attachment
  persisted on the agent row (migration `000057`), spawn wire without
  duplicate keys, first-run spec-ledger push with `--force`, mem adapter
  enforcing the SQLite uniqueness constraints, truthful orchestrator
  launch status, starter kit (5 files, never overwrites, requires
  `--repo-path`), dev mode flag rejection and token precedence, Protocol
  Injection decomposed to task-101 per the round-1 F2 scope note.
- ws.rs Activity telemetry scoped to a real workspace
  (`tenant_default_workspace()` via `WorkspaceRepository::list_by_tenant`,
  skip-and-log when unresolvable — the check-fabricated-scope-defaults
  pattern), never a fabricated `default-{tenant}` id.
- Round-6 fix (commit `6131513c`): purely mechanical rustfmt pass over
  the 11 files whose changed lines failed the gate plus `client.rs`
  (formatted as a child module of `main.rs`). No semantic changes; the
  diff collapses the `tenant_default_workspace` signature (ws.rs),
  reflows the stray `).await;` left by the emit_telemetry
  sync-to-async change (mcp.rs), and reflows the remaining changed
  lines to rustfmt defaults.
- Round-7 (rebase resolution, commit `30c30ca8`): merged base
  `4d6759a0` into the branch and resolved the three conflicts —
  (a) `gyre-cli/src/main.rs` step 6 kept the candidate's real
  `push_and_sync_specs` implementation over the base's older
  report-only block (the base contributed nothing else to the CLI;
  bootstrap.rs/client.rs auto-merged back to the candidate versions);
  (b) `inert-enforcement-exemptions.txt` dropped BOTH sides' entries —
  each side had fixed the other's finding (base bound+branched the
  `evaluate_attestation_abac` results in merge_processor.rs; candidate
  removed the void `validate_persona` calls in api/orchestrator.rs), so
  the merged tree has zero exempted sites and the file keeps only its
  header; (c) `relative-path-defaults-exemptions.txt` dropped the
  candidate's spawn.rs:600 entry — the merged spawn.rs (base's task-077
  F6 rewrite) no longer contains the `"./repos/"` prose comment it
  exempted.

### Tests

- Server: user-creation handler tests (key authenticates as new user,
  duplicate rejected, unknown role rejected), orchestrator
  wire/persona/spawn-name tests, ws activity tests
  (`ws_activity_event_emits_to_telemetry` scoped to the seeded
  workspace, `ws_activity_event_without_tenant_workspace_skips_emission`
  pinning skip-and-log), presence fixtures bound to the caller's tenant.
- CLI: 11 bootstrap tests (parse/flags, slug, gates, starter kit,
  manifest shape, summary rendering, persona coverage).
- This round changed formatting only; suites re-run to confirm no
  behavior drift: `cargo test -p gyre-server --lib ws::` — 82 passed,
  0 failed; `cargo test -p gyre-cli` — 131 + 1 passed, 0 failed;
  `cargo check -p gyre-server -p gyre-cli` — exit 0.
- Round-7 merge-resolution probes (merged tree): `cargo check -p
  gyre-cli -p gyre-server` — exit 0; `cargo test -p gyre-cli` — 131 + 1
  passed, 0 failed; `cargo test -p gyre-server --lib merge_processor`
  — 60 passed, 0 failed; `cargo test -p gyre-server --lib` — 1363
  passed, 0 failed; `check-inert-enforcement.sh` and
  `check-relative-path-defaults.sh` — both exit 0 against the resolved
  exemption files (proving no unexempted violation and no stale entry
  needed).

### Verification evidence

- Evidence under `/tmp/stage/review-evidence/` (source SHAs, commands,
  exit codes, logs; see its README.md). Round-7 (rebase resolution)
  evidence is in that directory's current contents: README.md plus the
  five probe logs listed in it (both exemption check scripts, cargo
  check, cli tests, merge_processor tests, server lib suite).
- Round-7 merged base `4d6759a0` (task-084 + task-077 revision) into
  the branch; conflicts resolved per the Behavior section. The merged
  tree keeps the candidate's bootstrap behavior unchanged (the base
  contributed no competing CLI code), and the exemption-file
  resolutions are proven minimal by both check scripts exiting 0.
- `python3 scripts/check-rustfmt-diff.py 1ae18d1c` (the finding's
  reproduction base) — exit 0, "changed lines clean (48 Rust files
  checked)"; also exit 0 against `86a98a29` (HEAD^1, the base CI's fmt
  job uses on merge — 32 files checked).
- Pre-fix reproduction: exit 1, 11 files (log `rustfmt-diff-before.log`).
- Pre-existing on the incoming head (not introduced here, unchanged):
  `cargo clippy -p gyre-domain --lib` errors in rust_extractor.rs,
  check-assertionless-tests exit 2, check-cli-spec-parity exit 1 on the
  spec_assist SSE display path (per round-4 stash reproduction).
- Round-4's live e2e (dev first-run bootstrap with `--starter-kit`,
  cross-tenant `--admin-user jsell`, failure paths) was run against
  round-4's head; rounds 5-6 changed ws.rs test fixtures/scoping and
  formatting only, none of which the bootstrap REST orchestration
  exercises.
- Full workspace suites, all-target Clippy, and GitHub CI remain with
  verification and publication.
