---
title: "Spec bindings as registry metadata with re-approval cascade, UI panel, and meta-spec-set retirement"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / bindings-registry-cascade / 4a3f940495a8"
depends_on: [task-301]
progress: ready-for-review
commits: ["58c89154f14ea2bff7349db6093e38b31e395e2b", "0e90f42ca0bc43d8df82e90c27c296facc574ff1", "e68a426e306a876263f63298af11b33996bdf072", "4d3ed96b7a6960b5faedefa5a9163091075a1e6f"]
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

Product commits on this branch: `4d3ed96b` (production migration), `e68a426e`
(registry-model tests, UI panel, set-retirement verification), `0e90f42c`
(first review repair: rustfmt + AC1 kill tests), and `58c89154` (this repair
round, findings `6e2bc9d0` / checkpoint `61104af`). No acceptance-criterion
checkboxes are ticked; independent review and deterministic gates decide
approval.

### Registry storage and re-approval cascade (AC1, AC3)

- `SpecLedgerEntry.bindings_sha` (gyre-domain/spec_ledger.rs) records the
  canonical SHA over the binding set; `registry_sha()` derives the spec's
  registry identity over content SHA + bindings SHA (equals `current_sha`
  when unbound, so existing approvals stay valid).
- `put_spec_meta_spec_bindings` (api/meta_specs.rs) writes the binding set
  AND the ledger: a changed set updates `bindings_sha`, resets
  `approval_status` to Pending (same invalidation machinery as a content
  edit), and clears the `stale_pin:{spec}:` dedup keys. Validation
  semantics preserved: pin-version-exists, cross-workscope leak guard,
  Developer-membership/global-Admin role gating.
- Review-repair hardening (`58c89154`): the ledger entry is loaded once at
  the top of the PUT with fail-closed semantics — a lookup error returns
  500 and an unregistered spec path returns 404 (previously both silently
  skipped the registry-SHA/approval write while the binding rows
  persisted, the exact fail-open F1 finding), and the invalidation write
  runs before the binding-row swap so a mid-PUT failure can never leave a
  new binding set active under a stale approval.
- The spawn gate (api/spawn.rs) refuses worker agents for tasks whose
  spec is not Approved, and now fails closed on a ledger lookup error
  (same fail-open read class); the merge gates
  `verify_spec_ref`/`verify_signed_spec_ref` (api/gates.rs) likewise fail
  closed on lookup errors. The existing SpecApproved → orchestrator →
  delegation chain re-fires unchanged on re-approval; the hourly
  `meta_spec_stale_pin_check` job keeps producing priority-6 drift
  alerts.
- Kill tests: `pin_update_on_approved_spec_resets_approval_and_registry_sha`
  (pin update → Pending + new registry SHA; identical re-PUT no-op;
  version bump and unbind re-invalidate; unbound identity = content SHA),
  `spec_bindings_reject_spec_absent_from_ledger` (unregistered path → 404,
  nothing persisted), `spawn_blocks_worker_for_pending_spec_until_reapproval`
  (spawn for a Pending spec → 409 with no agent/worktree; re-approval
  unblocks), `verify_spec_ref_denies_pending_ledger_despite_active_approval_row`
  (active signed approval row + Pending ledger → denied on both gates).

### UI bindings panel (AC2)

- `web/src/lib/DetailPanel.svelte` Edit tab: meta-spec bindings panel with
  registry listing, required meta-specs pre-selected with a locked
  checkbox and version, optional ones selectable with a pinned-version
  select; save PUTs the declarative full set.
- Review-repair (`58c89154`): the "available meta-specs" listing is no
  longer effectively Admin-only. New `GET /api/v1/workspaces/:id/meta-specs`
  (api/meta_specs.rs `list_workspace_meta_specs`, ABAC resolver mapping
  registered) serves the bindable set — Global + the workspace's tenant +
  the workspace itself — to Owner/Admin/Developer members with the same
  containment the bindings PUT enforces; agent tokens and non-members are
  rejected (403), unknown workspaces 404. The panel falls back to it when
  the Admin-only flat listing is forbidden, then per-binding lookups for
  bound entries the listing does not cover. `web/src/lib/api.js` gains
  `getWorkspaceMetaSpecs`; the retired workspace/tenant set client calls
  are gone.
