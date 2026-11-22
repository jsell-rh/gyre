# Review — task-302 (Spec bindings as registry metadata, re-approval cascade, UI panel, meta-spec-set retirement)

**Candidate:** `2d2cdeb6979d0a52f09f01664f7e96a003573846` (base `8546a781f772f05c823bbf4df6127bec812e2152`)
**Verdict:** NOT APPROVED — 2 blocking defects, 2 additional code findings
**Evidence:** `/tmp/stage/review-evidence/` (01–05)

## What was independently verified as real

- **Registry-metadata storage + invalidation**: `SpecLedgerEntry.bindings_sha` + `registry_sha()` (gyre-domain/spec_ledger.rs), SQLite/PG columns (migration 000071, next unused number), `put_spec_meta_spec_bindings` writes the ledger FIRST (bindings_sha update, approval reset to Pending, stale-pin dedup cleared) before replacing binding rows; identical re-PUT is a no-op; unregistered-spec and ledger-read failures reject the PUT. Kill test: `pin_update_on_approved_spec_resets_approval_and_registry_sha`.
- **Cascade gates**: spawn.rs refuses worker spawns (409, fail-closed on ledger errors) for tasks referencing a non-Approved spec (`spawn_blocks_worker_for_pending_spec_until_reapproval`); gates.rs `verify_spec_ref`/`verify_signed_spec_ref` deny on Pending ledger status even with an active signed approval row; `resolve_new_approval_status` compares history events against `registry_sha()`; `sync_spec_ledger` preserves `bindings_sha` across content edits.
- **Retirement**: routes, ABAC resolver entries, AppState fields, ports, adapters (sqlite/pg/mem), schema tables all removed; migration 000071 drops both KV tables; repo-wide greps for `meta_spec_sets`/`diff_meta_spec_set`/`compute_meta_spec_set_sha`/`MetaSpecPinnedEntry` in production code are empty; 404 kill tests for both retired endpoints; e2e-flow.sh and docs/api-reference.md updated.
- **Registry-model consumers**: `effective_required_meta_specs` (Global+Tenant+Workspace, required+Approved, fail-closed approval gate) feeds `workspace_meta_spec_context` (killing test proves registry sourcing), the §10 sweep, and `meta_spec_set_sha` stamping; merge drift compares `meta_specs_used` per-meta-spec (version+content_hash) against the current required set with warn-gate and strict-policy hard block, snapshot shape `{meta_spec_id, kind, content_hash, version}`.
- **UI panel**: reachable from the Specs view Edit tab; required rows pre-selected at current version and locked at three levels; optional selection with pinned-version select; api.js calls hit the canonical endpoints with correct encoding; substantive component tests; locale strings defined and the old "available after the spec-bindings API is extended" admission replaced.
- **Mechanical checks**: arch, ABAC route registry, migration versions, mem port contracts, dead message kinds, MCP write tools, migration SQL portability, in-memory state stores — all green at the candidate.
- **Coverage rows**: only the task's sections updated, with concrete source evidence.

## Blocking findings

1. **Push-time `context_binding.spec_sha` mismatch for bound specs** (api/specs.rs:649 vs :784; constraint_check.rs:1071-1087; git_http.rs:6067-6071). The approval history event now records `registry_sha` (64-hex sha256 when bound) while the signed attestation root minted by the same handler records the content SHA (40-hex); `resolve_task_binding_context` feeds the history value into `verify_context_binding`, so every attested push for a task referencing a bound spec is rejected ("push rejected: attestation context binding failed"). This breaks first-approval of bound specs under the authorization-provenance flow and the AC1/AC3 drift → pin-update → re-approval → re-implementation sequence at its final step. No test exercises a non-empty `bindings_sha` through this path. Fix: one consistent SHA on both sides.
2. **Fail-open `required_meta_spec_snapshots`** (reconciliation.rs:1435-1452): workspace-lookup and effective-set resolution errors degrade to an empty set with no logging — silently disabling the AC6 merge-drift check (strict policy included), the §10 sweep, and under-reporting `InputContent.meta_spec_set_sha`. Contradicts the fail-closed standard this same change applies to the bindings PUT, spawn gate, and `verify_spec_ref`.

## Additional findings (non-blocking)

3. Bindings panel skips the `specs/` prefix normalization used by every other spec call in DetailPanel (raw `entity.id` at :1467/:1578 vs :1144) — from the Explorer entry path the GET silently returns `[]` and the PUT 404s; the web tests match any `/meta-spec-bindings` URL so they cannot catch it.
4. The required-flag/approval reconciliation fan-out re-resolves meta-specs by globally non-unique name (`find_meta_spec_by_name`); on a cross-workspace name collision a wave can gate on the wrong record and skip a real rollout — thread the id from the update handler.

## Verifier must run (server suite exceeds this sandbox's build budget)

`cargo test -p gyre-server --lib` for: `api::meta_specs`, `api::spawn`, `api::gates`, `reconciliation`, `merge_processor` (meta_spec_drift), `llm_helpers`, `signal_chain`; `cd web && npm ci && npx vitest run src/__tests__/DetailPanel.test.js`; plus the Finding-1 probe (bound-spec approve → attestation → `resolve_task_binding_context` mismatch) described in the evidence files.
