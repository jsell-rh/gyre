# Review — task-259 (Mirror namespace isolation: mirrors/{forge}/{owner}/{repo})

Spec: `specs/system/source-control.md` §External Repository Mirroring (mirror-namespace-isolation bullet).
Base `61b6080d` → candidate `589f7123`. Product commits: `0bf540ae` (feature + tests), `9057fc8f` (prior-round test fix); CI repair `b58fa463` (`scripts/test_dev_cargo_clean.py`, tied to this task's PR #882 CI failure per durable findings).
Verdict: **approved** — independent probes support every acceptance criterion; no findings.

## Probes run (this round, candidate checkout)

- **Standalone behavioral probe of the verbatim `derive_mirror_namespace` / `validate_mirror_segment`** (copied byte-for-byte from `crates/gyre-server/src/api/repos.rs` @ `589f7123` into `/tmp/review-probe`, `url` crate 2.5.8, `cargo test -p mirror_ns_probe`): **4/4 passed**.
  - Spec shape: `github.com→github`, `gitlab.com→gitlab`, other host → sanitized host segment; `.git`-suffix and bare forms map to the same namespace.
  - Attack battery all rejected (fail closed): raw `..` segment (incl. pre-parse check the URL parser would normalize away), `%2F`/`%2E`/`%2D` encoded separators, embedded `..`, `.` segment, 3-segment paths, empty repo (`/.git`), bare host, backslash variants (`\`, `%5C`), Unicode fullwidth/ellipsis lookalikes, non-https scheme, non-URL.
  - Host spoofs do not collide with the `github` namespace (userinfo `github.com@evil.com` → forge `evil.com`; subdomain `github.com.evil.com` distinct); IPv6 host sanitizes to `[A-Za-z0-9._-]` only.
  - Containment invariant: every accepted URL derives a path that is exactly 3 clean single components under `{repos_root}/{ws}/mirrors/` (each segment charset-validated; `workspace_id` additionally rejects `..`/`/` at the handler top, pre-existing).
- **`cargo check -p gyre-server`** → exit 0 (7m23s, clean). **`cargo check -p gyre-server --tests`** → exit 0 (new test module type-checks).
- **`cd scripts && python3 -m unittest test_dev_cargo_clean`** (CI repair, pinned toolchain cargo 1.99 via `/tmp/stage/bin`) → **2/2 OK**, exit 0. The repair moves the retained-artifact sentinel from the member's own executable directory (which Cargo ≥1.100's `cargo clean --package` now removes — exactly the retained PR #882 failure: `target/debug/build/gyre_cache_probe/<meta>/out/unrelated-dependency.rlib` FileNotFoundError) to a fabricated sibling package build unit; a regressed full `cargo clean` still deletes it, so the test's killing power is preserved. `-p gyre_cache_probe` selects the same single member a bare `cargo test` builds.
- Mechanical gates: `check-relative-path-defaults.sh` OK, `check-byte-slice-truncation.sh` OK, `check-arch.sh` OK, `check-forged-scope-fields.sh` OK.

## Verified against the task contract

- **Namespace isolation is real.** `create_mirror_repo` now joins `repos_root/ws/mirrors/{forge}/{owner}/{repo}.git` from validated single components (`Path::join`, not `format!`), with a `starts_with(mirrors_root)` containment guard before the pre-clone stale-dir cleanup — the cleanup can only ever remove paths under `mirrors/`. Native `create_repo` is untouched (line 176 formula intact; diff hunks confined to `create_mirror_repo` + tests); its name validation still rejects `/`, so it cannot allocate under `mirrors/`.
- **Traversal rejected before any write.** `derive_mirror_namespace` runs before the duplicate check, row creation, cleanup, and clone; violations return `ApiError::InvalidInput` → 400 (mapping confirmed in `api/error.rs`). Test asserts no row and no directory under the workspace root.
- **Duplicate upstream fails cleanly.** Same-URL or same-derived-path (different `.git` spelling) in the same workspace → `ApiError::Conflict` → 409 before `repos.create`; test asserts the first mirror's row/path intact and exactly one row.
- **Downstream follows `Repository.path` unchanged.** `mirror_sync::run_once` filters `is_mirror` and fetches `repo.path` (legacy flat-path rows keep syncing — proven by `legacy_mirror_rows_sync_from_persisted_path` with real `git clone --mirror`/`git fetch` storage); receive-pack mirror read-only (git_http.rs:626) and smart-HTTP slug/name resolution are untouched and path-agnostic.
- **Tests are real, not self-confirming.** The e2e test uses a `GitOpsPort` double whose `clone_mirror`/`fetch_mirror` perform *real* `git` operations from a local fixture, then asserts the on-disk `HEAD` at the derived path, removal of a stale dir *inside* `mirrors/`, and byte-intact native-repo refs and pre-isolation flat dir adjacent to the escape target. Prior-round F1 (same `(ws,name)` in the forge-hosts test → mem-adapter uniqueness 500) is fixed in `9057fc8f`.
- **Docs/coverage accurate.** `docs/api-reference.md` mirror row and coverage row 13 (`implemented`, task-259, with source evidence) match the shipped behavior.

## Non-blocking observations (recorded, no action required)

- TOCTOU window between the duplicate-path check and `repos.create` (no unique constraint on `path`); sequential contract satisfied, consistent with existing handler patterns.
- URLs with a query/fragment (e.g. `?q=1`) are accepted when path segments are clean — namespace-safe (query never reaches the path); the clone itself may fail, as before.
- Non-default ports are ignored in forge identity (`github.com:8443` → `github`) — a naming nuance, not a safety issue (host must literally be `github.com` to collide).

## Deferred to the independent verifier (transport/size-bound in this sandbox)

- `cargo test --all` (workspace suite; AC 6).
- Focused: `cargo test -p gyre-server mirror` — `create_mirror_repo_returns_201`, `create_mirror_repo_maps_forge_hosts_into_namespace`, `create_mirror_repo_rejects_traversal_urls`, `create_mirror_repo_rejects_duplicate_upstream_url`, `create_mirror_repo_isolates_storage_and_serves_smart_http`, `legacy_mirror_rows_sync_from_persisted_path`, `derive_mirror_namespace_maps_forge_hosts`, `derive_mirror_namespace_rejects_unsafe_urls`.
- `python3 -m unittest discover -s scripts -p 'test_dev*.py'` (dev-controller CI gate; Cargo ≥1.100 layout is what the repair targets).
- `bash scripts/check-task-commit-attribution.sh` (timed out here — needs full history fetch; the candidate's task frontmatter records both product-surface commits `0bf540ae`, `9057fc8f`; `b58fa463` touches `scripts/` only, not product surface).
- Full CI on the merged candidate (PR #882 class).

Evidence: `/tmp/stage/review-evidence/` (probe source, command output, exit codes, mechanical-check logs).
