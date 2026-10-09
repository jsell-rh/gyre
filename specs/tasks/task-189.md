---
title: "Fix persona scope resolution to walk the real parent chain"
spec_ref: "platform-model.md §2 Scope Resolution"
depends_on: []
progress: not-started
coverage_sections:
  - "platform-model.md §Scope Resolution"
commits: []
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
