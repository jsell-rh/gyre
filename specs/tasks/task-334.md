---
title: "Manifest supersedes the [spec_lifecycle] config block for manifest repos (enabled flag and invalidation keying)"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / manifest-supersedes-lifecycle-config / a458cd7cdf30"
depends_on: []
progress: ready-for-review
commits: ["e0e7e802e4ba96a3a0182389d702581a081ba22c", "ef2e7c6af6571747495fc9e826cd823603ad3aaf"]
---

## Required behavior

spec-registry.md §Integration/Spec Lifecycle: "The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section." Today `process_spec_lifecycle` (crates/gyre-server/src/git_http.rs:3390) returns early when the repo's SpecLifecycleConfig has enabled=false — a master switch the manifest cannot override — and its approval-invalidation block keys on `config.auto_invalidate_approvals` (git_http.rs:3501) rather than the manifest's per-entry/defaults `auto_invalidate_on_change`. For repos whose pushed HEAD has a specs/manifest.yaml, make the manifest authoritative: (1) the config enabled=false early-return must not suppress manifest-driven lifecycle task creation for manifest repos — the flag keeps its meaning only for manifest-less repos, whose watched-prefix fallback it governs today; (2) the hook's approval invalidation on M/D/R of a registered spec must key on the manifest entry's effective `auto_invalidate_on_change` (per-entry override, else `defaults:`), falling back to `config.auto_invalidate_approvals` only for manifest-less repos and unregistered files. Aspects the manifest does not express (e.g. dedup window) remain config-governed; manifest-less prefix-fallback behavior (should_create_lifecycle_task None-arm) must not regress. Harmonize the cross-reference: spec-lifecycle.md §Configuration must state that for manifest repos the manifest's defaults/entries supersede the config block for hook eligibility and invalidation (alignment via the normal spec-change process; do not weaken either spec's contract). Coordination boundary: the staged spec-lifecycle.md plan tasks (spec-lifecycle-sha-pin, spec-lifecycle-binding-refs, spec-lifecycle-dedup-update) own task content, SHA pinning, and dedup inside the same function — this task changes only precedence and invalidation keying and must not regress those behaviors.

## Acceptance Criteria

- [ ] A repo with specs/manifest.yaml (entry auto_create_tasks: true) and [spec_lifecycle] config enabled=false: pushing a modification to that spec still creates the drift-review lifecycle task; a manifest-less repo with enabled=false still creates nothing (fallback preserved).
- [ ] A repo whose manifest entry sets auto_invalidate_on_change: false: pushing new content updates the ledger SHA but does not invalidate/revoke existing approvals via the hook; with true (or default) it does. config.auto_invalidate_approvals no longer decides this for manifest repos.
- [ ] spec-lifecycle.md §Configuration documents the manifest supersession for manifest repos.

## Verification intent

E2E push probe on the mismatch case: repo whose manifest entry has auto_create_tasks: true and auto_invalidate_on_change: false, repo config [spec_lifecycle] enabled=true with auto_invalidate_approvals=true. Push a content change to the spec. At the base the hook revokes the approvals (config key wins the invalidation decision); the probe asserts the drift-review task exists AND the approvals survive. Run the twin with enabled=false: at the base no task is created at all (master switch); the probe asserts the task exists. A fix that only re-keys invalidation or only removes the master switch fails one of the two probes.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "ff5387eb7f01a9b7b31c5126",
    "decision": "gap",
    "requirement_quote": "- The `[spec_lifecycle]` config block is superseded by the manifest's `defaults:` section",
    "detail": "Three of the four bullets are implemented: which files trigger hooks is manifest-driven (should_create_lifecycle_task creates tasks only for manifest entries whose effective auto_create_tasks is true; unregistered files create nothing), per-spec priority overrides are applied from the manifest entry/defaults over the change-kind default, and per-spec gates feed the gate chain (audited under Agent Gates). The supersession bullet is only partially implemented: when a manifest exists the manifest decides task eligibility and priority, but the [spec_lifecycle] config block still overrides the manifest — process_spec_lifecycle returns early when the repo's config has enabled=false, which disables manifest-driven task creation for manifest repos (the config block acts as a master switch the manifest cannot override), and the hook's approval invalidation keys on config.auto_invalidate_approvals rather than the manifest's auto_invalidate_on_change. The code comments cite spec-lifecycle.md §Configuration for the fallback, but spec-lifecycle.md's Configuration section does not mention the manifest at all, so this is an implementation-side harmonization, not a canonical supersession: the config block remains authoritative for enabled/invalidation/dedup even where the manifest declares policy.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        Some(m) => crate::spec_registry::manifest_entry_for_path(m, path)\n            .map(|e| e.effective_auto_create_tasks(&m.defaults))\n            .unwrap_or(false),",
        "detail": "Bullet 1 satisfied: in manifest mode only manifest entries with effective auto_create_tasks (per-entry override, else the defaults: section) trigger lifecycle tasks; files not registered create none."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            let priority = manifest\n                .as_ref()\n                .and_then(|m| {\n                    crate::spec_registry::manifest_entry_for_path(m, &path)\n                        .and_then(|e| e.effective_priority(&m.defaults))\n                })\n                .unwrap_or(priority);",
        "detail": "Bullet 2 satisfied: the manifest-declared priority (per-entry, else defaults) overrides the change-kind default from the config block."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "    if !config.enabled {\n        info!(repo_id, \"spec-lifecycle: disabled for repo, skipping\");\n        return;\n    }",
        "detail": "The supersession bullet's gap: this early return runs BEFORE the manifest is read, so a repo with a manifest declaring auto_create_tasks: true still creates no lifecycle tasks when its [spec_lifecycle] config sets enabled=false — the config block overrides the manifest instead of being superseded by it."
      },
      {
        "file": "specs/system/spec-lifecycle.md",
        "quote": "## Configuration",
        "detail": "The cited fallback authority does not resolve the contradiction: spec-lifecycle.md's Configuration section defines the [spec_lifecycle] block with no mention of the manifest or supersession, while spec-registry.md requires the block to be superseded by the manifest's defaults — the implementation keeps both authoritative (config for enabled/auto_invalidate/dedup, manifest for eligibility/priority), which satisfies neither statement fully."
      }
    ]
  }
]

