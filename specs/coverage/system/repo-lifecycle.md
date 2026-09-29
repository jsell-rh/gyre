# Coverage: Repo Lifecycle

**Spec:** [`system/repo-lifecycle.md`](../../system/repo-lifecycle.md)
**Last audited:** 2026-09-29 (full audit — verified domain fields; flagged partials in creation bootstrap, settings, archive grace period, delete preconditions)
**Coverage:** 15/18 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Where Repo Management Lives | 2 | implemented | - | Admin view with workspace-scoped repos. API routes exist for all repo operations. |
| 3 | Admin → Workspace Scope → Repos Tab | 3 | task-assigned | task-168 | API complete. No dedicated "Repos" tab in Admin web UI — repos managed through workspace settings but not as a distinct admin tab per spec. |
| 4 | Admin → Repo Scope → Settings | 3 | implemented | - | Partial — PUT /api/v1/repos/:id updates name/description/default_branch (repos.rs:216-245). Max concurrent agents and budget override NOT editable (absent from UpdateRepoRequest and Repository struct). RepoSettings.svelte exists. |
| 5 | Explorer → Workspace Scope | 3 | implemented | - | Explorer shows repos as architecture nodes. Clicking enters repo scope. |
| 6 | 2. Repo Creation | 2 | implemented | - | POST /api/v1/repos creates repos with workspace_id, name, description, default_branch, initialize options. |
| 7 | New Repo | 3 | implemented | - | Partial — bare repo created + initial commit made, but create_initial_commit writes an EMPTY tree (git2_ops.rs:424-429), NOT specs/manifest.yaml as spec §2 requires. Default gate chain (test/spec-review) not bootstrapped. `initialize` flag parsed but ignored (always commits). |
| 8 | Import External Repo | 3 | implemented | - | POST /api/v1/repos/mirror sets is_mirror, clone_mirror, periodic mirror_sync job registered (jobs.rs:267-278, fixed 60s). Partial — mirror_interval_secs not honored (hardcoded 60s); empty manifest not created when upstream lacks one. |
| 9 | 3. Repo Configuration | 2 | implemented | - | PUT /api/v1/repos/:id for settings. Gate and policy APIs exist. |
| 10 | Settings (Admin → Repo Scope → Settings) | 3 | implemented | - | Partial — name/description/default_branch updatable. Max concurrent agents and budget override not implemented (fields absent from Repository struct and UpdateRepoRequest). RepoSettings.svelte exists. |
| 11 | Gates (Admin → Repo Scope → Gates) | 3 | task-assigned | task-168 | API routes exist (GET/POST /repos/:id/gates, PUT/DELETE /repos/:id/gates/:gate_id). No dedicated gate configuration UI panel in web app — gates managed through API only. |
| 12 | Spec Policies (Admin → Repo Scope → Policies) | 3 | implemented | - | GET/PUT /api/v1/repos/:id/spec-policy. Spec policy toggles (require_spec_ref, require_approval, etc.) functional. |
| 13 | 4. Repo Archival and Deletion | 2 | implemented | - | Both archive and delete endpoints implemented. |
| 14 | Archive | 3 | implemented | - | Partial — sets Archived, cancels non-terminal tasks, closes Open/Approved MRs, stops repo agents. Missing spec §4 step 2 graceful shutdown: no 60s grace period, no inbox shutdown message, no kill_process — agents transitioned directly to Stopped. Step 8 not implemented: mirror_sync ignores status so sync is not paused on archive. |
| 15 | Delete | 3 | implemented | - | Partial — DELETE /api/v1/repos/:id requires archived, removes DB rows + git dir. Missing spec §4 preconditions: no cross-workspace spec_link check (409 Conflict on target_repo_id) and mirror credentials not deleted from credential store. |
| 16 | 5. Repo Discovery | 2 | implemented | - | Explorer workspace scope, API listing, Cmd+K search. |
| 17 | 6. Domain Changes | 2 | verified | - | Repository struct has all NEW spec fields: description (Option<String>), status (RepoStatus), updated_at (u64). repository.rs:44-46. |
| 18 | Repository Status | 3 | verified | - | RepoStatus enum with Active/Archived (default Active), Display/FromStr impls, archive()/unarchive()/is_archived() methods. repository.rs:4-85. |
| 19 | API Summary | 3 | task-assigned | task-168 | Most endpoints exist. Gap: git_http.rs receive-pack handler does not check RepoStatus::Archived — archived repos should reject git push with appropriate error. |
| 20 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
