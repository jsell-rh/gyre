---
title: "Spec API read path: linked_tasks under both spellings and per-spec open task counts in the generated index"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / spec-api-read-completion / a458cd7cdf30"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Two read-path defects in the spec API surface. (1) GET /api/v1/specs/{path} (crates/gyre-server/src/api/specs.rs:385-393) populates linked_tasks via TaskRepository::list_by_spec_path with the manifest-relative spelling only ('system/x.md'), while the post-receive lifecycle hook records tasks with the git-rooted spelling ('specs/system/x.md', git_http.rs:3630) and the repository matches spec_path exactly — so hook-created drift-review/implementation tasks are invisible in the single-spec response; the spec-lifecycle patrol already works around exactly this dual-spelling problem by querying both forms (spec_lifecycle_patrol.rs rooted_path/alt). Fix the single-spec endpoint — and the history endpoint's linked_tasks at specs.rs:2191, which shares the defect — to resolve tasks under both spellings (the patrol's established approach), so linked_tasks contains hook-created tasks. (2) GET /api/v1/specs/index (specs.rs:312-342) generates the markdown index with only Spec | Status | SHA columns; the spec requires "Open task count per spec". Query the task repository using the same both-spelling resolution and add a per-spec open-task count (open = not Done/Cancelled) to each row. Keep the satisfied parts unchanged: grouping by directory, per-spec approval status, links to spec files.

## Acceptance Criteria

- [ ] After a push that triggers the lifecycle hook for specs/system/x.md, GET /api/v1/specs/system/x.md returns the hook-created task's id in linked_tasks (and GET history likewise).
- [ ] GET /api/v1/specs/index markdown carries an open-task-count column whose value equals the number of open tasks for each spec — N for a spec with N open hook-created tasks, 0 for a spec with none.

## Verification intent

Seed storage with a task whose spec_path is exactly 'specs/system/x.md' (the spelling the hook writes) plus a ledger entry 'system/x.md'. GET /api/v1/specs/system/x.md: at the base linked_tasks is empty (exact-match query misses the rooted spelling) — the probe asserts the task id is present. Same seed against the index: at the base the document has no task-count column at all — the probe asserts the row for system/x.md shows 1 and a taskless spec's row shows 0. A fix that only widens one endpoint or hardcodes counts cannot satisfy both probes.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "16caff87fba84183d8dfcc06",
    "decision": "gap",
    "requirement_quote": "| `GET /api/v1/specs/{path}` | GET | Single spec: policy, approval status, linked MRs/tasks, drift |",
    "detail": "All seven endpoints in the table are registered and implemented against the real ledger and approval stores: GET /api/v1/specs (list), GET /api/v1/specs/{path} (single spec with policy/status/links/drift and best-effort content), POST .../approve (with optional Sigstore signature), POST .../revoke (reason required), GET .../history, GET /api/v1/specs/pending, GET /api/v1/specs/drifted. The gap is in the single-spec endpoint's linked-tasks delivery: it populates linked_tasks via TaskRepository::list_by_spec_path with the manifest-relative spelling only ('system/x.md'), while the post-receive lifecycle hook records tasks with the git-rooted spelling ('specs/system/x.md') and the repository matches spec_path exactly — so hook-created drift-review/implementation tasks are invisible in GET /api/v1/specs/{path}'s linked_tasks for specs whose tasks were recorded the hook's way. The spec-lifecycle patrol works around exactly this dual-spelling problem by querying both forms; the API does not. linked_mrs derivation (MR spec_ref prefix match) does not have this defect.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        .route(\"/api/v1/specs\", get(specs::list_specs))\n        .route(\"/api/v1/specs/pending\", get(specs::list_pending_specs))\n        .route(\"/api/v1/specs/drifted\", get(specs::list_drifted_specs))\n        .route(\"/api/v1/specs/index\", get(specs::spec_index))",
        "detail": "The listed endpoints exist (together with /api/v1/specs/:path, /approve, /revoke, /history registered in the same router block), so the surface itself is complete."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "    // Populate linked_tasks from tasks with matching spec_path.\n    entry.linked_tasks = state\n        .tasks\n        .list_by_spec_path(&spec_path)\n        .await\n        .unwrap_or_default()",
        "detail": "linked_tasks is populated with a single-spelling exact query: spec_path here is the manifest-relative form the ledger uses ('system/x.md'), so tasks stored under the alternate spelling never match."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            task.spec_path = Some(path.clone());",
        "detail": "The lifecycle hook records tasks with the git-diff path spelling ('specs/system/x.md') — the other spelling than the one get_spec queries, so the hook's drift-review/implementation tasks are missing from the single-spec response's linked_tasks."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/task.rs",
        "quote": "                .filter(tasks::spec_path.eq(&sp))",
        "detail": "The repository matches spec_path by exact equality (no normalization, no prefix logic; the PostgreSQL adapter is the same), so the two spellings never meet — the linked-tasks half of the row underreports for hook-created tasks."
      },
      {
        "file": "crates/gyre-server/src/spec_lifecycle_patrol.rs",
        "quote": "    for query in [rooted_path, alt.as_str()] {",
        "detail": "The dual-spelling hazard is real and known: the patrol's spec_has_live_task queries BOTH spellings to answer spec/task coverage, which the API's single-spec endpoint does not — confirming the linked-tasks linkage in GET /api/v1/specs/{path} is incomplete rather than a different intentional design."
      }
    ]
  },
  {
    "id": "9df024e265cff9471afd92d4",
    "decision": "gap",
    "requirement_quote": "Returns a markdown document with:\n- All specs grouped by directory\n- Current approval status per spec\n- Links to spec files\n- Open task count per spec",
    "detail": "GET /api/v1/specs/index is implemented and generates a real markdown document from the ledger: specs are grouped by directory (BTreeMap on the first path segment), each row carries the current approval status, and rows link to the spec files. Three of the four required contents are present. The fourth — 'Open task count per spec' — is absent: the generated table has only Spec | Status | SHA columns and the handler never queries the task repository, so the index cannot tell a reader which specs have open work. The web UI spec dashboard exists (SpecDashboard.svelte renders the specs list), so that closing sentence is met; the gap is confined to the missing per-spec open task count in the generated document.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "        let dir = entry.path.split('/').next().unwrap_or(\"other\").to_string();",
        "detail": "Grouping by directory is real (BTreeMap<String, Vec<SpecLedgerEntry>>), and each row renders title, approval_status, and a markdown link to specs/{path} — three of the four required contents."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "        md.push_str(\"| Spec | Status | SHA |\\n\");\n        md.push_str(\"|------|--------|-----|\\n\");",
        "detail": "The generated table columns are only Spec, Status, and truncated SHA: there is no open-task-count column, and the handler body (inspected in full from `pub async fn spec_index` to the response return) contains no query against state.tasks — the specced 'Open task count per spec' is not produced."
      },
      {
        "file": "web/src/components/SpecDashboard.svelte",
        "quote": "   * SpecDashboard — S4.5 Specs View (list pane)",
        "detail": "The web UI half of the section (spec dashboard rendering spec state) exists as a live Svelte component fed from the specs API."
      }
    ]
  }
]
