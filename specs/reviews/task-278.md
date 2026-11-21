# Review — task-278 (Dependency auto-detection: full extraction forms and every-push trigger)

Spec: `specs/system/dependency-graph.md` §Automatic Detection (gaps 12ff1c4b / e07d32e2 / 8244b7d9).
Candidate under review: `1dbe2c1f` (branch `pipeline/task-278/de008f67a76e423e95b20789187a0713-3`, base `9e7cf62e`; product commit `05ba6613`, checkpoint `dcdb28f1`, docs `06de2257`, process `1dbe2c1f`).
Verdict: **needs-revision** — two findings, both committed tests that fail against the candidate's own (correct) production code. The production implementation itself satisfies the task contract on every acceptance criterion, verified behaviorally.

## Round 1

Probes (checkout at candidate; evidence under `/tmp/stage/review-evidence/`, summary in `task-278-evidence.md`):

- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (10m43s; server test-target execution deferred to verifier per 4 GiB model-phase budget).
- `bash scripts/check-byte-slice-truncation.sh` → exit 0; `bash scripts/check-arch.sh` → exit 0.
- **Verbatim parser probe**: the nine task-278 parser functions (`detect_cargo_deps`, `extract_toml_string_value`, `repo_name_from_reference`, `is_git_reference`, `detect_package_json_deps`, `detect_pyproject_deps`, `detect_openapi_refs`, `repo_name_in_url`, `detect_mcp_refs`) mechanically extracted from the candidate `git_http.rs` into a standalone crate and executed against 28 assertions mirroring the acceptance criteria and over-match guards — all pass.
- **Git tree probe** replicating the `make_commit` test-helper mechanics (fresh scratch index per call) — proves each `make_commit` commit's tree contains only the files passed to that call.
- Full diff read (`git_http.rs` +1103/−402, `api/dependencies.rs` tests, coverage rows 5/6/8); reconcile/§4 logic re-read and confirmed untouched.

Verified working (no findings on production behavior):

- **Every-push trigger (gap 12ff1c4b)**: the `detect_dependencies_on_push` call moved out of the `filter(|u| u.refname == default_ref)` loop into `for update in applied_updates.iter()` (git_http.rs:1601) — any branch. `verify_applied_ref_updates` already excludes deletes (deleted refs fail `rev-parse --verify`), so only applied non-delete updates run detection. The changed-dependency-file early return is removed entirely; all eight detectors always parse the tree at the pushed tip; `ran_methods` covers all methods each push. Reconcile (unchanged) orphans — never deletes — edges whose declarations vanish, skips Manual edges, and is idempotent for no-change pushes. The repo-created-later case (AC-6 mechanism) works because `state.repos.list()` is re-read every push.
- **Cargo.toml (gap e07d32e2)**: `detect_cargo_deps` extracts both `path = "…"` and `git = "<url>"` inline-table forms with word-boundary key matching (`some-path-crate`/`legit` names cannot trigger `path`/`git` extraction — probed); `version`/`tag`/`rev` captured as the pin; `repo_name_from_reference` handles `<ws>/<repo>.git`, `git@host:` scp forms, `#fragment`/`?query` stripping (all probed). ALL `Cargo.toml` files enumerated tree-wide via `git ls-tree` (`list_files_by_names`), source_artifact = manifest tree path. Registry-only entries ignored.
- **package.json / pyproject.toml**: git URL values (`git+https`, `git+ssh`, `#fragment` as pin) and PEP 508 `pkg @ git+https://…` extracted; `file:`/`workspace:`/Poetry-path forms retained (regression-probed); registry deps ignored; go.mod untouched.
- **API contracts (gap 8244b7d9)**: `detect_openapi_refs` now parses the document (serde_yaml; JSON ⊂ YAML — probed with a swagger.json-shaped doc) and extracts server URLs only — OpenAPI 3.x `servers[].url`, Swagger 2.0 `host`+`basePath` — matched as whole URL labels (`repo_name_in_url`; `xrepo-b.example.com` does not match `repo-b` — probed). `detect_mcp_refs` parses the JSON structure for `mcpServers.<name>.url`/`.server` and `tools[].url`/`.server` only. Description/unrelated-field mentions create NO edge (probed — the pre-fix substring-scan over-match is gone). Contract files enumerated tree-wide (`api/openapi.yaml` carries its tree path as source_artifact). Proto detection untouched.
- **Drift pass**: `version_pinned` resolved from the detected CargoToml edges by (method, target, source_artifact) — consistent with reconcile's identity keys; every Active CargoToml edge post-reconcile is a detected key, so the lookup resolves.
- **Coordination**: no changes to `process_spec_lifecycle`, gates, or any unrelated function; coverage rows 5/6/8 re-evidenced with current line numbers, nothing else touched.
- Pre-existing openapi/mcp tests were updated to the structured behavior (the old substring-expectation test now derives its match from `servers[].url`); reconcile tests unchanged and consistent.

