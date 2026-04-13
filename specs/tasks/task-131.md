---
title: "Implement ABAC policy CLI commands"
spec_ref: "abac-policy-engine.md §CLI"
depends_on:
  - task-128
  - task-129
progress: not-started
coverage_sections:
  - "abac-policy-engine.md §CLI"
commits: []
---

## Spec Excerpt

From `abac-policy-engine.md` §CLI:

```bash
gyre policy list                                    # List all policies
gyre policy list --scope workspace --workspace-id ws-1  # Workspace policies
gyre policy create --file policy.yaml               # Create from YAML
gyre policy evaluate --subject user:jsell --action approve --resource spec:identity  # Dry-run
gyre policy decisions --subject agent:worker-42 --decision deny --since 1d  # Audit
gyre policy effective --subject user:jsell --resource-type spec  # "What can jsell do with specs?"
```

## Implementation Plan

1. **Add `policy` subcommand group to `gyre-cli`:**
   - `gyre policy list` — calls `GET /api/v1/policies`, displays table (id, name, scope, effect, enabled)
   - `--scope {Tenant|Workspace|Repo}` filter
   - `--workspace-id` filter (requires --scope workspace)

2. **`gyre policy create --file policy.yaml`:**
   - Parse YAML policy definition (same fields as Policy entity)
   - POST to `POST /api/v1/policies`
   - Display created policy id on success

3. **`gyre policy evaluate`:**
   - `--subject {type}:{id}` — e.g., `user:jsell` or `agent:worker-42`
   - `--action {action}` — e.g., `approve`, `push`, `read`
   - `--resource {type}:{id}` — e.g., `spec:identity`
   - POST to `POST /api/v1/policies/evaluate`
   - Display decision, matched policy, evaluation time

4. **`gyre policy decisions`:**
   - `--subject {type}:{id}` — filter by subject
   - `--decision {allow|deny}` — filter by outcome
   - `--since {duration}` — e.g., `1d`, `1h`, `30m`
   - GET from `GET /api/v1/policies/decisions`
   - Display table of decisions (timestamp, subject, action, resource, decision, policy)

5. **`gyre policy effective`:**
   - `--subject {type}:{id}` — who are we checking
   - `--resource-type {type}` — what resource type
   - GET from `GET /api/v1/policies/effective`
   - Display matrix of allowed actions per resource type

## Acceptance Criteria

- [ ] `gyre policy list` displays all policies with scope/effect columns
- [ ] `--scope` and `--workspace-id` filters work correctly
- [ ] `gyre policy create --file policy.yaml` creates policy from YAML
- [ ] `gyre policy evaluate` performs dry-run and shows decision
- [ ] `gyre policy decisions` queries audit log with filters
- [ ] `gyre policy effective` shows effective permissions matrix
- [ ] Error handling for all API failures (auth, not found, validation)
- [ ] Help text for all commands and flags
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §CLI for the full spec. CLI implementation: `crates/gyre-cli/src/`. Existing CLI commands: look at other subcommand modules (e.g., `tasks.rs`, `mr.rs`) for the pattern. API client: check how CLI currently calls the server API. API endpoints: `GET /api/v1/policies`, `POST /api/v1/policies`, `POST /api/v1/policies/evaluate`, `GET /api/v1/policies/decisions`, `GET /api/v1/policies/effective` — all registered in `gyre-server/src/api/mod.rs`. YAML parsing: use `serde_yaml` or similar.
