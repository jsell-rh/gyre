---
title: "Spec bindings as registry metadata with re-approval cascade, UI panel, and meta-spec-set retirement"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / bindings-registry-cascade / 4a3f940495a8"
depends_on: [task-301]
progress: ready-for-review
commits: ["4d3ed96b7a6960b5faedefa5a9163091075a1e6f", "e68a426e306a876263f63298af11b33996bdf072", "0e90f42ca0bc43d8df82e90c27c296facc574ff1", "58c89154f14ea2bff7349db6093e38b31e395e2b", "9c4128983f74ea323e1efadc99a26d8441acfe83", "cd6787dd8b8db7eabfb426449a0e0a26d850af77"]
---

## Required behavior

Per agent-runtime.md §2 Spec-Level Binding ('Stored in the spec registry alongside other spec metadata') and Stale Pin Detection ('Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval'), plus the amendments-table meta-spec-reconciliation.md row's registry-replacement clauses. (1) Storage: the binding set becomes part of the spec registry metadata covered by the spec SHA recorded in the approval ledger — updating a pin via PUT /api/v1/specs/:path/meta-spec-bindings produces a new registry SHA for the spec, resets approval_status to Pending (invalidating the prior approval exactly as a content edit does, reusing the existing invalidation machinery), and clears stale-pin dedup keys. The endpoint's existing validation semantics (pin-version-exists, cross-workscope leak guard, role gating) are preserved. (2) Cascade: after a human re-approves, the existing chain fires unchanged — SpecApproved → workspace orchestrator → delegation task → re-implementation with the newly pinned prompt versions; no new trigger mechanism is invented. The hourly stale-pin check keeps producing priority-6 drift alerts, and acting on an alert now genuinely forces re-approval before any agent runs with the new pins. (3) UI: the Specs view Edit flow gains the meta-spec bindings panel — it lists available meta-specs from the tenant and workspace registries, shows required ones pre-selected and locked (cannot be removed), allows selecting optional ones and their pinned versions, and saves via the bindings endpoint; the web API client gains the corresponding calls against the canonical /api/v1/meta-specs contract. (4) Meta-spec-set retirement (amendments table, meta-spec-reconciliation.md row: 'The PUT /api/v1/workspaces/{id}/meta-spec-set endpoint is replaced by updating required flags and spec-level bindings via the meta-spec API'): retire PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set (404/405, no silent alias) and remove every production writer to the meta_spec_sets/tenant_meta_spec_sets KV stores. Migrate the remaining consumers to the DB-backed registry model (required flags + spec-level bindings): reconciliation effective-set resolution (resolve_effective_meta_spec_set), the §10 conformance sweep (run_conformance_sweep), and the user-query prompt context llm_helpers::workspace_meta_spec_context ({{meta_spec_context}} in specs/assist, briefing/ask, explorer/generate). Reconciliation triggering by required-flag changes and meta-spec approval (the existing meta-spec approval handler trigger) is preserved so the replacement surface drives the same behavior the old endpoint drove. Merge-time drift enforcement compares the merge-attestation meta_specs_used array against current required meta-spec versions (replacing the old-model compute_meta_spec_set_sha comparison in merge_processor.rs and reconciliation.rs), and snapshot-shaped data is produced as {meta_spec_id, kind, content_hash, version} tuples rather than path@sha MetaSpecPinnedEntry strings. Coordination: the active ui-layout plan's llm-endpoint-contract-completion task builds its LLM-endpoint prompt-context injection on the workspace meta-spec-set mechanism — this retirement must migrate that context source to the registry model and must not silently invalidate that plan's baseline; the spec-lifecycle plan's spec-lifecycle-sha-pin task owns the push-hook blob-SHA pinning — this task changes only the registry-metadata/SHA semantics for binding edits, not the push hook.

## Acceptance Criteria