Findings:

- [-] **F1 (failing committed test): `test_detect_package_json_git_ref` (crates/gyre-server/src/api/dependencies.rs:1803) fails when executed.** `detect_package_json_deps` retains the `git+` scheme in the extracted reference — the verbatim-candidate probe returns `deps[0].0 = "git+https://gyre.example.com/ws/repo-b.git"` for the exact test input — but the assertion expects `https://gyre.example.com/ws/repo-b.git` (scheme stripped); the sibling `git+ssh` assertion (scheme retained) passes, so line 1803 alone is wrong. Edge creation is unaffected (`repo_name_from_reference` yields `repo-b` from the `git+` form), but the suite fails. The task text's "Test evidence" claims the new tests pass — only `cargo check` was ever run. Repair: strip the `git+` prefix in `detect_package_json_deps` (keeping `repo_name_from_reference` callers correct either way) or fix the assertion to the retained-scheme value; re-run the test.
- [-] **F2 (failing committed test): `detect_dependencies_on_push_full_extraction_forms` (crates/gyre-server/src/git_http.rs test module) fails when executed.** Its second phase calls `make_commit(&repo_path_str, &[("README.md", …)])`, but `make_commit` builds each commit's tree from only the files passed to that call (fresh scratch index, no parent inheritance — empirically confirmed: `git ls-tree` of such a tip lists only `README.md`, and `git show <tip>:Cargo.toml` exits 128). At that tip no manifest exists, so (a) the expected repo-c CargoToml edge can never be created — `.expect("repo-c edge after repo creation + no-dep-file push")` panics — and (b) reconcile marks all five repo-b edges Orphaned, failing the trailing all-Active/count-5 assertions. The AC-6 scenario requires the pushed tip to still contain the manifest that references repo-c with only a README changed (e.g. `make_commit` with all sha1 files plus README.md). Repair: fix the test setup; re-run.

Both findings are test-side; no production behavior change is required. They are blocking because the verifier's test run fails the candidate as shipped, and the shipped evidence claims ("new tests … pass") are unverified.

Test-inflation check: none found beyond the above — the probes are real (feature-branch push through actual git-receive-pack with packfile; polling the dependency store; over-match guards assert both the positive edge and the negative non-edge). The failures are authoring errors, not mirrored-logic or stub-backed assertions.

Verifier must run (server test-target compilation exceeds the model-phase budget):

- `cargo test -p gyre-server --lib test_detect_package_json_git_ref` (expected FAIL until F1 fixed)
- `cargo test -p gyre-server --lib detect_dependencies_on_push_full_extraction_forms` (expected FAIL until F2 fixed)
- `cargo test -p gyre-server --lib push_to_non_default_branch_detects_git_dependency_then_orphans` (coherent by inspection; must pass)
- `cargo test -p gyre-server --lib test_cargo_deps test_repo_name_from_reference test_detect_package_json test_detect_pyproject test_detect_openapi test_detect_mcp test_detect_go_mod`
- Full api::dependencies::tests reconciliation suite and full workspace suite + Clippy per standard verification.
