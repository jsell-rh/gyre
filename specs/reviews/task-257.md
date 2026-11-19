# Review — task-257 (Publish reviewed spec backlog for specs/development/speed-backpressure.md)

Task: publish the independently reviewed missing-work plan (plan_id `9a77d1e70c18...`) for `specs/development/speed-backpressure.md` — task documents and section bindings only, no implementation.
Commits under review: `f046fe63` ("chore: publish reviewed spec backlog 9a77d1e70c18"), base `bd611083`.
Verdict: **approved** — publication is faithful, complete, non-duplicative, and non-destructive. One non-blocking factual error in the inherited plan text is recorded below for the task-256 implementer.

## Probes run

All probes saved under `/tmp/stage/review-evidence/` (`probes.txt`, `run-probes.sh`, `hashcheck.py`, `contentcheck.py`, `pemscan.py`, `pem-fixture-finding.txt`):

- Diff scope `bd611083..f046fe63`: exactly 3 files — `specs/task-spec-inputs.json` (M, +10), `specs/tasks/task-256.md` (A, 105), `specs/tasks/task-257.md` (A, 224). Zero diff lines in `specs/coverage/`, zero outside `specs/`.
- `specs/task-spec-inputs.json` parses as JSON; new `sections["backlog/2a09dd756eff40d15b609010"]` = `{file: specs/development/speed-backpressure.md, heading: [The Wheel: Speed & Backpressure, Pre-Commit Hooks]}` (heading path matches the actual spec structure); new binding `"Spec gap spec-17abbc950d9bce3d8a575239 / commit-time-backpressure-invariants / 8207800eca00" -> ["backlog/2a09dd756eff40d15b609010"]`; exactly one binding references the section id; key format matches sibling entries (e.g. `spec-c62aac...` family).
- Verbatim-publication check (programmatic): plan title, required_behavior (all 7 deliverable paragraphs), all 9 acceptance bullets, and negative_probe from the task-257 body JSON appear verbatim in `specs/tasks/task-256.md`. task-256 `spec_ref` equals the binding key; `8207800eca00` is the 12-hex prefix of the audit's requirements hash.
- Duplicate-work search: no file under `specs/tasks/` other than task-256/257 references `speed-backpressure`; nothing outside the new files + registry references section id `2a09dd756eff40d15b609010`. Task numbering is sequential (256, 257 follow 255). The referenced ci-docs-release script `scripts/check-i18n-coverage.sh` does not exist in the tree, so the plan's conditional reuse instruction ("REUSE if present; implement to identical contract only if absent") cannot fork a duplicate.
- Frontmatter: task-256 `progress: not-started`, `depends_on: []`, `commits: []` — satisfies "new tasks remain not-started until this backlog publication merges". Dependencies `[]` are correct (no prerequisite task; the i18n overlap is handled by the conditional reuse note).
- Audit challenge (all functional claims verified against the candidate tree): `.pre-commit-config.yaml` registers exactly 78 hooks; no shellcheck/backup/secret-scanner/cargo2nix/i18n-coverage/web-build hook exists (grep by function across the config, both CI workflows, and scripts/); no `Cargo.nix` anywhere and flake.nix uses crane; `web/src/locales/` holds only `en.json`; `.gitignore` backup mitigation is a single `*.swp` line; `crates/gyre-server/src/email.rs` `type_heading()` has exhaustive `Locale::En` and `Locale::De` matches with 41 `NotificationType::` arms each (matching the 41 enum variants in `crates/gyre-common/src/notification.rs`) and no `_ =>` wildcard; `crates/gyre-cli/src/main.rs` has only a locale CLI arg. All 9 audit `source_evidence` sha256s match the file blobs at both base `bd611083` and candidate `f046fe63`. The gap decision on §Pre-Commit Hooks is accurate: mechanism present, em-dash enforced, web build CI-only, the other six enumerated invariants unenforced anywhere. The intro section's `context` classification is correct (aspirational rationale; the "for now" claim is true in production).

## Verified working (no findings)

- Publication is exact: only the assigned task documents were created; the plan (audit, required behavior, acceptance, negative probe) is reproduced verbatim with the assigned name task-256.
- Registry additions follow existing conventions and add exactly one sections entry plus one binding for the audited gap section; no existing task, spec, registry entry, coverage status, product file, or gate was modified.
- No duplicate active assignment: the section is bound once, to one new task; no sibling task touches this spec.
- No missing dependency; new task is `not-started`.
- Commit message follows conventional commits, matching the established publication pattern (cf. `f3d157bc`).

## Non-blocking observation (recorded for task-256 and its reviewer)

- The published plan's required_behavior item 2 states the secrets scanner "must exit 0 on the untouched tree (the only tree-wide near-match is a literal `\"[REDACTED PRIVATE KEY]\"` placeholder in crates/gyre-adapters/src/llm/rig_vertexai.rs, which anchored patterns do not match)". **That factual claim is wrong.** Byte-exact substring search shows the file contains no such literal; `crates/gyre-adapters/src/llm/rig_vertexai.rs:1028` instead holds a fake test-fixture PEM block (`-----BEGIN RSA PRIVATE KEY-----` marker with body `fake`), committed identically at base and candidate (file sha256 `2ecb2735...`). The specced anchored pattern *will* match this line, so "exit 0 on the untouched tree" requires either a sharper anchor (e.g. require a base64 body, which `fake` is not) or an entry in the plan-permitted NEW check-owned exemption file. Both paths are already authorized by the plan's own text, so the published task remains implementable as specced and no acceptance criterion is unsatisfiable — hence non-blocking for this publication (the candidate had no discretion to edit the plan; "publish exactly").
  - Root-cause note for the controller: the review sandbox's tool-output filter rewrites PEM blocks in command output as `[REDACTED PRIVATE KEY]` (observed live: `sed -n '1028p' <file>` printed the bracketed placeholder while the byte dump of the identical command showed the true PEM marker). The upstream audit almost certainly recorded the redacted display as file content. Evidence: `/tmp/stage/review-evidence/pem-fixture-finding.txt`.
  - Task-256 reviewers should expect the secrets-scan acceptance probe to surface `rig_vertexai.rs:1028` and treat a narrowly-scoped exemption for that fake fixture (or a body-validating anchor) as conformant, not as scanner weakness.

Test-inflation check: not applicable — task creation only; no product code, tests, or coverage claims shipped. The task's own text correctly states "Task definitions and explicit bindings only; no product implementation claimed."

Generated-output note: none — no generated artifacts (`web/dist/`, `specs/coverage/SUMMARY.md`) were touched by this candidate.
