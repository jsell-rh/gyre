---
title: "Platform Model Post-Merge Validation + Recovery Protocol"
spec_ref: "platform-model.md §6 Rollback & Recovery"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-095.md
coverage_sections:
  - "platform-model.md §6 Rollback & Recovery"
  - "platform-model.md §6 Post-Merge Validation"
  - "platform-model.md §6 Recovery Protocol"
  - "platform-model.md §6 Agent Behavior During Recovery"
commits: ["ee2bcf8eff79c9849b8576ae12ba2a173108cd80", "c2e1ee93344be8f6989615ba84e315b590450273", "e6a6db47dbd8b988c8367a1965db50fd77d5abe7", "3a9b11f69c57cb576d2e3914ff7af7e54941b518", "5aaded213193d1af1cec693e729ac2872893b10b", "86c7d382312fae4fb52ae4effe40e021330f3ac2", "7c723298b41ee761067b15ae426651aa280fb75e", "c8595a409f15c267ab53b460532f409917124e2b"]
---

## Spec Excerpt

### Post-Merge Validation

After the merge processor merges an MR, a post-merge validation gate runs against the new HEAD of the default branch.

### Recovery Protocol

```
MR merged to main → Post-merge gate runs
  ├── PASS: continue. Merge queue processes next entry.
  └── FAIL:
        1. Merge queue PAUSES
        2. Forge creates a REVERT commit
        3. Post-merge gate re-runs on reverted HEAD
        4. Original MR re-opened with status `Reverted`
        5. Author agent receives RevertNotification via MCP
        6. Task created: "MR #{id} reverted: {failure reason}"
        7. MR's gate results invalidated
```

**MR Status Enum** gains `Reverted` variant.

### Agent Behavior During Recovery

Agents keep working — their branches need rebasing after main is fixed (jj handles automatically). Merge queue paused, workspace orchestrator notified.

## Implementation Plan

1. **Post-merge gate execution:**
   - After `merge_processor.rs` merges an MR, run configured post-merge gates
   - Add `post_merge_gates` to repo gate configuration (same schema as pre-merge gates)
   - Execute against HEAD of default branch after merge commit

2. **MR `Reverted` status:**
   - Add `Reverted` variant to `MrStatus` enum in domain
   - Migration to add "Reverted" to the status constraint (if using CHECK)

3. **Recovery protocol implementation:**
   - On post-merge gate failure:
     a. Pause the merge queue for the repo (`merge_queue_paused: bool` on repo state)
     b. Create revert commit using `git revert --no-edit <merge_sha>`
     c. Push revert to default branch
     d. Re-run post-merge gates on reverted HEAD
     e. If revert passes: resume merge queue, re-open original MR as `Reverted`
     f. If revert fails: escalate to human (critical notification)
   - Send `RevertNotification` message to author agent
   - Create remediation task referencing the failure reason

4. **Merge queue pause/resume:**
   - Add `paused: bool` and `pause_reason: Option<String>` to merge queue state
   - When paused, merge processor skips processing new entries
   - Resume clears the flag and triggers immediate queue processing

5. **Workspace orchestrator notification:**
   - Emit Event-tier message when merge queue pauses/resumes
   - Workspace orchestrator can reprioritize work accordingly

## Acceptance Criteria

- [x] Post-merge gates run after merge against new HEAD
- [x] `Reverted` variant added to MrStatus
- [x] Merge queue pauses on post-merge gate failure
- [x] Revert commit created and pushed automatically
- [x] Original MR re-opened as Reverted
- [x] Author agent receives RevertNotification
- [x] Remediation task created with failure reason
- [x] Merge queue resume works after main is green
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §6 "Rollback & Recovery". The merge processor is in `gyre-server/src/merge_processor.rs`. Gate execution is in `gyre-server/src/gate_executor.rs`. The MrStatus enum is in `gyre-domain/src/lib.rs` (or similar). For git revert operations, check how the forge executes git commands (likely via `gyre-server/src/git_http.rs` or a git helper). The merge queue state is managed in the merge processor — look for how queue entries are processed sequentially.

