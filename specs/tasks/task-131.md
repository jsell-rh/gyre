---
title: "Implement ABAC policy CLI commands"
spec_ref: "abac-policy-engine.md §CLI"
depends_on:
  - task-128
  - task-129
progress: complete
coverage_sections:
  - "abac-policy-engine.md §CLI"
commits: ["05549afd91dfda702b701cd828359c6c93cae8be"]
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

## Shipped

All six `abac-policy-engine.md` §CLI commands ship as the `gyre policy` command group in `gyre-cli` (commit `05549afd`), calling the real policy API shipped by task-129 — no server changes were needed.

### Behavior

- **`gyre policy list`** — `GET /api/v1/policies`, table of id, name, scope, scope_id, effect, priority, enabled, built-in. `--scope {tenant,workspace,repo}` maps to the server's scope filter (clap `ValueEnum`, so unknown values are rejected at parse time); `--workspace-id` maps to the `scope_id` query param and is gated twice: clap `requires = "scope"` plus a runtime check that the scope is exactly `workspace` (tested: `--scope tenant --workspace-id ws-1` fails before any request).
- **`gyre policy create --file policy.yaml`** — parses a serde_yaml definition mirroring the `POST /api/v1/policies` body (required: name, scope, priority, effect, actions, resource_types; optional: description, scope_id, conditions with string/list/number/null values, enabled), validates scope/effect client-side before any network I/O, prints the created id/scope/effect. Reserved `trust:`/`builtin:` names and Admin-only enforcement remain server-side and surface as HTTP errors.
- **`gyre policy evaluate`** — `POST /api/v1/policies/evaluate` with the `{type}:{id}` flags mapped onto the documented request body (`subject.type`/`subject.id`, `resource.type`/`resource.id`); prints decision, matched policy (or "(none — default deny)"), evaluated-policy count, and evaluation time.
- **`gyre policy decisions`** — `GET /api/v1/policies/decisions` with the subject-id filter (the `agent:` prefix is CLI syntax; the audit log keys on the id) and `--limit` (default 100, matching the server); `--decision {allow,deny}` and `--since <duration>` (s/m/h/d/w) are applied to the fetched newest-first page by the `filter_decisions` helper; prints timestamp, subject, action, resource, decision, matched policy.
- **`gyre policy effective`** — `GET /api/v1/policies/effective?subject={id}&resource_type={type}`; prints the action/decision/matched-policy matrix answered from the subject's real stored attributes (server side).
- **Error handling** — every API failure propagates as an error naming the HTTP status and the server's message (auth 403, validation 400, not-found 404); malformed `--subject`/`--resource`/`--since`/YAML input is rejected before any request is sent. Help text exists for the group and every subcommand/flag (verified against the built binary).

`serde_yaml` moved from gyre-cli dev-dependencies to regular dependencies (`workspace = true`); no other crates touched.

### Test evidence

23 focused tests in `gyre-cli` (all passing): arg-parsing tests including the clap `requires` rejection and unknown-scope-value rejection; `parse_subject_ref`/`parse_since_duration` unit tests (valid and malformed forms, overflow); `load_policy_yaml` tests (full definition with string/list/number condition values, optional-field defaults, missing-required-field, invalid scope/effect, missing file); a `filter_decisions` outcome/age matrix test; and mock-server command-flow tests asserting the exact request lines and bodies (`GET /api/v1/policies?scope=workspace&scope_id=ws-1`, `POST /api/v1/policies` with the YAML-derived JSON body, the evaluate request JSON shape, `GET /api/v1/policies/decisions?limit=250&subject_id=worker-42`, `GET /api/v1/policies/effective?subject=jsell&resource_type=spec`) plus error propagation for 403/400 and pre-request rejection of malformed input.

Probes run in this sandbox (per the 4 GiB budget rule, no gyre-server build here): `cargo test -p gyre-cli` — 178 passed, 0 failed; `cargo fmt -p gyre-cli --check` clean; `cargo clippy -p gyre-cli --all-targets` — no gyre-cli warnings; `scripts/check-byte-slice-truncation.sh`, `check-arch.sh`, `check-inert-enforcement.sh`, `check-relative-path-defaults.sh`, `check-lossy-secret-conversion.sh` all pass.

The independent verifier must run `cargo test --all` (the final acceptance criterion) — in particular `cargo test -p gyre-cli` (the 23 policy tests) and the existing server-side `api::policies` tests that own the endpoint contracts the CLI calls.