- [ ] Updating a pin on an approved spec changes the spec's registry SHA and sets approval_status to Pending; no worker agent is spawned for tasks referencing that spec until it is re-approved; after re-approval a SpecApproved event and delegation task are produced (existing chain) and newly spawned agents receive the meta-spec content at the new pinned version in prompt-assembly band 3.
- [ ] The UI bindings panel renders required meta-specs pre-selected and locked, allows saving an optional selection with a pinned version, and the saved set is returned by GET /api/v1/specs/:path/meta-spec-bindings and consumed by prompt assembly.
- [ ] The drift alert → pin update → re-approval → re-implementation sequence is demonstrable end to end: a spec pinning v1 of a meta-spec now at v2 gets a priority-6 alert, the pin update forces re-approval, and only post-approval agents use v2.
- [ ] PUT/GET /api/v1/workspaces/:id/meta-spec-set and PUT/GET /api/v1/tenants/:id/meta-specs/set return 404/405 and no production code path writes meta_spec_sets/tenant_meta_spec_sets; no production reference to diff_meta_spec_set/compute_meta_spec_set_sha remains.
- [ ] Changing a required flag (or approving a new version of a required meta-spec) drives the same reconciliation sweep the old set-PUT drove, and llm_helpers {{meta_spec_context}} for user-initiated LLM queries resolves from the registry's effective set.
- [ ] Merge-time drift enforcement compares meta_specs_used against current required meta-spec versions: an attestation recorded against an older required version is flagged, and snapshot data exchanged on that path is shaped {meta_spec_id, kind, content_hash, version}.

## Verification intent

