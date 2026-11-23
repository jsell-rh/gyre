---
title: "Blast radius computed from provenance: version-discriminated affected workspaces plus per-repo spec/commit counts from real attestation queries"
spec_ref: "Spec gap spec-556a0b49e9f2803bf0e2ff3d / blast-radius-provenance / b288723d2f73"
depends_on: [task-302]
progress: ready-for-review
commits: ["ded8ebc8ec3ce1578fbf2160ff2b26f12eb472a5"]
---

## Required behavior

Per §4, the blast radius is a database query over provenance records + bindings — computed, not guessed. Today GET /api/v1/meta-specs/:path/blast-radius (meta_specs.rs:650-745) does a path-membership check on effective set pins only: no provenance store is queried, there are no per-repo spec/commit counts, and no version discrimination (a workspace pinned to any version counts as affected; the spec's 'bound to v2, skipped' case is not skipped). COORDINATION: active task-302 replaces the path@sha set-binding basis with the registry model (required flags + spec-level bindings, versioned tuples); deliver the §4 behavior on the model present after task-302. Deliverables: (1) Version discrimination — the blast-radius request carries (or the registry supplies) the change context: the superseded version/SHA of the changed meta-spec; a workspace is affected when its effective binding requires/pins the changed meta-spec at the SUPERSEDED version; workspaces bound to other versions are reported as skipped/not-affected with their pinned version, matching the spec's example (internal-tools on v2 is skipped for a v3→v4 change). (2) Per-repo counts from provenance — for each affected workspace, per repo: the count of distinct implementation specs with provenance produced under the superseded version (distinct spec identity in provenance records — chain/merge attestations, agent-commit provenance — whose recorded governing meta-spec versions include the superseded version of the changed meta-spec) and the count of commits under it; the response carries per-workspace/per-repo spec and commit counts plus totals (repos, specs, commits), all computed from real storage queries. (3) The existing workspace_binding vs tenant_inheritance reason labels are preserved (§8 amendment). (4) The endpoint stays read-only and ABAC-mapped; counts never fabricated — a repo with no provenance under the superseded version contributes zero/omitted even though it belongs to an affected workspace.

## Acceptance Criteria

- [ ] Workspace A requiring the changed meta-spec at v3 (provenance: 2 distinct specs, N commits across 2 repos) and workspace B bound at v2: GET blast-radius for the v3→v4 change reports A affected with per-repo spec/commit counts matching the seeded provenance and correct totals; B appears as skipped/not-affected with pinned v2 and is excluded from totals.
- [ ] A repo in A with no provenance under v3 contributes no counts and is not listed among affected repos with work — counts derive from provenance records, not workspace repo membership.
- [ ] Inherited bindings are still labeled tenant_inheritance and own bindings workspace_binding, with counts computed identically for both.

## Verification intent

Seed the A/B scenario and call GET /api/v1/meta-specs/{path}/blast-radius for the v3→v4 change; assert B is absent from affected workspaces and A's repos carry spec/commit counts equal to the seeded provenance. At the base both assertions fail: the handler's entire affected-workspace test is a path-membership check (effective.personas.values().any(|e| e.path == spec_path) || … over principles/standards/process) so B — which pins the path at v2 — is reported affected exactly like A, and the handler's only data sources are workspaces.list, the set stores, and repos.list_by_workspace (no chain_attestations, attestation_store, or agent-commit query anywhere in the body), so no spec or commit count can be produced at all.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/meta-spec-reconciliation.md`. Source: `50575cf65e6491279049052901a65c68a4d2993d`; plan: `c2ceefa3b1c58d079ded6a7ee7cd9311539758965f2e1d0edd37b7ffdd4c9071`.

[
  {
    "id": "3662e9dd04c597d5db7d61f0",
    "decision": "gap",
    "requirement_quote": "This is a database query over provenance records + workspace meta-spec set bindings. The blast radius is computed, not guessed.",
    "detail": "Requires blast radius computed from provenance records plus set bindings, with per-repo spec/commit counts under the superseded version and version-diff reasoning (the example skips a workspace 'bound to v2, not affected'). What exists: GET /api/v1/meta-specs/:path/blast-radius does a real binding scan — it lists all workspaces, resolves each one's effective set (tenant defaults merged with workspace overrides), checks whether any personas/principles/standards/process entry references the path, labels the reason workspace_binding vs tenant_inheritance (the §8 amendment), and lists the repos of affected workspaces. What is missing: the provenance half — the handler (inspected in full, meta_specs.rs 650-745) never queries attestation/provenance stores, so there are no per-repo spec counts, no commit counts under the superseded version, and no version discrimination at all: a workspace pinned to any version of the path counts as affected (the 'bound to v2, skipped' case is not skipped), and the response carries only affected repo/workspace ids with a reason string — no totals over provenance. The binding half is real; the 'database query over provenance records' half is absent.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "let references_spec = effective.personas.values().any(|e| e.path == spec_path)",
        "detail": "The entire affected-workspace test is a path-membership check on the effective set's pinned entries (|| principles/standards/process any()). No comparison of the pinned sha against the changed version, no provenance lookup — a workspace pinned to an old version is reported affected exactly like one pinned to the new version."
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "let workspaces = state.workspaces.list().await.unwrap_or_default();",
        "detail": "Handler data sources, inspected across the full body (meta_specs.rs 650-745): workspaces.list, meta_spec_sets.get, tenant_meta_spec_sets.get, repos.list_by_workspace. No chain_attestations / attestation_store / spec ledger provenance query appears anywhere in the handler, so the spec/commit counts and 'commits under v3' figures of the §4 output shape cannot be produced by this endpoint."
      }
    ]
  }
]

