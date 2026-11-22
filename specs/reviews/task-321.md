# Review — task-321 (Publish reviewed spec backlog for specs/development/ci-docs-release.md)

Spec: `specs/development/ci-docs-release.md` (publication of plan `a6e170077d3916adf545dc2d6627606f0766342baf60ae0965d0b3a25b9ac267`).
Candidate under review: `4e17e76ba0949aa0f03d4abcb46dec2b4f97e983` (branch `pipeline/task-321/a476ee6483b54bd29cbf6dbaf215796f-1`, base `52d7b66b`; publication commit `cf6abbd4`, rebase merge `c15e8fe8`, notes `4e17e76b`).
Verdict: **approved** — no findings. Task creation only; no product implementation claimed or needed.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `git diff --name-status 52d7b66b..4e17e76b` → exactly 9 files: 8 new task docs (`specs/tasks/task-314.md` … `task-321.md`) + `specs/task-spec-inputs.json`. No product file, spec, coverage matrix, script, exemption file, or workflow touched.
- Plan-to-document fidelity (programmatic, `fidelity-check3.txt`, `discovery-fidelity.txt`): extracted the plan JSON from the assignment body and asserted, per task (314–320): title, full `required_behavior`, every acceptance criterion, negative probe, and discovery-evidence JSON record match the plan **verbatim**; `covers` equals the plan's gap-section IDs; `depends_on` equals the plan's dependencies mapped to assigned names (task-318 → [task-314], task-320 → [task-319], rest []); `progress: not-started` on all seven.
- Registry integrity (`rebase-union.txt`): candidate `sections`/`bindings` equal the exact union of both rebase parents (`cf6abbd4` + `52d7b66b`); nothing removed or modified from the base; JSON valid; no conflict markers. Each of the 7 section IDs appears exactly twice in the registry (section definition + one binding) — no other plan or task binds these sections (duplicate-work check; the speed-backpressure / supply-chain / agent-gates plans referenced for partitioning are not yet published, and the task texts carry explicit consume-don't-fork instructions for them).
- Section-ID resolution: all 7 `backlog/<id>` entries resolve to real heading paths in `specs/development/ci-docs-release.md` (fence-aware heading walk; initial misses were the changelog example's in-fence `#` lines, not registry defects). Binding-key suffix `d1e5376399b8` = first 12 hex of the plan's requirements hash, matching the dogfooding publication convention (task-312/313).
- Audit challenge against the current tree (all 7 gap claims confirmed real): no `astro.config.*` / starlight dependency anywhere; `.github/workflows/` contains only `ci.yml` + `e2e.yml` (no Pages deployment); no `scripts/check-*i18n*` or doc-freshness script; `ChangelogEntry` has no `spec_ref` field; `release_prepare` gates on `UserRole::Admin` only (line 57); `check_pre_accept_gates` early-returns for repos with no configured gates (git_http.rs:2226) and the pre-accept whitelist includes `revert`; `gyre --version` comes from clap's manifest `version` attribute. The spec file is byte-identical to the audit base `7e7462334` (empty diff), so the plan is still valid for this tree.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 (no unlisted product-surface commits; the branch's three commits touch only specs/).
- Task-doc format matches the established publication convention (compared with `specs/tasks/task-312.md` from the dogfooding publication: same section layout, same boilerplate, same spec_ref naming).

## Assessment

- Every acceptance criterion of the publication contract holds: faithful task documents, no duplicate binding or missing dependency, zero product/coverage/enforcement changes, all new tasks `not-started`.
- Decomposition is sound: 7 audited gaps → 7 tasks, one each; satisfied/context sections unassigned; the two cross-task dependencies (release flow → docs site; build-time injection → dev-version computation) are real data/control dependencies, not ordering noise.
- The rebase repair preserved both concurrent publications exactly (programmatic union proof), and the shipped notes describe it accurately.
- No runtime probes are required: the diff contains no compilable or executable artifact.

## Verifier notes

Nothing deferred beyond the executor-owned gates. If the verifier rebuilds `specs/coverage/SUMMARY.md`, development specs have no per-spec coverage matrix (only `specs/coverage/system/`), consistent with prior development-spec publications.