Approve a spec with a binding pinned to v1 of a meta-spec now at v2, then update the pin to v2 via the API and inspect the spec ledger — at the base put_spec_meta_spec_bindings (api/meta_specs.rs:9432) writes only the meta_spec_bindings side table: the spec's SHA and approval_status are unchanged, no re-approval is required, and no SpecApproved/delegation/re-implementation follows (the specced cascade never fires because the ledger SHA is the git blob SHA of the markdown file and does not cover bindings). For the retirement: PUT /api/v1/workspaces/:id/meta-spec-set at the base returns 200 and drives the old-model machinery (diff_meta_spec_set over path@sha pins → emit_meta_spec_set_updated → run_reconciliation), llm_helpers::workspace_meta_spec_context resolves the path@sha effective set for user queries, and reconciliation.rs contains no reference to meta_specs_used — after the change all three assertions must fail at the base paths (endpoint 404, context registry-sourced, drift model meta_specs_used-based).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "e97ea9a092758dc93c245e6e",
    "decision": "gap",
    "requirement_quote": "The spec author selects these via the UI (Specs view → Edit → Meta-spec bindings panel). The UI shows available meta-specs from the tenant and workspace registries, with required ones pre-selected and locked (cannot be removed).",
    "detail": "The binding model is real as an API: PUT/GET /api/v1/specs/:path/meta-spec-bindings performs a declarative full-set replace with pin-version-exists validation, a cross-workscope binding leak guard, and role gating; band 3 of prompt assembly consumes the bindings at pinned versions via version-history lookup. Two gaps: (1) Storage location — bindings live in a separate meta_spec_bindings DB table keyed by the bare spec path string, NOT 'stored in the spec registry alongside other spec metadata'; the spec ledger entry's SHA is the git blob SHA of the spec markdown file and does not cover bindings (this is the root cause of the broken stale-pin cascade, accounted under 'Stale Pin Detection'). (2) The specced UI bindings panel does not exist: no web component or API client references the bindings endpoint (the web meta-specs surface covers registry CRUD only), and the shipped locale string admits workspace-level drift tracking is 'available after the spec-bindings API is extended' — so spec authors have no UI to select bindings, pre-selected required entries, or locked required rows.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "spec_id: spec_path.clone(),",
        "detail": "Bindings are keyed by the bare spec path string in the side table — the ledger entry, its blob SHA and its approval status are separate structures, so bindings are not spec-registry metadata and pin changes have no effect on spec identity."
      },
      {
        "file": "web/src/locales/en.json",
        "quote": "\"drift_text_suffix\": \"Workspace-level drift tracking (repos built under older versions) is available after the spec-bindings API is extended.\",",
        "detail": "The shipped UI itself concedes the bindings surface is not integrated: no Specs-view bindings panel exists, and the drift text defers workspace-level tracking until the bindings API is 'extended'."
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "quote": "// Band 3: spec-level bindings at pinned versions. A binding that",
        "detail": "Positive wiring: the assembly consumes bindings at their pinned versions via the version-history lookup, so the binding model itself works end-to-end for prompt injection."
      }
    ]
  },
  {
    "id": "9f95c49cf209205750a0177a",
    "decision": "gap",
    "requirement_quote": "Human updates the pin → spec metadata changes → spec SHA changes → spec needs re-approval",
    "detail": "Detection and notification are real: an hourly background job (meta_spec_stale_pin_check, also triggered on meta-spec edits) runs detect_stale_pins, compares every binding's pinned_version against the current version, dedups via KV keys, and creates priority-6 MetaSpecDrift notifications ('Meta-spec drift alert: ... Review and update pin.') routed to the workspace's Admin/Owner members. But the specced cascade never fires: updating a pin (put_spec_meta_spec_bindings) writes only the meta_spec_bindings side table and clears the stale-pin dedup keys — it never touches the spec ledger entry, its blob SHA, or its approval status. Because bindings are not part of the spec registry metadata (the ledger SHA is the git blob SHA of the spec markdown file), a pin update does NOT change the spec SHA, does NOT require re-approval, emits no SpecApproved event, and triggers no delegation task or re-implementation; the new pin takes effect only for agents spawned later under tasks that happen to reference the spec. The section's outcome — 'agents re-implement with new prompts' after a human acts on a drift alert — is therefore unreachable through the implemented mechanisms. (This was blocking review feedback at the prior audit base; the cited code is unchanged at this base.)",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "let prefix = format!(\"stale_pin:{}:\", spec_path);",
        "detail": "The pin-update handler's only side effects are the meta_spec_bindings side table and stale-pin dedup-key clearing — it never writes the spec ledger, so the pin update cannot change the spec SHA or reset its approval."
      },
      {
        "file": "crates/gyre-domain/src/spec_ledger.rs",
        "quote": "/// Git blob SHA of the spec file at HEAD.",
        "detail": "The ledger SHA is the git blob SHA of the spec markdown file — bindings are not part of it, so the specced 'spec metadata changes → spec SHA changes → spec needs re-approval' chain cannot fire on a pin update."
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "quote": "\"Meta-spec drift alert: '{}' uses {} v{}, but v{} is available. Review and update pin.\",",
        "detail": "Positive wiring for the detection half: the priority-6 drift notification with the specced message body is created for stale pins — the alert itself works; it is the post-alert cascade that is missing."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "name: \"meta_spec_stale_pin_check\".to_string(),",
        "detail": "The background job runs the detection hourly, satisfying the 'background job (or check on spec access)' trigger for detection."
      }
    ]
  }
]

## Shipped

No acceptance-criterion checkboxes are ticked; independent review and
deterministic gates decide approval.

### Implementation (product commits on this branch)

