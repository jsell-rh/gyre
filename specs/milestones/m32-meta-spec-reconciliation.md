# M32: Meta-Spec Reconciliation — Kind Field, Workspace Binding, Blast Radius

**Status:** Done
**Milestone:** M32

---

## Summary

M32 makes Gyre's spec registry meta-spec-aware: specs can be tagged with a `kind` field to identify them as personas, principles, standards, or process docs. Workspaces maintain a pinned `MetaSpecSet` binding their active meta-specs. A blast-radius endpoint shows which workspaces and repos would be affected by a change to any given meta-spec.

Related spec: [specs/system/meta-spec-reconciliation.md](../system/meta-spec-reconciliation.md)

---

## M32.1 — Spec Kind Field (PR #347)

A new optional `kind` field is added to `SpecEntry` and `SpecLedgerEntry`:

| Kind value | Meaning |
|---|---|
| `meta:persona` | Persona definition spec |
| `meta:principle` | Engineering principle spec |
| `meta:standard` | Coding or architectural standard spec |
| `meta:process` | Process or workflow spec |
| _(omitted)_ | Regular implementation spec |

**`GET /api/v1/specs`** now accepts a `?kind=<kind>` query parameter to filter the spec list by kind. Each entry in the response includes the `kind` field when set.

**`specs/manifest.yaml`** entries may include a `kind:` field. The spec sync (`sync_spec_ledger`) propagates it into the ledger on each push.

---

## M32.2 — Workspace Meta-Spec Set (PR #347)

Each workspace can maintain a pinned collection of active meta-specs, grouped by role:

```json
{
  "workspace_id": "<uuid>",
  "personas": {
    "backend": {"path": "specs/personas/backend.md", "sha": "<40-char-hex>"},
    "reviewer": {"path": "specs/personas/reviewer.md", "sha": "<40-char-hex>"}
  },
  "principles": [
    {"path": "specs/principles/hexagonal.md", "sha": "<40-char-hex>"}
  ],
  "standards": [
    {"path": "specs/standards/rust-style.md", "sha": "<40-char-hex>"}
  ],
  "process": [
    {"path": "specs/process/ralph-loop.md", "sha": "<40-char-hex>"}
  ]
}
```

**Endpoints:**

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/api/v1/workspaces/{id}/meta-spec-set` | any | Get current meta-spec set; returns empty set if none configured; 404 if workspace not found |
| `PUT` | `/api/v1/workspaces/{id}/meta-spec-set` | Admin | Replace meta-spec set; 404 if workspace not found |

---

## M32.3 — Meta-Spec Blast Radius (PR #347)

**`GET /api/v1/meta-specs/{path}/blast-radius`** — given a URL-encoded spec path, returns the workspaces and repos that would be affected by a change to that spec:

```json
{
  "spec_path": "specs/personas/backend.md",
  "affected_workspaces": [{"id": "<uuid>"}],
  "affected_repos": [
    {"id": "<uuid>", "workspace_id": "<uuid>", "reason": "workspace_binding"}
  ]
}
```

Matching logic: a workspace is affected if the spec path appears in any of its `personas`, `principles`, `standards`, or `process` entries. Its repos are collected via the `workspace_repos` KV store key.

---

## M32.4 — Spawn Provenance: `meta_spec_set_sha` (PR #347)

`SpawnAgentResponse` gains a `meta_spec_set_sha: Option<String>` field — the SHA256 digest of the workspace's bound `MetaSpecSet` at spawn time. When the workspace has no meta-spec set configured, the field is `null`.

This enables commit provenance: agents can attest which version of the persona/principle/standard collection governed their behavior during a task.

---

## M32.5 — MetaSpecs UI (PR #347)

`MetaSpecs.svelte` — card grid view of meta-specs:

- Fetches `GET /api/v1/specs?kind=meta:persona`, `?kind=meta:principle`, `?kind=meta:standard`, `?kind=meta:process` — one request per kind
- Renders specs as cards with: path, title, `kind` badge, approval status badge
- **Blast-radius button** on each card opens a modal — calls `GET /api/v1/meta-specs/{path}/blast-radius` and displays affected workspaces and repos in a list

**Route:** `/meta-specs` (sidebar: "Meta-Specs" under Overview section).

---

## Acceptance Criteria

- [x] `SpecEntry` and `SpecLedgerEntry` carry optional `kind` field
- [x] `GET /api/v1/specs?kind=meta:persona` returns only persona specs
- [x] `GET /api/v1/workspaces/{id}/meta-spec-set` returns MetaSpecSet (empty if unconfigured)
- [x] `PUT /api/v1/workspaces/{id}/meta-spec-set` requires Admin role; 404 for unknown workspace
- [x] `GET /api/v1/meta-specs/{path}/blast-radius` returns affected workspaces and repos
- [x] `SpawnAgentResponse.meta_spec_set_sha` is non-null when workspace has a meta-spec set
- [x] MetaSpecs view renders kind-filtered cards with blast-radius modal
- [x] Route `/meta-specs` reachable via sidebar and direct URL
- [x] Unit tests: `blast_radius_empty`, `meta_spec_set_not_found_for_unknown_workspace`

---

## Implementation Notes

- `crates/gyre-server/src/api/meta_specs.rs` — handler module with 3 endpoints
- `crates/gyre-server/src/api/specs.rs` — `?kind=` filter added to `ListSpecsParams`
- `web/src/components/MetaSpecs.svelte` — UI component
- `web/src/lib/api.js` — `getMetaSpecs(kind)`, `getMetaSpecBlastRadius(path)` methods added
- Meta-spec sets stored in `AppState.meta_spec_sets: Mutex<HashMap<String, MetaSpecSet>>`
- Workspace repos for blast radius: KV store key `"workspace_repos"` → JSON array of repo IDs
