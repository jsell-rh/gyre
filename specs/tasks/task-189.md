---
title: "Fix persona scope resolution to walk the real parent chain"
spec_ref: "platform-model.md §2 Scope Resolution"
depends_on: []
progress: complete
coverage_sections:
  - "platform-model.md §Scope Resolution"
commits: ["b36fad006a4becd3fc4e28d17f37409052a94789", "a7e1f558317f2f31199a77cc019ccc2f6f903731", "a977a9175d0f3e6c96172156c7983a2d25cb0803", "8cd3f081bd1a53eab155700f7522799943322237", "2d1e74d949a55a5b166a317d19f5faf2478490e3"]
---

## Spec Excerpt

From `platform-model.md` §2 Scope Resolution:

> When a gate or agent-compose references a persona by name, the forge resolves it with nearest-scope-wins:
>
> 1. Check repo-scoped personas first
> 2. Check workspace-scoped personas
> 3. Check tenant-global personas
> 4. Not found -> error
>
> A repo can override a workspace persona (e.g., repo-specific security review requirements). A workspace can override a tenant persona.

## Problem

`resolve_persona` (`crates/gyre-server/src/api/personas.rs:269-304`) is hollow. It receives a single `scope_id` from the query and clones **that same id** into all three scope variants:

```rust
"Repo" => vec![
    PersonaScope::Repo(scope_id.clone()),
    PersonaScope::Workspace(scope_id.clone()),   // WRONG: repo id used as workspace id
    PersonaScope::Tenant(scope_id.clone()),      // WRONG: repo id used as tenant id
],
```