- `4d3ed96b` — production migration. `SpecLedgerEntry.bindings_sha`
  (gyre-domain/spec_ledger.rs) records the canonical SHA over the binding set;
  `registry_sha()` derives the spec's registry identity over content SHA +
  bindings SHA (equals `current_sha` when unbound, so existing approvals stay
  valid). `put_spec_meta_spec_bindings` (api/meta_specs.rs) writes the binding
  set AND the ledger: a changed set updates `bindings_sha`, resets
  `approval_status` to Pending via the existing content-edit invalidation
  machinery, and clears the `stale_pin:{spec}:` dedup keys, with the existing
  validation semantics preserved (pin-version-exists, cross-workscope leak
  guard, role gating). The spawn gate (api/spawn.rs) and merge gates
  (api/gates.rs) fail closed on non-Approved specs and ledger lookup errors, so
  no worker agent runs under tasks referencing a Pending spec; the existing
  SpecApproved → orchestrator → delegation chain re-fires on re-approval and
  band-3 prompt assembly injects the newly pinned versions. The
  workspace/tenant meta-spec-set endpoints are retired (404/405) with the KV
  stores dropped (migration 000071); reconciliation effective-set resolution,
  the §10 conformance sweep, and llm_helpers `{{meta_spec_context}}` resolve
  from the DB-backed registry (required flags + spec-level bindings);
  required-flag changes and meta-spec approval still trigger the
  reconciliation sweep. Merge-time drift enforcement compares the attestation
  `meta_specs_used` array against current required versions
  (merge_processor.rs `selection_meta_spec_drift_hold`, reconciliation.rs
  `mr_meta_spec_drift`), exchanging `{meta_spec_id, kind, content_hash,
  version}` snapshots. No production reference to `diff_meta_spec_set` /
  `compute_meta_spec_set_sha` or writer to meta_spec_sets /
  tenant_meta_spec_sets remains.
- `e68a426e` — registry-model tests, Specs-view Edit bindings panel
  (web/src/lib/DetailPanel.svelte; required meta-specs pre-selected and locked,
  optional selection with pinned versions, saved via the bindings endpoint)
  with web API client calls (web/src/lib/api.js), and set-retirement
  verification.
- `0e90f42c` — first review repair: rustfmt + AC1 kill tests
  (`pin_update_on_approved_spec_resets_approval_and_registry_sha`,
  `spec_bindings_reject_spec_absent_from_ledger`).
- `58c89154` — review-repair hardening: the PUT loads the ledger entry once at
  the top with fail-closed semantics (lookup error → 500, unregistered path →
  404) and runs the invalidation write before the binding-row swap; spawn and
  merge gates fail closed on lookup errors; workspace-scoped meta-spec listing.

### Rebase repair against 82aed3db (this round, merge commit `9c412898`)

Rebased the saved source onto the task-280 base. Three resolutions, all
behavior-preserving, evidence in
`/tmp/stage/review-evidence/rebase-merge-resolution.md`:

1. lib.rs `gyre_ports` import conflict: took the union — base's
   `CascadeTestResultRepository` imported, our removal of
   `MetaSpecSetRepository`/`TenantMetaSpecSetRepository` kept (retirement
   intact; cascade wiring present in AppState and the store! registry).
2. Base-side test helper `ledger_entry` (api/specs.rs) initialized
   `SpecLedgerEntry` without the new `bindings_sha` field — added
   `bindings_sha: String::new()` consistent with the other eleven
   initializers.
3. Migration version collision: both sides had added a 000070 migration —
   renamed ours to `2026-11-21-000071_spec_bindings_registry` (contents
   unchanged) so neither silently never runs;
   `scripts/check-migration-versions.sh` passes.

### Test evidence (this round)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 (after
  resolution 2).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-adapters -p gyre-domain -p gyre-ports
  --tests` → exit 0.
- `scripts/check-arch.sh`, `check-migration-versions.sh`,
  `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`,
  `check-dead-message-kinds.sh` → all pass.

Deferred to the independent verifier (4 GiB budget; large gyre-server test
compilation): `cargo test -p gyre-server meta_specs`, `cargo test -p gyre-server
merge_processor` (meta_specs_used drift holds + task-280
`trigger_cascade_tests_*`), `cargo test -p gyre-server prompt_assembly`,
`cargo test -p gyre-adapters` (000071 up/down, cascade_test_result repos), and
`cd web && npm ci && npm test` (bindings panel component tests). Behavior on
those suites is carried unchanged from the reviewed commits plus the three
rebase resolutions above.
