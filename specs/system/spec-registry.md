# Spec Registry

> **Status: Design** — This spec describes planned functionality. `specs/manifest.yaml` and the forge ledger are not yet implemented. `specs/index.md` is still hand-maintained.

## Problem

Specs are currently identified by convention: files under `specs/` are specs because of their path. The approval ledger tracks approvals by path + SHA. The lifecycle hooks fire on path prefixes. `specs/index.md` is hand-maintained.

This is fragile:
- No explicit declaration of "this file is a spec"
- No per-spec policy (some specs need security review, others don't)
- No machine-readable registry of what specs exist, their status, and their relationships
- `specs/index.md` can drift from reality
- A non-spec file in `specs/` triggers lifecycle hooks it shouldn't
- No way to query "which specs are approved?" or "which specs have open drift-review tasks?"

## Solution: Git Manifest + Forge Ledger

Two complementary systems:

1. **`specs/manifest.yaml`** (in git) - declares what specs exist and their policies. This is the source of truth for **what is a spec and what rules apply to it**. It's versioned, diffable, reviewable.

2. **Forge spec ledger** (in the database) - tracks runtime state: current SHA, approval status, linked MRs, linked tasks, drift status. This is the source of truth for **the current state of each spec**.

The manifest defines the contract. The ledger tracks adherence.

## The Manifest: `specs/manifest.yaml`

```yaml
version: 1

# Default policies applied to all specs unless overridden
defaults:
  requires_approval: true
  auto_create_tasks: true
  auto_invalidate_on_change: true

specs:
  # System specs (what Gyre does)
  - path: system/design-principles.md
    title: Design Principles
    owner: user:jsell
    requires_approval: true
    approvers:
      - user:jsell
    gates:
      - accountability

  - path: system/identity-security.md
    title: Identity & Security
    owner: user:jsell
    requires_approval: true
    approvers:
      - user:jsell
    gates:
      - security
      - accountability

  - path: system/source-control.md
    title: Source Control
    owner: user:jsell

  - path: system/agent-runtime.md
    title: Agent Runtime & Compute
    owner: user:jsell

  - path: system/supply-chain.md
    title: Supply Chain Security
    owner: user:jsell
    gates:
      - security

  - path: system/agent-gates.md
    title: Agent Gates & Spec Binding
    owner: user:jsell
    gates:
      - security
      - accountability

  # Development specs (how Gyre gets built)
  - path: development/architecture.md
    title: Architecture & Standards
    owner: user:jsell

  - path: development/ralph-loops.md
    title: Ralph Loops
    owner: user:jsell

  - path: development/database-migrations.md
    title: Database & Migrations
    owner: user:jsell

  # Reference-only specs (no implementation tracking)
  - path: system/trusted-foundry-integration.md
    title: Trusted Foundry (Future)
    owner: user:jsell
    auto_create_tasks: false  # Reference only, no implementation expected

  # Personas (not implementation contracts)
  - path: personas/ceo.md
    title: CEO Agent Persona
    owner: user:jsell
    requires_approval: false
    auto_create_tasks: false

  - path: personas/accountability.md
    title: Accountability Agent Persona
    owner: user:jsell
    requires_approval: false
    auto_create_tasks: false

  - path: personas/security.md
    title: Security Agent Persona
    owner: user:jsell
    requires_approval: false
    auto_create_tasks: false
```

### Manifest Schema

| Field | Type | Default | Description |
|---|---|---|---|
| `path` | string | required | Relative path from `specs/` |
| `title` | string | required | Human-readable name |
| `owner` | string | required | User or team responsible |
| `requires_approval` | bool | true | Must be approved before code can reference it |
| `approvers` | string[] | [owner] | Who can approve this spec. Empty = anyone with approval permission |
| `gates` | string[] | [] | Gate agent personas that review MRs referencing this spec |
| `auto_create_tasks` | bool | true | Create tasks on spec change (from spec-lifecycle) |
| `auto_invalidate_on_change` | bool | true | Invalidate approval when content changes |
| `superseded_by` | string | null | Path to replacement spec (marks this as deprecated) |

### Manifest Rules

1. **Every spec file must be in the manifest.** Once implemented, the forge will reject pushes that add files under `specs/` without a corresponding manifest entry.
2. **The manifest itself is a spec.** Changes to `specs/manifest.yaml` trigger the spec lifecycle hooks and require approval.
3. **Removing a manifest entry without removing the file is an error.** The forge rejects this as inconsistent state.
4. **The manifest is the single source of truth for policy.** The `spec_lifecycle` config block in the spec-lifecycle spec is superseded by per-spec manifest entries.

## The Forge Ledger

The forge maintains runtime state for each registered spec:

```sql
CREATE TABLE spec_registry (
    path            TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    owner           TEXT NOT NULL,
    current_sha     TEXT NOT NULL,
    approval_status TEXT NOT NULL DEFAULT 'pending',
    approved_by     TEXT,
    approved_at     INTEGER,
    approval_sig    TEXT,
    linked_tasks    TEXT NOT NULL DEFAULT '[]',
    linked_mrs      TEXT NOT NULL DEFAULT '[]',
    drift_status    TEXT NOT NULL DEFAULT 'unknown',
    last_checked    INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);
```

### Ledger State Machine

```
                    push with new content
    APPROVED ---------------------------------> PENDING
        |                                          |
        |  (no changes)                            |  approval submitted
        |                                          |
        +--- APPROVED <----------------------------+
                |
                |  spec deleted from manifest
                |
                v
            DEPRECATED
```

### Ledger Sync on Push

When a push lands, the forge:

1. Reads `specs/manifest.yaml` from the new HEAD
2. For each entry in the manifest:
   - Computes the git blob SHA of the spec file
   - If SHA changed from ledger's `current_sha`: update SHA, set `approval_status = pending`, invalidate old approval (if `auto_invalidate_on_change`)
   - If new entry (not in ledger): create ledger record
3. For entries in ledger but not in manifest: mark as `DEPRECATED`
4. For files under `specs/` not in manifest: reject push (or warn, policy-dependent)

### API Surface

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/specs` | GET | List all registered specs with ledger state |
| `GET /api/v1/specs/{path}` | GET | Single spec: policy, approval status, linked MRs/tasks, drift |
| `POST /api/v1/specs/{path}/approve` | POST | Approve a spec version (with optional Sigstore signature) |
| `POST /api/v1/specs/{path}/revoke` | POST | Revoke an approval (with reason) |
| `GET /api/v1/specs/{path}/history` | GET | Approval history for a spec |
| `GET /api/v1/specs/pending` | GET | All specs awaiting approval |
| `GET /api/v1/specs/drifted` | GET | All specs with open drift-review tasks |

## Auto-Generated Index

Once implemented, `specs/index.md` will be auto-generated by the forge from the manifest + ledger (replacing hand-maintenance):

```
GET /api/v1/specs/index
```

Returns a markdown document with:
- All specs grouped by directory
- Current approval status per spec
- Links to spec files
- Open task count per spec

The web UI renders this as the spec dashboard. Agents read it as their entry point.

## Integration with Existing Specs

### Spec Lifecycle (spec-lifecycle.md)

The spec lifecycle hooks now use the manifest instead of path prefix matching:
- **Which files trigger hooks:** files listed in the manifest with `auto_create_tasks: true`
- **Per-spec priority:** manifest can override default task priority
- **Per-spec gates:** manifest declares which gate agents review MRs referencing this spec
- The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section

### Agent Gates (agent-gates.md)

The `gates:` field in the manifest feeds the gate chain:
- MR references `system/identity-security.md` -> manifest says gates = [security, accountability]
- Forge spawns security gate agent and accountability gate agent for this MR
- Different specs can require different gate agents

The spec approval ledger in agent-gates.md is now unified with the forge ledger described here. One table, one source of truth.

### Spec-to-Code Binding (agent-gates.md)

Pre-accept validation gains a new check:
- The MR's `spec_ref` path must exist in the manifest
- The referenced SHA must have `approval_status = approved` in the ledger
- If the spec has `requires_approval: false`, any SHA is accepted

### Supply Chain (supply-chain.md)

The AIBOM includes manifest state at release time:
- Which specs were approved and by whom
- Which specs had open drift-review tasks (flagged as risk)
- Manifest version (git SHA of `specs/manifest.yaml`)

### Accountability Agent (personas/accountability.md)

The Accountability agent's patrol gains manifest-aware checks:
- Spec files that exist but aren't in the manifest (orphans)
- Manifest entries whose files don't exist (stale entries)
- Specs with `requires_approval: true` that have been `pending` for more than one Ralph loop cycle
- Specs with `auto_create_tasks: true` that were modified but have no corresponding task