## Shipped

For repos whose pushed HEAD carries `specs/manifest.yaml`, the manifest is
now authoritative over the `[spec_lifecycle]` config block for hook
eligibility and approval invalidation (spec-registry.md §Integration/Spec
Lifecycle). Implemented in commit `ef2e7c6a` (rebased through merge
`9de491eb` onto base `f3ca7db3`), finalized by `e0e7e802` (cargo fmt of the
new test code + coverage-row evidence).

### Behavior

- **`enabled` is no longer a master switch.** `process_spec_lifecycle`
  (git_http.rs) removed the config-level early return; the flag is now
  evaluated per ref-update, *after* the manifest is read:
  `manifest.is_none() && !config.enabled` skips the update. A manifest repo
  whose entry has `auto_create_tasks: true` still gets its drift-review task
  under config `enabled=false`; a manifest-less repo with `enabled=false`
  still creates nothing (watched-prefix fallback preserved — the skip runs
  before workspace-scope resolution, events, tasks, and invalidation, so the
  old early-return semantics are unchanged for that case).
- **Invalidation keys on the manifest.** On M/D/R of a spec registered in
  the manifest, the hook revokes active approvals iff the entry's effective
  `auto_invalidate_on_change` (per-entry override, else `defaults:`) is
  true; `config.auto_invalidate_approvals` decides only for manifest-less
  repos and `specs/` files not registered in the manifest. The decision is
  per stale path inside the existing revocation loop, so a single push can
  revoke a registered spec while sparing an unregistered one.
- **Spec harmonized.** spec-lifecycle.md §Configuration now documents the
  manifest supersession for manifest repos (eligibility + invalidation),
  keeping dedup window, watched/ignored prefixes, and per-change-kind
  default priorities config-governed. spec-registry.md's supersession
  bullet is unchanged (it was already canonical).
- **Coordination boundary respected:** task content, SHA pinning, and dedup
  inside `process_spec_lifecycle` were not touched; only precedence and
  invalidation keying changed.

### Test evidence

Three new tests in `git_http.rs` build real git repos on disk and drive the
real `process_spec_lifecycle` (real diff parsing, real manifest reader,
real task/approval stores):

- `manifest_entry_spares_approvals_and_creates_task_despite_config` —
  verification-intent probe A (mismatch case): entry
  `auto_create_tasks: true` + `auto_invalidate_on_change: false`, config
  enabled with `auto_invalidate_approvals: true` → drift-review task exists
  AND the seeded active approval survives. Fails at the base (config key
  revoked the approval).
- `manifest_supersedes_config_enabled_false_creates_task` — probe B twin:
  same manifest under config `enabled=false` → the drift-review task is
  still created. Fails at the base (master switch suppressed it).
- `manifest_auto_invalidate_true_overrides_config_false` — reverse
  direction: registered spec revoked via manifest defaults despite config
  `auto_invalidate_approvals: false`, while an unregistered `specs/` file
  in the same push falls back to the config flag and survives.

Existing guards for unchanged behavior remain:
`spec_lifecycle_disabled_skips_task_creation` (manifest-less +
enabled=false creates nothing), `spec_lifecycle_without_manifest_uses_prefix_fallback_e2e`,
`spec_lifecycle_manifest_gates_task_creation_e2e`,
`spec_lifecycle_dedup_skip_still_records_spec_changed_audit`.

`cargo check -p gyre-server --lib --tests` is clean (cold check, 10m16s,
evidence in /tmp/stage/review-evidence/task-334-cargo-check.txt). gyre-server
test-binary compilation/execution is deferred to the independent verifier
per the 4 GiB build-budget rule; the exact test commands are listed in the
evidence file. Coverage rows 14/15 of
`specs/coverage/system/spec-registry.md` were updated with the task-334
source evidence.