## Shipped

No acceptance-criterion checkboxes are ticked; independent review and
deterministic gates decide approval.

### §4 blast radius: version discrimination + provenance counts

Delivered on the task-302 registry model (this task's `depends_on`), in
`crates/gyre-server/src/api/meta_specs.rs`
(`get_meta_spec_blast_radius` + `blast_radius_repo_provenance`):

- **Change context (deliverable 1).** `GET /api/v1/meta-specs/{path}/blast-radius`
  accepts `?from_version=&to_version=`; `from_version` is the SUPERSEDED
  version (defaulted to the registry's current version), and a
  `to_version <= from_version` is a 400 (a meta-spec change moves to a
  LATER version). A workspace is affected only when its effective binding
  requires/pins the changed meta-spec at the superseded version:
  spec-level `meta_spec_bindings` pins (joined to the ledger entry's
  workspace scope) at exactly that version, or required-flag visibility
  (Global/Tenant/Workspace) at the registry's current version. Workspaces
  bound at other versions are reported in `skipped_workspaces` with their
  pinned version and excluded from every count and total — the spec's
  "internal-tools on v2 is skipped for a v3→v4 change" case.
- **Per-repo counts from provenance (deliverable 2).**
  `blast_radius_repo_provenance` queries three real storage surfaces per
  repo: agent-commit provenance (`agent_commits.find_by_repo`) joined with
  the author agent's spawn-time prompt-set record (`meta_specs_used`),
  chain attestations (`chain_attestations.find_by_repo` — commit SHA from
  the attestation, spec identity from the signed input's `spec_path`),
  and merge attestation bundles (`attestation_store.find_by_mr_id` — the
  bundle's own `meta_specs_used` array, merge commit SHA, and MR spec
  reference). It counts distinct spec identities and distinct commit SHAs
  whose recorded governing meta-spec versions include the superseded
  version of the changed meta-spec, deduplicated across surfaces. The
  response carries per-workspace `repos: [{id, spec_count,
  commit_count}]`, a flat `affected_repos` binding scan, and
  `totals {repos, specs, commits}` — all computed from storage, never
  fabricated: a repo with no provenance under the superseded version gets
  zero counts in `affected_repos` and is omitted from the workspace's
  repos-with-work list (it does not increment `totals.repos`).
- **§8 labels preserved (deliverable 3).** `reason` stays
  `workspace_binding` vs `tenant_inheritance` (own spec-level pins and
  Workspace-scoped required flags are own bindings; broader-scope
  required flags are inherited), with a granular `binding` field
  (`required_global` / `required_tenant` / `required_workspace` /
  `spec_binding`). Counts are computed identically for both classes.
- **Read-only + ABAC (deliverable 4).** The endpoint is an unchanged
  GET route, still mapped in the ABAC resolver
  (`abac_middleware.rs:805`); no new route, no exemption entries.

### Verification evidence

- `cargo check -p gyre-server --all-targets` (build wrapper): exit 0, no
  errors, no warnings — includes type-checking of the new tests.
- Mechanical gates pass: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-inert-enforcement,
  check-byte-slice-truncation, check-fabricated-scope-defaults,
  check-scope-literal-defaults, check-lossy-secret-conversion,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-in-memory-state-stores, check-unbounded-external-http,
  check-dead-message-kinds, check-mem-port-contracts.
- New tests in `crates/gyre-server/src/reconciliation.rs` (in the tests
  module):
  `blast_radius_version_discriminates_and_counts_provenance` (the A/B
  acceptance scenario: B pinned v2 skipped with pinned version reported
  and excluded from totals; A's two repos carry spec/commit counts equal
  to the seeded provenance — the v2-recorded commit in repo A excluded;
  the no-provenance repo of A present in the flat scan with zero counts
  but omitted from the repos-with-work list; exact totals
  {repos:2, specs:2, commits:3}),
  `blast_radius_defaults_superseded_version_to_registry_current`,
  `blast_radius_labels_and_counts_from_chain_and_merge_attestations`
  (AC3: inherited vs own labels with identical counting over the chain
  and merge-attestation surfaces),
  `blast_radius_rejects_backwards_version_range` (400).
- **Deferred to the independent verifier (sandbox constraint):** the
  cargo build wrapper refuses gyre-server code generation ("exceeds
  4 GiB ... Server builds/tests belong to the independent verifier"), so
  the new tests are type-checked here but must be EXECUTED by the
  verifier. Required probe, recorded in
  `/tmp/stage/review-evidence/task-327-verifier-tests.txt`:

      cargo test -p gyre-server --lib blast_radius

  (covers all four new tests plus the pre-existing
  `blast_radius_covers_required_scopes_and_spec_bindings` and
  `blast_radius_empty`; `cargo test -p gyre-server --lib reconciliation::`
  as the wider regression scope).

### Docs and coverage

- `docs/api-reference.md`: the blast-radius row now documents the query
  parameters and the full response shape (skipped workspaces, per-repo
  counts, totals, binding kinds).
- `specs/coverage/system/meta-spec-reconciliation.md` row 8 (§4) updated
  from Partial to the task-327 implementation with source evidence.
