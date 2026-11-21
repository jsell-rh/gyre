# Review: task-278 — Dependency auto-detection: full extraction forms and every-push trigger

**Candidate:** `5221a2648bc70339c8ba1b1507e958848ea4a853` (base `077d9421069afb2ef2e788b74285186176084b7c`)
**Reviewer verdict:** approved — independent evidence supports the task contract.
**Evidence:** `/tmp/stage/review-evidence/` (cargo-check logs, verbatim-extraction parser probe + mutation probe, attribution checks)

## Scope check

Diff touches `crates/gyre-server/src/git_http.rs` (detection helpers + post-receive call site only — `process_spec_lifecycle` and gates untouched), `crates/gyre-server/src/api/dependencies.rs` (test module only; all seven hunks inside `mod tests`, no production API change), `specs/coverage/system/dependency-graph.md` (rows 5/6/8 re-evidenced), `specs/tasks/task-278.md`. No unrelated tasks, scripts, or verifier files modified.

## What was verified

**Trigger (gap 12ff1c4b).** The `detect_dependencies_on_push` call moved out of the `applied_updates.iter().filter(|u| u.refname == default_ref)` loop into a plain loop over all applied updates (git_http.rs:1601-1608), and the changed-dependency-file early return (the whole `diff-tree`/`has_*` block) is deleted — all eight detectors parse the tree at the pushed tip on every push. Deletes cannot reach the loop: `verify_applied_ref_updates` (git_http.rs:2760) only admits updates whose refname `rev-parse --verify`-resolves to `new_sha`, which a deleted ref cannot. `ran_methods` now unconditionally covers every method, so reconcile orphaning stays correct; `reconcile_dependencies` itself is unchanged (§4 untouched, as required).

**Code manifests (gap e07d32e2).** `detect_cargo_deps` parses `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` inline-table entries with both `path = "…"` and `git = "<url>"` keys via word-boundary key matching (`extract_toml_string_value`), version pinned from `version`/`tag`/`rev`; `repo_name_from_reference` extracts the repo name (last path segment, `#fragment`/`?query` stripped, `.git` removed, `git@host:` scp form handled). All `Cargo.toml` files in the tree are enumerated via `git ls-tree` (`list_files_by_names`), manifest tree path carried as `source_artifact`. package.json gains git+https/git+ssh value extraction; pyproject gains PEP 508 `pkg @ git+https://…`. go.mod unchanged. Registry deps remain ignored (matching against `state.repos.list()` at the call site).

**API contracts (gap 8244b7d9).** `detect_openapi_refs` now parses the document (serde_yaml; JSON is valid YAML) and extracts server URLs only — OpenAPI 3.x `servers[].url`, Swagger 2.0 `host`+`basePath` — matched as whole URL labels (`repo_name_in_url`), so a name in a description/title creates no edge. `detect_mcp_refs` parses the JSON structure for `mcpServers.<name>.url`/`.server` and `tools[].url`/`tools[].server` only. Contract files enumerated tree-wide (`api/openapi.yaml` carries its tree path). Proto import detection unchanged.

## Independent probes run (model phase)

1. `cargo check -p gyre-server` — exit 0 (`task-278-cargo-check.log`, 8m35s). Candidate compiles.
2. `cargo check -p gyre-server --tests` — exit 0 (`task-278-cargo-check-tests.log`, 4m35s). New e2e/unit tests type-check.
3. **Parser behavior probe** (`task-278-parser-probe/`): the parser functions extracted *verbatim* from the candidate (extraction scripted in `extract.sh`) compiled into a standalone crate; 17/17 behavior tests pass, covering AC-1 (git-only entry, version/tag/rev), AC-3/AC-4 over-match guards (description-only and unrelated-field mentions produce no ref; `xrepo-b.example.com` does not match `repo-b`), Swagger 2.0 host+basePath, JSON documents, AC-5 (package.json git refs with `#fragment` versions; pyproject PEP 508), file:/workspace:/poetry regression forms, multibyte UTF-8 safety, and malformed-document fail-closed.
4. **Mutation discrimination probe** (`task-278-mutation-probe.log`): reverting `detect_openapi_refs` to the base substring scan makes the over-match tests fail; reverting the Cargo parser to path-only extraction makes the git-ref tests fail. The tests are discriminating, not self-confirming.
5. `scripts/check-arch.sh`, `scripts/check-byte-slice-truncation.sh` — pass. `scripts/check-task-commit-attribution.sh` — passes against the committed candidate (verified in a temp worktree at `5221a264`; the failure in the live tree is an artifact of the controller's working-tree reset of task-278.md to `commits: []`, not a candidate defect). Clippy `manual_pattern_char_comparison` (the prior verification finding) is clean on the extracted functions — repair commit 78bc6dac is effective and behavior-identical.
6. Coverage rows 5/6/8 line-number citations spot-checked against the candidate source (1603, 3807, 3890, 3941, 4082, 4179, 4218, 4259, 4524, 4548) — accurate.

## Deferred to the independent verifier (server test builds exceed the model-phase budget)

- `cargo test -p gyre-server` — in particular `git_http::tests::push_to_non_default_branch_detects_git_dependency_then_orphans` (real HTTP push to a non-default branch → Code/CargoToml edge with `version_pinned` from `tag`, then a tip-without-declaration push → Orphaned, edge retained) and `git_http::tests::detect_dependencies_on_push_full_extraction_forms` (tree-wide nested `crates/member/Cargo.toml` with manifest path as source_artifact, nested `api/openapi.yaml` Api edge, mcp.json McpToolRef edge, package.json/pyproject git forms, repo-created-after-manifest AC-6, idempotent no-change push), plus the detector unit tests in `api::dependencies::tests` and the reconcile suite.
- The workspace clippy gate (`check-clippy-diff.py`).

## Notes (non-blocking)

- package.json/go.mod/pyproject.toml remain root-file-only enumeration; the task requires tree-wide enumeration only for Cargo.toml and contract files, so this is in scope.
- Every-push detection runs several git subprocesses per push (ls-tree + file reads); a performance cost, not a correctness defect.
- Test semantics note: detection is against the pushed tip, so a push to a branch lacking a declaration orphans an edge created from another branch — exactly the semantics the task text specifies ("an edge from a branch whose declarations vanish on a later push is marked Orphaned, never deleted").
