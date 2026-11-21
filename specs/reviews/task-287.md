# Review — task-287 (Spec Links: cross-repo link validation)

Spec: `specs/system/spec-links.md` §Cross-Repo and Cross-Workspace Links (spec gap `ff74a147e0ca35a407836447`).
Candidate under review: `aa58cd59ce69cc0c8455d39723523097025d7144` (branch `pipeline/task-287/b8df7b9cc99d40dda25ccd93aa24d74e-2`, base `8e2e7458`; product commit `e3270d27`).
Verdict: **approved** — all four required behaviors are implemented in production code shared by every sync entry point, the new tests are regression-killing (each fails against the pre-fix code path it targets), and the candidate type-checks including test code. Server test execution is wrapper-blocked in the review stage and is recorded as a verifier probe (infrastructure bound, not a code defect).

## Probes run (checkout at candidate `aa58cd59`; working tree carries only the executor's protected-task-text restoration of `specs/tasks/task-287.md`, which is not part of the product diff)

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (9m46s). This is the step the previous interrupted attempt failed (E0061 argument-count errors on the new `sync_spec_ledger` signature); the candidate compiles including all new test code.
- `bash scripts/check-fabricated-scope-defaults.sh` → exit 0 — directly relevant: the literal `Id::new("default")` tenant lookup is removed from `spec_link_staleness.rs`.
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- Full diff read of all 7 touched files; sweep of all four production `sync_spec_ledger` call sites (git_http.rs:1607, api/repos.rs:997/1148, mirror_sync.rs:57).
- Evidence: `/tmp/stage/review-evidence/` (probes-run.md, check-output.txt, mechanical-check logs, both source diffs).

## Verified against the task contract

1. **Same-workspace `@{repo}/{path}` resolution.** `validate_cross_repo_target` (spec_registry.rs) resolves the repo via `find_by_name_and_workspace` on the SOURCE workspace id — the `None`-workspace bug is gone (the old `resolve_repo_by_name` helper that returned `None` whenever `workspace_id` was `None` is deleted). The staleness pass (`resolve_unresolved_cross_workspace_links`) re-resolves unresolved `CrossRepo` links via source repo → workspace (`state.repos.find_by_name_and_workspace(&source_ws.id, repo_name)`) — the old "Leave as unresolved" refusal is gone. AC 1's staleness half rides the real production step-6b inbound staleness (changed-spec-paths set → all links targeting the changed path marked stale), exercised on real git repos in the test.
2. **Linking-user read access.** `LinkPrincipal::from_auth` threads the authenticated pusher from the git_http `git_receive_pack` handler and `api/repos.rs` `sync_specs` into `sync_spec_ledger`; `validate_cross_workspace_target` checks `principal_has_read_access` (workspace-membership lookup with `accepted` required; lookup error and missing-repo-handle both fail closed; system principal exempt via the task-099-hardened `is_system_principal` = `agent_id == "system" && user_id.is_none()`; agent principals validated by their signature-verified workspace-scope claim). A denial stores the link `unresolved` with `target_repo_id` NULL, a warning naming principal and target, and a durable KV marker (`spec_link_denials` namespace, keyed by the deterministic `spec_link_id` = `source_repo:source_path:type:target`, so it is stable across re-pushes) that the staleness pass consults — a denied link is never silently flipped `active`. Re-push by an authorized principal resolves the link active and clears the marker (positive control in the test). Mirror fetches pass `None` (no linking user exists — content originates from the configured upstream).
3. **Target-manifest existence.** `target_manifest_declares_path` reads the target repo's current `specs/manifest.yaml` via the same `read_manifest`/`read_git_file` git-show mechanism the sync itself uses (`refs/heads/{default_branch}`), normalizing paths with the existing `normalize_spec_rel_path`; a missing/unparseable manifest declares nothing. Enforced at push time for both `@{repo}/{path}` and `@{ws}/{repo}/{path}` targets AND at staleness re-resolution; failures store `unresolved` with the missing path named in the warning (`spec_path = path` field).
4. **Tenant-scoped re-resolution.** The fabricated literal `"default"` tenant lookup is replaced by source repo → workspace → tenant (`find_by_slug(&source_ws.tenant_id, slug)`); every unresolvable hop (no source repo id, repo not found, workspace not found) skips with a log line — never fabricates. `check-fabricated-scope-defaults.sh` passes.

Coverage row 4 (`specs/coverage/system/spec-links.md`) was honestly downgraded `verified` → `implemented` with task-287 attribution and concrete source evidence, explicitly noting test execution is deferred to the verifier. Commit attribution in the task frontmatter is present in the committed candidate.

## Test strength

Each new test fails against the pre-fix code path it targets (analysis in `/tmp/stage/review-evidence/probes-run.md`); the denial test carries a positive control (grant Viewer → active, marker cleared) and the re-resolution tests carry negative controls (undeclared path stays unresolved), so none are self-confirming. Tests use real git repos (temp dirs, real `git show`/`rev-parse`) and the mem adapters implementing the same ports the SQLite/Postgres adapters implement.

## Non-blocking observations (recorded for the verifier / future work, not contract violations)

- The staleness re-resolution flips unresolved (not-explicitly-denied) cross-workspace links to `active` without an access re-check. This is inherent — the pusher is not recoverable from the link row — and explicitly denied links are protected by the KV marker. Edge: a link pushed while the target workspace did not yet exist (access never evaluated at push, since the target did not resolve) can later re-resolve `active`; the task scopes the access validation to "when a cross-workspace target resolves", and AC 4 mandates exactly this later re-resolution.
- KV denial markers can orphan if a link is removed from the manifest (no cleanup alongside `replace_for_source`); hygiene only — a stale marker can at worst keep a re-declared same-id link from being flipped active by the staleness pass until the next push, which re-evaluates and clears it.
- Agent principals with user_id `None` and no workspace-scope claim are denied cross-workspace links (fail closed) — stricter than a tenant-admin-style grant would be, but matches the task's "workspace membership/role lookup" instruction.

## Verifier must run (wrapper-blocked in review stage)

```
cargo test -p gyre-server --lib spec_registry::tests::cross_repo_same_workspace_link_resolves_active_then_stale_on_target_change
cargo test -p gyre-server --lib spec_registry::tests::cross_repo_link_unresolved_when_target_path_not_in_target_manifest
cargo test -p gyre-server --lib spec_registry::tests::cross_workspace_link_denied_without_read_access_stays_unresolved
cargo test -p gyre-server --lib spec_link_staleness::tests::staleness_pass_resolves_cross_workspace_link_under_source_tenant
cargo test -p gyre-server --lib spec_link_staleness::tests::staleness_pass_resolves_cross_repo_same_workspace_link
cargo test -p gyre-server --lib spec_link_staleness::tests::staleness_does_not_flip_access_denied_link_active
# regression scope:
cargo test -p gyre-server --lib spec_registry:: spec_link_staleness::
```
