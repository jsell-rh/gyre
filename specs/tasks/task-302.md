---
title: "Spec bindings as registry metadata with re-approval cascade, UI panel, and meta-spec-set retirement"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / bindings-registry-cascade / 4a3f940495a8"
depends_on: [task-301]
progress: ready-for-review
commits: ["443e75a84d6f56f47a31fe374a7ee0e52c3d7480", "cd6787dd8b8db7eabfb426449a0e0a26d850af77", "58c89154f14ea2bff7349db6093e38b31e395e2b", "0e90f42ca0bc43d8df82e90c27c296facc574ff1", "e68a426e306a876263f63298af11b33996bdf072", "4d3ed96b7a6960b5faedefa5a9163091075a1e6f", "0160a25e5ce31515fb553963f06843d357dfcc91"]
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
  (gyre-domain/spec_ledger.rs) records the canonical SHA over the binding
  set; `registry_sha()` derives the spec's registry identity over content
  SHA + bindings SHA (equals `current_sha` when unbound, so existing
  approvals stay valid). `put_spec_meta_spec_bindings` (api/meta_specs.rs)
  writes the binding set AND the ledger: a changed set updates
  `bindings_sha`, resets `approval_status` to Pending via the existing
  content-edit invalidation machinery, and clears the `stale_pin:{spec}:`
  dedup keys, with the existing validation semantics preserved
  (pin-version-exists, cross-workscope leak guard, role gating,
  fail-closed unregistered-spec rejection). The spawn gate (api/spawn.rs)
  and merge gates (api/gates.rs) fail closed on non-Approved specs and
  ledger lookup errors; the existing SpecApproved → orchestrator →
  delegation chain re-fires on re-approval.
- `e68a426e` — UI bindings panel (Specs view Edit flow: required
  meta-specs pre-selected and locked, optional selection with pinned
  versions, saved via PUT /api/v1/specs/:path/meta-spec-bindings), web API
  client calls, and the meta-spec-set retirement: workspace/tenant set
  PUT/GET routes removed (404), all production writers to the
  meta_spec_sets/tenant_meta_spec_sets KV stores removed,
  `resolve_effective_meta_spec_set` / `run_conformance_sweep` /
  `llm_helpers::workspace_meta_spec_context` migrated to the DB-backed
  registry model (required flags + spec-level bindings), merge-time drift
  enforcement compares the attestation `meta_specs_used` array against
  current required versions with `{meta_spec_id, kind, content_hash,
  version}` snapshot tuples.
- `0e90f42c`, `58c89154`, `cd6787dd` — review-repair rounds (rustfmt,
  fail-closed ledger reads in the bindings PUT / spawn gate / merge gates,
  workspace-scoped bindable-meta-spec listing for the panel).
- `443e75a8` — verification-repair round for the 26 failures from
  `cargo test --all` at the prior candidate (run a5483570): registry-test
  ledger-path convention, restored §8 `force` flag contract on
  `PUT /api/v1/meta-specs/:id`, §6/§7 mechanics tests migrated to
  required-spec fixtures, `set_update_emits_meta_spec_set_updated_event`
  rewritten to the registry-model trigger, production fix in
  `parse_meta_spec_drift_warning` (right-split of `{kind}:{id}@v{version}`
  labels whose kinds carry a colon), effective-set kind-rank fixture, and
  signal_chain `impl_task` re-approves the seeded spec before spawning.
- `0160a25e` — verification repair for the single remaining failure at
  candidate `056a011c` (run `be00b4ae`,
  `signal_chain::tests::scheduler_decomposes_delegation_into_ordered_
  subtasks`, panic "implementation sub-task assigned to a spawned worker
  agent"). Root cause: the test invoked the `on_spec_approved` hook
  directly, but production `approve_spec` (api/specs.rs) writes the
  Approved status to the spec ledger BEFORE invoking that hook — the test
  skipped the handler's ledger write, so the seeded Pending spec stayed
  Pending and the task-302 worker-spawn approval gate (api/spawn.rs)
  correctly blocked the Phase-4 spawn. Fix: the ledger re-approval
  `impl_task` already performed is extracted into a documented
  `approve_spec_in_ledger` helper (production parity with the approval
  handler) and called in the decomposes test before `on_spec_approved`.
  No assertion was weakened and the Pending spawn-block is unchanged
  (still pinned by
  `api/spawn.rs::spawn_blocks_worker_for_pending_spec_until_reapproval`).

### Test evidence and deferral

- Local (this sandbox): `cargo check -p gyre-server --lib --tests` clean
  at this round's head; rustfmt clean on the touched file; web/dist left
  at its committed state (executor restores generated assets).
- The independent verifier must re-run `cargo test --all` (the original
  failing command). This round's change is confined to the
  `signal_chain.rs` test module; the fixed test exercises the same
  assertions as before against the now-approved ledger state. No routes,
  migrations, port traits, or production code paths were touched in this
  round.
