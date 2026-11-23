# Review: TASK-329 — §9 MetaSpecPolicy in full

**Reviewer:** Independent review (pi runtime)  
**Date:** 2026-11-24 (R1)  
**Candidate:** `724c3afcf2c195922bb824b42e73b24be5396c3d` (base `87377fe145ebddf27ef975fba7829fb10f4bb136`)  
**Verdict:** approved

---

## Scope inspected

Full diff base..candidate (9 files, +1467/−235): `merge_policy.rs`,
`reconciliation.rs`, `merge_processor.rs`, `api/workspaces.rs`,
`api/merge_deps.rs`, `prompt_assembly.rs`, `docs/api-reference.md`,
coverage row 13, task file.

## Findings

None blocking.

### Non-blocking observations (not task-contract defects)

1. **Pre-existing (not introduced):** `web/src/components/MergeQueueGraph.svelte:174`
   `driftLabel` reads `meta_spec_drift.produced_set_sha`/`current_set_sha` — a
   response shape retired before the assigned base (base already returned
   `{produced, current}` label arrays; the candidate returns `{transitions}`).
   The badge's presence check (`node?.meta_spec_drift` truthiness) still works;
   only the label text is degenerate, exactly as at the base. Outside this
   task's deliverables (§9 policy shape, tolerance reasoning, naming, workspace
   round-trip); worth a follow-up UI task to render `transitions`.
2. **Pre-existing (unchanged from base):** `merge_policy_or_default` degrades a
   policy-store outage to the warn-only default — a strict workspace's block
   would not hold during a KV outage. This is the base's documented degradation
   for both surfaces; the candidate neither introduces nor worsens it.
3. **Working-tree note for the verifier:** the review sandbox's working tree
   carries an unstaged controller revert of `specs/tasks/task-329.md`
   (progress `not-started`, `commits: []`). The candidate commit itself has the
   correct attribution (`e80bf106`, `1d00568b`, `53268114`) —
   `check-task-commit-attribution.sh` FAILS against the reverted working tree
   but PASSES at the candidate commit (verified in a clean worktree).

## Independent evidence (this review)

- **Source inspection:** all five deliverables are real production code, no
  stubs/fakery found:
  - (1) `MetaSpecPolicy {warn_on_drift=true, block_on_drift=false,
    drift_tolerance=1}` persisted per workspace via the port-backed KV store
    (`workspace_merge_policies`); legacy `require_current_meta_spec_set: true`
    documents map on read (`StoredPolicyDoc`) to `block_on_drift: true,
    drift_tolerance: 0`; new writes store the full shape only.
  - (2) `warn_on_drift: false` suppresses the `[WARN]` gate result except under
    `block_on_drift`, where the warning is always recorded alongside the block.
  - (3) `MetaSpecDriftTransition::versions_behind = current − recorded`;
    `exceeds_tolerance` blocks only when MORE than tolerance behind; missing
    entries and current-version-hash-disagreements are `recorded_version: None`
    (unverifiable, exceeds any tolerance — never fail-open).
  - (4) Warning/block naming `meta-spec-drift: {kind} {name} v{old} -> v{new}`
    with format/parse round-trip asserted exactly.
  - (5) `UpdateWorkspaceRequest.meta_spec_policy` (partial update, Admin-gated
    like the merge-policy endpoint, tolerance validated ≥ 0 → 400 via
    `parse_drift_tolerance` on a raw JSON number), persisted and returned by
    GET/list workspace; the WorkspaceSettings form's exact snake_case payload
    round-trips.
- **`cargo check -p gyre-server --lib --tests`** on the candidate: exit 0
  (8m38s cold; log: `/tmp/stage/review-evidence/cargo-check-gyre-server.log`).
- **Clippy:** `cargo clippy -p gyre-server --lib` — zero
  `doc_lazy_continuation` occurrences (the sole failure of verification attempt
  7dbb272a, fixed by the comment-only `e80bf106`; verified the only crates/
  delta since the verified candidate `91a12d61` is that one `///` line).
  Note: my local clippy 1.99 flags a pre-existing `explicit_counter_loop` in
  `gyre-common/src/view_query.rs:823` — untouched by this candidate, identical
  at the base, and not flagged by the verifier's toolchain (toolchain drift,
  recorded for awareness only).
- **rustfmt --check** on all six touched Rust files: clean.
- **All 19 mechanical invariant scripts** (arch, ABAC registry + exempt
  handlers, MCP write tools, migration versions/portability, dead message
  kinds, byte-slice, relative paths, fail-open refs, mem port contracts,
  fabricated/lossy scope checks, inert enforcement, forged scope fields,
  forwarded headers, in-memory state stores, unbounded HTTP, commit
  attribution): all pass at the candidate commit.
- **Test quality:** AC2's test (`meta_spec_drift_tolerance_one_warns_within_and_blocks_beyond`)
  exercises the exact specced scenario end-to-end — registry at v4, MRs whose
  prompt-set records (stored via production `store_prompt_set_record`) claim
  v3/v2 — through the real `process_next` merge processor, asserting one
  Merged-with-[WARN] and one blocked-and-requeued. Not mirrored logic.

## Probes the verifier must run (blocked here by the executor's 4 GiB
build-wrapper on gyre-server test binaries — infrastructure, not a defect)

```
cargo test -p gyre-server --lib -- merge_policy
cargo test -p gyre-server --lib -- drift
cargo test -p gyre-server --lib -- workspace_meta_spec_policy_roundtrip_via_update_workspace
cargo test -p gyre-server --lib -- merge_policy_full_shape_and_tolerance_validation
cargo test -p gyre-server --lib -- warn_on_drift_false_suppresses_warning_except_in_strict_mode
cargo clippy --all-targets -- -D warnings   # pinned verifier toolchain
```

Coverage row 13 (§9 Merge Gate Behavior) was updated accurately with source
evidence matching the inspected code. No unrelated rows touched.
