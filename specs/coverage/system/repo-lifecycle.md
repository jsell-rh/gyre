# Coverage: Repo Lifecycle

**Spec:** [`system/repo-lifecycle.md`](../../system/repo-lifecycle.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 14/18 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Where Repo Management Lives | 2 | implemented | - | Admin view with workspace-scoped repos. API routes exist for all repo operations. |
| 3 | Admin → Workspace Scope → Repos Tab | 3 | task-assigned | task-168 | API complete. No dedicated "Repos" tab in Admin web UI — repos managed through workspace settings but not as a distinct admin tab per spec. |
| 4 | Admin → Repo Scope → Settings | 3 | implemented | - | PUT /api/v1/repos/:id updates name, description, default_branch (repos.rs:216-245). RepoSettings.svelte exists. |
| 5 | Explorer → Workspace Scope | 3 | implemented | - | Explorer shows repos as architecture nodes. Clicking enters repo scope. |
| 6 | 2. Repo Creation | 2 | implemented | - | POST /api/v1/repos creates repos with workspace_id, name, description, default_branch, initialize options. |
| 7 | New Repo | 3 | implemented | - | Bare git repo created. Initial commit with specs/manifest.yaml if initialized. |
| 8 | Import External Repo | 3 | implemented | - | POST /api/v1/repos/mirror. Sets is_mirror, creates sync job, periodic git fetch. |
| 9 | 3. Repo Configuration | 2 | implemented | - | PUT /api/v1/repos/:id for settings. Gate and policy APIs exist. |
| 10 | Settings (Admin → Repo Scope → Settings) | 3 | implemented | - | Name, description, default_branch updatable. RepoSettings.svelte component exists. |
| 11 | Gates (Admin → Repo Scope → Gates) | 3 | task-assigned | task-168 | API routes exist (GET/POST /repos/:id/gates, PUT/DELETE /repos/:id/gates/:gate_id). No dedicated gate configuration UI panel in web app — gates managed through API only. |
| 12 | Spec Policies (Admin → Repo Scope → Policies) | 3 | implemented | - | GET/PUT /api/v1/repos/:id/spec-policy. Spec policy toggles (require_spec_ref, require_approval, etc.) functional. |
| 13 | 4. Repo Archival and Deletion | 2 | implemented | - | Both archive and delete endpoints implemented. |
| 14 | Archive | 3 | implemented | - | POST /api/v1/repos/:id/archive. Stops active agents (Stopped status). Cancels non-terminal tasks with timestamp and reason. Closes all Open/Approved MRs with "Repo archived" reason. Tests confirm behavior. |
| 15 | Delete | 3 | implemented | - | DELETE /api/v1/repos/:id. Requires archived status. Removes repo data from DB and git directory from filesystem. |
| 16 | 5. Repo Discovery | 2 | implemented | - | Explorer workspace scope, API listing, Cmd+K search. |
| 17 | 6. Domain Changes | 2 | implemented | - | Repository struct has all spec fields: description, status (Active/Archived via RepoStatus), updated_at. |
| 18 | Repository Status | 3 | implemented | - | RepoStatus enum with Active and Archived variants. archive() and unarchive() methods on Repository. |
| 19 | API Summary | 3 | task-assigned | task-168 | Most endpoints exist. Gap: git_http.rs receive-pack handler does not check RepoStatus::Archived — archived repos should reject git push with appropriate error. |
| 20 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
