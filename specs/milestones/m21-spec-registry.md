# M21: Spec Registry

**Status:** Complete  
**Goal:** Formal spec governance via a git manifest + forge ledger with approval tracking, drift detection, and dashboard.

## Deliverables

### M21.1 – Spec Registry Backend
- `specs/manifest.yaml` + ledger system for explicit spec registration
- Per-spec policies, approval tracking, auto-generated index
- REST API:
  - `GET /api/v1/specs` — list all specs with ledger state (path, title, owner, sha, approval_status, drift_status)
  - `GET /api/v1/specs/pending` — specs awaiting approval
  - `GET /api/v1/specs/drifted` — specs with open drift-review tasks
  - `GET /api/v1/specs/index` — auto-generated markdown index
  - `GET /api/v1/specs/{path}` — single spec ledger entry
  - `POST /api/v1/specs/{path}/approve` — approve a spec version (SHA-pinned); **Developer+ required**; blocked by unmet `implements`/`conflicts_with` link conditions
  - `POST /api/v1/specs/{path}/revoke` — revoke approval; caller must be original approver or Admin
  - `GET /api/v1/specs/{path}/history` — approval event timeline
- Approver type derived server-side: JWT bearer = `agent`, global token/API key = `human`

### M21.1-B – RBAC Enforcement
- ReadOnly callers receive 403 on `POST /api/v1/specs/{path}/approve`
- Only original approver or Admin may revoke

### M21.1-C – Spec Link Approval Gates
- Approval blocked (400) when an `implements` link exists and the parent spec is not yet approved
- Approval blocked (400) when a `conflicts_with` link exists and the conflicting spec is already approved

### M21.2 – Spec Dashboard (UI)
- **Specs sidebar** under Source Control
- Stats cards row: Total / Approved / Pending / Drifted (live from ledger)
- Filter pills: All / Pending / Approved / Drifted
- Spec table: path (mono), title, owner, status Badge, 7-char SHA, relative timestamp
- Slide-in detail panel (380px) with three tabs:
  - **Info**: full ledger metadata
  - **History**: approval event timeline with approver, SHA, timestamps, revocation reason
  - **Links**: linked MRs and tasks
- Approve button → SHA-confirmation modal → `POST /api/v1/specs/{path}/approve`
- Revoke button → reason-input modal → `POST /api/v1/specs/{path}/revoke`