Workspace-scoped personas key on the **workspace id** and tenant-scoped personas key on the **tenant id** (`PersonaScope` is defined in `crates/gyre-domain/src/workspace.rs`; the persona's `scope` stores the id of the scope it belongs to). Because the handler passes the repo id into the `Workspace`/`Tenant` lookups, `find_by_slug_and_scope` can only ever match an **exact-scope** persona. The nearest-scope-wins fallback to parent scopes never fires — parent-scope personas are unreachable. This is not scope resolution per spec.

## Implementation Plan

1. **Resolve the real parent chain before building the scope list.** The entities already carry parent ids:
   - `Repository.workspace_id` (`crates/gyre-domain/src/repository.rs:43`)
   - `Workspace.tenant_id` (`crates/gyre-domain/src/workspace.rs:46`)

   `AppState` exposes the ports needed to walk it: `state.repos` (`RepoRepository`), `state.workspaces` (`WorkspaceRepository`), `state.tenants` (`TenantRepository`) — see `crates/gyre-server/src/lib.rs:197,318-322`.

2. **Rewrite the `scopes_to_try` construction** in `resolve_persona`:
   - `scope_kind == "Repo"`: look up the repo by `scope_id` via `state.repos.find_by_id`. From it derive `workspace_id`, then look up that workspace via `state.workspaces.find_by_id` to derive `tenant_id`. Build the chain with the **correct** id at each level:
     ```
     [ Repo(repo_id), Workspace(repo.workspace_id), Tenant(workspace.tenant_id) ]
     ```
   - `scope_kind == "Workspace"`: look up the workspace by `scope_id`, derive `tenant_id`, build `[ Workspace(workspace_id), Tenant(workspace.tenant_id) ]`.
   - `scope_kind == "Tenant"`: `[ Tenant(tenant_id) ]` (unchanged; the id already is the tenant id).
   - Unknown `scope_kind`: keep the existing `ApiError::InvalidInput` branch.

3. **Handle missing parents cleanly.** If the repo or workspace referenced by `scope_id` does not exist, return `ApiError::NotFound` with a descriptive message (do not silently skip a scope level — a bad id is a client error, not a not-found persona).

4. **Confirm `PersonaScope` construction matches storage.** Verify against `find_by_slug_and_scope` in the SQLite/Postgres persona adapters (`crates/gyre-adapters`) that the scope discriminator + id are compared exactly as constructed here. If `PersonaScope::Tenant` wraps `Id` while personas are seeded/stored with a different discriminant, align the query — do not paper over a mismatch.

## Acceptance Criteria

- [ ] `resolve_persona` walks the actual parent chain: repo → its workspace → that workspace's tenant, using the entities' real parent ids, not a cloned `scope_id`.
- [ ] A persona defined **only** at workspace scope resolves when queried with `scope_kind=Repo&scope_id=<repo in that workspace>`.
- [ ] A persona defined **only** at tenant scope resolves when queried with `scope_kind=Repo` (repo whose workspace belongs to that tenant) and with `scope_kind=Workspace` (workspace in that tenant).
- [ ] Nearest-wins precedence holds: a repo-scoped persona with the same slug shadows a workspace- or tenant-scoped one; a workspace-scoped one shadows a tenant-scoped one.
- [ ] Querying a `scope_id` whose repo/workspace does not exist returns `NotFound` (not a spurious persona miss).
- [ ] `scope_kind=Tenant` still resolves tenant-global personas directly.

## Tests (must fail before the fix, pass after)

Add integration tests in the `#[cfg(test)] mod tests` block of `personas.rs` (uses `test_state()` / `api_router()`):

- **Workspace fallback:** seed a tenant, workspace, repo, and an **approved** persona at `PersonaScope::Workspace(workspace_id)` with slug `security`. Query `resolve?scope_kind=Repo&scope_id=<repo_id>&slug=security` → 200, returns the workspace persona. This test MUST fail against the current cloned-id logic (repo id ≠ workspace id) and pass after.
- **Tenant fallback:** seed only a `PersonaScope::Tenant(tenant_id)` persona `security`; query with the repo id → 200 returns it.
- **Nearest-wins shadowing:** seed both a `Workspace` and a `Repo` persona with slug `security` (distinct system prompts); query with the repo id → returns the **repo** one.
- **Bad scope id:** query `scope_kind=Repo&scope_id=<nonexistent>` → `NotFound`.

Do not write self-confirming tests: seed personas with distinct `system_prompt`/`id` values and assert the resolved body is the parent-scope one, so a regression to exact-scope-only matching fails the test.

## Agent Instructions

- Read `crates/gyre-server/src/api/personas.rs:260-304` (handler) and the test module below it for the existing test harness (`test_state`, `body_json`, seeding pattern).
- Read `crates/gyre-domain/src/workspace.rs` for `PersonaScope` and `Workspace.tenant_id`; `crates/gyre-domain/src/repository.rs` for `Repository.workspace_id`.
- Read `crates/gyre-ports/src/` persona/repo/workspace/tenant port traits to confirm `find_by_id` / `find_by_slug_and_scope` signatures.
- Confirm the route `GET /api/v1/personas/resolve` registration in `crates/gyre-server/src/api/mod.rs` before relying on the path in tests.
- Do NOT change the query contract (`scope_kind`, `scope_id`, `slug`); only fix the resolution logic behind it.
- Skip project-wide lint/format/test suites; run `cargo test -p gyre-server personas` (or the crate's persona tests) to validate.

## Shipped

`resolve_persona` (`crates/gyre-server/src/api/personas.rs`) now walks the real parent chain instead of cloning the queried `scope_id` into every scope variant:

- `scope_kind=Repo` loads the repo via `state.repos.find_by_id` and builds `[Repo(repo_id), Workspace(repo.workspace_id), Tenant(workspace.tenant_id)]` — the workspace id comes from the repo entity, the tenant id from loading that workspace via `state.workspaces.find_by_id`. `scope_kind=Workspace` derives `[Workspace(workspace_id), Tenant(workspace.tenant_id)]` the same way; `scope_kind=Tenant` passes through. Unknown `scope_kind` keeps the `InvalidInput` branch.
- A repo/workspace id that does not exist is an entity `NotFound` ("repo/workspace '…' not found"), not a persona miss — a bad id is a client error, not a silently skipped scope level.
- Nearest-wins holds: the scope chain is ordered Repo → Workspace → Tenant and the first `find_by_slug_and_scope` hit wins, so a repo persona shadows a workspace persona shadows a tenant persona with the same slug.
- Adapter parity verified (plan step 4): both SQL adapters store and query `serde_json::to_string(scope)` on the personas table (`sqlite/workspace.rs:387-480`, `postgres/workspace.rs:387-480`), so handler-constructed `PersonaScope` values match stored rows exactly; the mem adapter compares the enum directly (`mem.rs:1708-1720`). No mismatch to align.
- Query contract unchanged: `scope_kind`/`scope_id`/`slug`, route registration (`api/mod.rs:766`), and the `PersonaResponse` shape are untouched.

Test evidence (`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib personas` at `b36fad00`, logs in `/tmp/stage/review-evidence/`):

- **8 passed, 0 failed** — 3 pre-existing persona tests plus the 5 new resolve tests: `resolve_repo_scope_falls_back_to_workspace_persona`, `resolve_repo_scope_falls_back_to_tenant_persona`, `resolve_workspace_scope_falls_back_to_tenant_persona`, `resolve_prefers_nearest_scope`, `resolve_unknown_scope_entity_is_not_found`.
- **Mutation probe (pre-fix regression proof):** the handler's chain construction was temporarily reverted to the pre-fix cloned-`scope_id` logic exactly as the task's Problem section quotes it; the suite failed **4 of 5** resolve tests (both repo→parent fallbacks, workspace→tenant fallback, and the bad-scope-id discrimination — it got the persona-miss 404 instead of the entity-not-found 404). `resolve_prefers_nearest_scope` passing under mutation is expected: exact-scope precedence was never broken; that test guards order-inversion of the fixed chain. Handler restored and tree verified clean afterward.
- Tests are not self-confirming: `scoped_state()` seeds distinct ids `t1`/`ws1`/`r1`, and each fallback test asserts the resolved body's `id`/`system_prompt`/`scope` is the parent-scope persona, so a regression to exact-scope-only matching fails them.

Repair over the interrupted checkpoint: the recovered implementation (commit `a7e1f558`, functionally identical to reviewed `a977a917`) carried rustfmt violations on changed lines, which fail CI's `check-rustfmt-diff.py HEAD^1` against base `8c2d1775` (lines 307-309 handler Workspace branch, 499-513 test helpers, 656-684 error-message asserts). `b36fad00` applies pure rustfmt reformatting — no logic or test-semantics change — and the gate now passes ("changed lines clean, 1 Rust file checked"). The `web/dist` churn from the first compile's embedded `npm run build` was reverted; task branches do not ship dist rebuilds.

Out of scope / not weakened: `resolve_persona` returns personas regardless of `approval_status` (the cited spec section has no approval requirement; approval semantics are the Persona Lifecycle row) and does not check caller membership in the target tenant/workspace (matches the platform's M34 persona-read model; the cited section says nothing about caller scoping). Both were flagged as conscious decisions by the prior review round and are restated here so they stay visible.