- Component tests (`web/src/__tests__/DetailPanel.test.js`, "Spec edit tab
  — meta-spec bindings panel"): required pre-selected+locked, optional
  selection with pinned version, full-set PUT payload, unbind keeps the
  locked required entry, and the new fallback test (403 flat listing →
  workspace-scoped listing, required still pre-selected and locked). All
  45 DetailPanel tests pass locally (vitest run, this sandbox).

### Meta-spec-set retirement (AC4)

- PUT/GET `/api/v1/workspaces/:id/meta-spec-set` and
  `/api/v1/tenants/:id/meta-specs/set` are removed from the router —
  every method 404s, no alias (tests `retired_meta_spec_set_endpoints_return_404`).
- The `meta_spec_sets`/`tenant_meta_spec_sets` ports, adapters, and
  AppState wiring are deleted; no production writer or reader remains.
- `resolve_effective_meta_spec_set` (reconciliation.rs) resolves the
  effective set from the registry (fail-closed approval gate);
  `run_conformance_sweep` compares the registry effective-set SHA
  (`meta_spec_snapshot_sha` over `{meta_spec_id, kind, content_hash,
  version}` tuples) against provenance; `llm_helpers::
  workspace_meta_spec_context` renders `{{meta_spec_context}}` from the
  same registry resolution (consumed by specs/assist, briefing/ask,
  explorer/generate).
- Review-repair (`58c89154`): docs/api-reference.md no longer documents
  the retired endpoints (they now 404) — the registry-replacement model
  and the new workspace-scoped listing are documented instead;
  scripts/e2e-flow.sh probes the live workspace listing rather than the
  retired route.

### Reconciliation triggers preserved (AC5)

- Required-flag flips and meta-spec approval drive the same waves the
  retired set-PUT drove. Tests: `required_flag_flip_via_api_triggers_reconciliation`,
  `tenant_spec_approval_triggers_per_workspace_reconciliation`,
  `required_flip_propagates_to_tenant_workspaces`,
  `two_step_edit_then_approve_triggers_reconciliation`,
  `meta_spec_context_resolves_from_registry_effective_set` (llm_helpers.rs),
  and the new `workspace_meta_spec_listing_scopes_to_membership`
  (member sees Global + own-tenant + own-workspace, other workspaces
  invisible, non-member/agent-token 403, unknown workspace 404).

### Merge-time drift model (AC6)

- `mr_meta_spec_drift` compares the merge-attestation `meta_specs_used`
  array against current required meta-spec versions
  (`resolve_effective_meta_spec_set`); snapshot data on the path is
  shaped `{meta_spec_id, kind, content_hash, version}` tuples. Tests:
  `drift_check_*` (reconciliation.rs) and merge_processor strict-policy
  regressions.

### Deferred verification probes

The sandbox build wrapper blocks gyre-server test-binary compilation
("code generation plus the model exceeds 4 GiB"); `cargo check -p
gyre-server` passes at `58c89154` (exit 0). The independent verifier must
run the focused server tests:

    cargo test -p gyre-server --lib -- \
      spec_bindings_reject_spec_absent_from_ledger \
      workspace_meta_spec_listing_scopes_to_membership \
      spec_bindings_put_get_and_validation \
      pin_update_on_approved_spec_resets_approval_and_registry_sha \
      spec_bindings_require_membership_in_owning_workspace \
      stale_pin_detection_creates_notification \
      spawn_blocks_worker_for_pending_spec_until_reapproval \
      verify_spec_ref_denies_pending_ledger_despite_active_approval_row \
      retired_meta_spec_set_endpoints_return_404 \
      required_flag_flip_via_api_triggers_reconciliation \
      meta_spec_context_resolves_from_registry_effective_set

Evidence for this round recorded under /tmp/stage/review-evidence/
(cargo check log, vitest run, gate scripts, rustfmt changed-lines check).
