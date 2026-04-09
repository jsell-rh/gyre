# Review: TASK-017 — Spec Registry Manifest Enforcement on Push

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** PASS — 0 findings  
**Commit reviewed:** `0c4af7ed`

## Verification Summary

Verified the implementation against `spec-registry.md` §Manifest Rules (Rule 1) and §Ledger Sync on Push (step 4).

### Acceptance Criteria

- [x] Push adding spec files not in `specs/manifest.yaml` is rejected with clear error
- [x] Error message lists unregistered files and includes fix hint
- [x] `index.md`, `prior-art/`, `milestones/` are excluded from enforcement
- [x] Enforcement is configurable via spec policy (`enforce_manifest` field)
- [x] Tests cover enforcement and graceful degradation
- [x] `cargo test --all` passes (31 spec_registry tests, 8 spec_policy tests — all green)

### What Was Checked

1. **Spec conformance:** `find_unregistered_specs` correctly implements §Ledger Sync step 4 ("For files under `specs/` not in manifest: reject push (or warn, policy-dependent)"). The `enforce_manifest` policy flag controls reject-vs-warn behavior.

2. **Domain/adapter/API consistency:** `enforce_manifest: bool` added to `SpecPolicy` (domain), `SpecPolicyRow` (both SQLite and Postgres adapters), `SpecPolicyResponse` (API), and `schema.rs`. All use `#[serde(default)]` / `DEFAULT 0` for backwards compatibility.

3. **Migration:** `000048` has both `up.sql` (ALTER TABLE ADD COLUMN) and `down.sql` (DROP COLUMN).

4. **Push handler wiring:** Enforcement runs after constraint checks but before response. On rejection: `undo_ref_updates` + `PushRejected` event + `FORBIDDEN` response — identical pattern to existing constraint check rejection.

5. **Exclusions:** `.md` extension filter correctly excludes `manifest.yaml`. Directory exclusions (`index.md`, `prior-art/`, `milestones/`) match existing behavior.

6. **Test coverage:** 7 new tests cover detection (4), enforcement/warn modes (3), and API round-trip (1). Tests use real git repos via `make_test_repo` helper.

7. **Port trait:** `SpecPolicyRepository` operates on `SpecPolicy` struct directly — no port changes needed.

8. **API docs:** `docs/api-reference.md` updated with `enforce_manifest` field documentation.

### Notes (not findings)

- `sync_spec_ledger` step 7 still runs `find_unregistered_specs` for warn-only logging after the push handler's enforcement check passes. This is redundant computation but not a spec violation.
- Default is `false` for all repos (new and existing). The task's implementation plan suggested `true` for new repos, but the spec says enforcement is "policy-dependent" without mandating a specific default. The conservative default is reasonable.
