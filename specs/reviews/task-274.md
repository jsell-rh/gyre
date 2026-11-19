# Review — task-274 (Publish reviewed spec backlog for specs/system/admin-panel.md)

Candidate: `161580311e13a240bffa0afa512a1b67637acdf2` (base `67cc03988b938c4635973761c08108cee8f8459d`).
Verdict: **approved** — bindings-only publication, faithful to the reviewed plan; no product change.

## Scope of the diff (exhaustive)

`git diff --name-status base..candidate` → exactly three changes, 411 insertions, 0 deletions:

- `A specs/tasks/task-273.md` — the assigned work task (admin-panel-remaining-gap), `progress: not-started`, `depends_on: []`.
- `A specs/tasks/task-274.md` — the publication task doc embedding the full plan JSON.
- `M specs/task-spec-inputs.json` — one new `sections` entry (`backlog/42df7a7e48e1904b76eface1` → `specs/system/admin-panel.md`, heading `["Admin Panel"]`) and one new `bindings` entry mapping the task's spec_ref to that section.

No product code, script, spec, coverage matrix, exemption file, or gate was touched. Matches the task contract ("Task definitions and explicit bindings only; no product implementation claimed") and the established publication pattern (task-267 / 67cc0398 shipped the identical file shape).

## Fidelity to the plan

- task-273 `## Required behavior` is **byte-identical** to the plan's `required_behavior` (programmatic comparison: True).
- All **11/11 acceptance criteria** match the plan verbatim (0 mismatches).
- The plan's `negative_probe` is embedded verbatim in the task's Verification intent section, together with the required "update coverage rows only after implementing" guard.
- task-274.md's embedded plan JSON **decodes equal** to the plan in the assignment body (programmatic comparison: True), including `plan_id c52f9b2f…`, audit base/requirements hashes, and `assigned_names` (task-273 / task-274 / base 67cc0398).
- The binding key follows the existing convention exactly (`Spec gap <doc-id> / <task-key> / <requirements-hash-prefix>`, cf. task-261/262); the `e61ffd082155` prefix matches the audit's requirements hash. The `sections` heading `["Admin Panel"]` is the spec's H1; `specs/system/admin-panel.md` is a 17-line single-section document whose full body is quoted verbatim in the requirement quote — the binding covers the whole spec with nothing dropped.

## Audit challenge (spot-checked every gap claim against the candidate tree)

- (a) SOPS/Vault absent: `git grep -iE 'sops|vault' -- crates web/src scripts docker` → only a false positive (`lcsOps` in `web/src/lib/lineDiff.js`); actual backend is `encrypt_value` AES-256-GCM at `crates/gyre-adapters/src/sqlite/secret.rs:79`. Confirmed.
- (b) Server logs absent: no `server/logs` route in `api/mod.rs` (grep exit 1); TenantSettings has 13 tabs (users…bcp), none for server logs. Confirmed.
- (c/d) Maintenance jobs absent: 19 registered jobs in `start_job_registry`; none of `repo_maintenance`, `oauth_state_cleanup`, `remote_agent_cleanup`, `job_history_cleanup`; no `oauth_state` anywhere in `crates/`. Confirmed.
- (e) Single rotation handler: `static ROTATION_HANDLERS: &[&dyn SecretRotationHandler] = &[&OAUTH_REFRESH];` (`secret_rotation.rs:173`). Confirmed.
- (f) Job history hollow: `JobRun` has no `logs` field; `history: HashMap<String, VecDeque<JobRun>>` with `MAX_HISTORY_PER_JOB = 50` in-process. Confirmed.
- (h) Admin audit reads the in-memory ring: `state.telemetry_buffer.list_all_since(...)` at `api/admin.rs:299`. Confirmed.

The audit's SATISFIED-side claims also held up where probed (secrets API routes, jobs tab, 13 TenantSettings tabs incl. secrets). The gap classification is therefore accurate against the canonical spec, whose normative bullets are unambiguous ("SOPS as default, with Vault support as an option", "Server logs - accessible from the admin UI", etc.).

## Decomposition / dependencies / duplicate work

- One task (`task-273`) covers all eight gap letters (a)–(h). This is the reviewed plan's own decomposition; it is defensible because the deliverables share substrate (durable job-run logs are required by the maintenance/refresh/cleanup jobs; the new ports/adapters follow one pattern) and the task enumerates each gap letter with matching acceptance criteria and negative probes. Nothing in the plan is unassigned.
- No duplicate **active** assignment: only task-273/274 reference `admin-panel` or section `42df7a7e…`; no other binding references the section. `task-097` (platform-model §7 Storage Backend, `progress: complete`, not active) shipped AES-256-GCM with a documented divergence from the SOPS wording — the plan explicitly cross-references platform-model §7 as canonical and requires re-encryption of pre-change AES-GCM secrets, so the overlap is acknowledged and handled rather than duplicated. The pre-existing platform-model row 44 coverage note (which records only "Vault integration absent" as residual) is untouched by this diff, as the task requires; its accuracy is owned by the coverage auditors, not this publication.
- `depends_on: []` matches the plan; the only prior art it builds on (task-097's secrets substrate, the job registry) is already in-tree, so no missing dependency.
- Task numbers 268–272 are absent from this branch; the controller's `assigned_names` assigned 273/274 explicitly, so the gap corresponds to other in-flight pipeline reservations, not a candidate defect.

## Mechanical checks run

- `specs/task-spec-inputs.json` parses as valid JSON at the candidate; every `bindings` value resolves to an existing `sections` entry; no duplicate binding for the new section.
- `python3 scripts/test_pipeline_requirements.py` → 23 tests OK.
- `python3 -m unittest scripts.test_pipeline_decisions` → 5 tests OK.
- `bash scripts/check-task-commit-attribution.sh` → OK (the candidate's `chore: publish…` commit is specs-only, not a product-surface commit).
- Working tree clean at the candidate SHA throughout the review; no source files modified.

Probe evidence: `/tmp/stage/review-evidence/` (probe-sops-vault.txt, probe-registered-job-names.txt, probe-jobrun-struct.txt via jobs.rs inspection, probe-serverlogs-route.txt, probe-adminaudit.txt, probe-rotation-handlers.txt, probe-oauth-state.txt, probe-tenant-tabs.txt, probe-pipeline-req-tests.txt, probe-pipeline-dec-tests.txt).

## Verdict

Approved. The candidate publishes exactly the reviewed plan: task definitions and explicit section bindings only, faithful verbatim, correct conventions, no duplicate active assignment, new task `not-started`, and no change to any product behavior, spec, coverage status, or gate. The five negative probes embedded in task-273 were each verified to genuinely fail on the current code, so the follow-up task's verification intent is sound.
