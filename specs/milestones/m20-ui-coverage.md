# M20: UI Coverage

**Status:** Complete  
**Goal:** Address all 19 UI Accountability findings — ensure every major server feature has a corresponding dashboard surface.

## Deliverables

### M20.1 – Task Board Enhancements
- Task board cards are now clickable and navigate to a **Task Detail view**
- Task Detail has two tabs: **Info** (all fields: title, description, priority, status, assigned_to, parent) and **Artifacts** (linked MR, Ralph refs)

### M20.2 – Repo Detail New Tabs
- **Policy tab**: ABAC policy editor (`GET/PUT /api/v1/repos/{id}/abac-policy`) with claim/operator/value rule list; spec-policy toggles (`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`, `require_current_spec`)
- **Activity tab**: hot files panel (`GET /api/v1/repos/{id}/hot-files`) with per-line blame attribution (`GET /api/v1/repos/{id}/blame?path=`)
- **Gates tab**: quality gate management + push-gate toggles (`ConventionalCommit`, `TaskRef`, `NoEmDash`)
- **Commits tab**: agent attribution column + Ed25519 signature badge per commit
- **Branches tab**: speculative merge status badge per branch

### M20.3 – Merge Request Detail Dependencies
- MR detail sidebar: **Dependencies panel** with `depends_on` list, inline remove buttons, add-dep input, read-only "Required by" dependents
- `spec_ref` chip shows bound spec path + short SHA when present
- `atomic_group` badge in Details section

### M20.4 – Merge Queue DAG View
- **DAG toggle** in merge queue header switches to dependency graph (`GET /api/v1/merge-queue/graph`)
- Blocked-by dependency chips (orange left border) + green ready indicator per entry

### M20.5 – Audit View
- Two-tab view: **Live Stream** (SSE from `GET /api/v1/audit/stream`) + **History** (filtered query)
- Aggregate stats card showing event counts by type

### M20.6 – Spec Approvals View
- Full CRUD: approval table, Approve modal (path + SHA input), Revoke modal (reason input)
- Uses `GET/POST /api/v1/specs/approvals`, `POST /api/v1/specs/approve`, `POST /api/v1/specs/revoke`

### M20.7 – Auth Token UI Improvements
- Auth status dot in topbar (green = authenticated, red = error)
- Token modal fetches `GET /api/v1/auth/token-info` on open; displays token kind, agent ID, task ID, scope, expiry