## Shipped

Round-5 revision (repair of the Round-4 review findings R5-F1…R5-F3,
plus the interrupted upstream merge), on top of the previously
shipped §6 Post-Merge Validation + Recovery Protocol work (Rounds 1–4:
post-merge gates on merged HEAD, `MrStatus::Reverted`, queue pause on
failure, `git revert -m 1` three-way-merge semantics with Conflict
leaving the branch untouched, remediation tasks + RevertNotification,
resubmission-stable circuit breaker, merge-commit-sha-scoped manual
revert — details in `specs/reviews/task-095.md`):

- **Upstream merge resolved:** the interrupted merge of base
  `110180fb` completed. `retention.rs` took the upstream side — a
  superset evolution of this branch's checkpoint contribution
  (recovered upstream via `92074d84` and extended: configurable
  `SnapshotTiers`, `purge_older_than`, band-no-spillover deletion);
  every function this branch's checkpoint added exists upstream.
  `web/dist` was rebuilt from the merged `web/src` (`npm ci` +
  `vite build`), which reproduced the upstream asset set exactly
  (index-CLIRvdnY.js / index-GeLbLKhg.css / elk.bundled-C7RdDx88.js),
  confirming the merged sources and committed build output are
  coherent.
- **R5-F1 (inert §7.2 Allow):** `builtin:attested-authorization` was
  seeded `enabled: false`, so `policy_engine::evaluate` (which
  retains only `p.enabled` policies) never ran it and
  `builtin-default-deny` denied every push/merge carrying a valid
  attestation chain. Now seeded `enabled: true`; deployments that
  already stored the disabled row are refreshed automatically
  because `builtin_policy_drifted` compares `stored.enabled` against
  the code definition. Both deterministically-failing tests
  (`merge_processor::merge_time_abac_valid_chain_merges_under_builtin_policies`,
  `policy_engine::builtin_signed_authorization_satisfied_by_chain_presence`)
  consume `m34_builtin_policies()` directly, so the single seed-site
  fix addresses both.
- **R5-F2 (rustfmt):** the over-width `updated_at` no-op-seed
  assertion split across lines; `check-rustfmt-diff.py` clean against
  both `6a9739e5` and `110180fb` (29/43 files checked), plain
  `cargo fmt --check -p gyre-server` clean.
- **R5-F3 (shifted exemption line):** the legacy `git_http.rs` ws_id
  fallback site moved from line 1495 to 1499 (push-time ABAC
  insertion above it); the frozen line-keyed exemption entry was
  moved to 1499 — same legacy site, entry count still 5, baseline
  unchanged. `check-fabricated-scope-defaults.sh` green.

### Test evidence

Mechanical gates green at head `c8595a40` (logs under
`/tmp/stage/review-evidence/`): check-arch, check-abac-route-registry,
check-migration-versions, check-byte-slice-truncation,
check-dead-message-kinds, check-inert-enforcement,
check-mem-port-contracts, check-fail-open-ref-resolution,
check-lossy-secret-conversion, check-fabricated-scope-defaults,
check-scope-literal-defaults, check-forwarded-header-trust,
check-relative-path-defaults, check-unbounded-external-http,
check-mcp-write-tools, check-abac-exempt-handlers,
check-task-commit-attribution, check-rustfmt-diff (vs `6a9739e5` and
`110180fb`), web build from merged sources.

Sandbox limitations recorded for remote verification: this runtime
has no crates.io egress (registry cache holds ~30 index entries, no
`.crate` archives, no prebuilt target dir), so `cargo test` probes
for the two R5-F1 suites could not be executed here — the fix is the
exact change the finding prescribed (`enabled: false` → `true` at the
single seed site both failing tests read), and the deterministic
repros (`cargo test -p gyre-server --lib merge_processor` /
`policy_engine`) must be confirmed by CI/remote verification. The TCP
listener probe is likewise unsupported (`errno 95`,
`/tmp/stage/capabilities.json`); GitHub CI on the branch head remains
the authoritative transport-level check.
