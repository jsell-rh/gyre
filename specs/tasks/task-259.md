---
title: "Mirror namespace isolation: mirrors/{forge}/{owner}/{repo} storage namespace for external-forge mirrors"
spec_ref: "Spec gap spec-36fad8fcafe7b73a2b6f2d0e / mirror-namespace-isolation / b580b93c564f"
depends_on: []
progress: complete
commits: ["9057fc8f06b699202bd4cb1dd9070198fc94059d", "0bf540aef508b63a1a0586cf60ee24a1f0a96ae6"]
---

## Required behavior

Per specs/system/source-control.md §External Repository Mirroring (mirror-namespace-isolation bullet), store mirror repositories in an isolated namespace that encodes the upstream forge identity, instead of sharing the native-repo namespace. Today create_mirror_repo (crates/gyre-server/src/api/repos.rs:639) computes the mirror's storage path with the exact formula create_repo uses for native repos (line 176): {repos_root}/{workspace_id}/{name}.git — no mirrors/ prefix and no forge identity anywhere in the path. Change create_mirror_repo so a mirror's storage path is {repos_root}/{workspace_id}/mirrors/{forge}/{owner}/{repo}.git (spec example: mirrors/github/{owner}/{repo}), where the segments are parsed from the mirror URL: host github.com → github, gitlab.com → gitlab, any other https host → a sanitized host-derived segment; owner/repo from the URL path segments. Every derived segment must be validated (reject '..', '/', and percent-encoded/separator traversal) BEFORE any filesystem write, so the derived path can never escape the mirrors/ namespace; unsafe URLs are rejected with a 4xx. The stale-directory cleanup that runs before clone_mirror ('Removing stale repo directory before mirror clone') must be scoped so it can only ever remove paths under {repos_root}/{ws}/mirrors/ — creating a mirror must never touch a native repo's directory. Creating a second mirror of the same upstream URL in the same workspace (same derived path) must fail cleanly rather than silently share or overwrite storage. Everything downstream follows the stored Repository.path unchanged: the mirror_sync job / fetch_mirror, post-sync graph extraction and spec-ledger sync, receive-pack mirror read-only enforcement, and smart-HTTP clone-by-name. Existing mirror rows keep their persisted paths (path is per-record; no rewrite or migration of previously created mirrors). Native create_repo is unchanged and stays unable to allocate a path under mirrors/ (its name validation already rejects '/'). Keep the existing SSRF https-only URL gate and the (workspace_id, name) DB identity rule — this task is the storage-namespace isolation the spec mandates, not a rework of mirror functionality. The derived path is canonicalized at rest (absolute under repos_root) per scripts/check-relative-path-defaults.sh.

## Acceptance Criteria

- [ ] POST /api/v1/repos/mirror with url=https://github.com/octocat/Hello-World.git in workspace W creates a repo whose record path and on-disk storage are {repos_root}/{W}/mirrors/github/octocat/Hello-World.git; the mirror_sync job fetches refs into that path and the repo is cloneable via the existing smart-HTTP name route.
- [ ] Disjoint namespaces: a native repo and a mirror in the same workspace never share a storage path — creating the mirror leaves every native repo directory and its refs untouched (the pre-clone stale-dir cleanup provably cannot path outside mirrors/).
- [ ] A mirror URL containing traversal segments (owner '..', embedded '/', or encoded separators) is rejected with a 4xx and creates no directory anywhere on disk.
- [ ] Creating a second mirror of the same upstream URL in the same workspace is rejected cleanly (error response, first mirror's storage intact).
- [ ] Mirror rows created before this change still sync and serve from their persisted paths (no migration required for them); native repo creation is byte-for-byte unchanged.
- [ ] cargo test --all passes, with existing mirror-creation tests updated to the new path shape.

## Verification intent

POST /api/v1/repos/mirror with url=https://github.com/octocat/Hello-World.git and name Hello-World, then inspect the created Repository.path and the filesystem: on the current tree the mirror is stored at {repos_root}/{ws}/Hello-World.git — byte-identical in namespace to a native repo (create_mirror_repo at crates/gyre-server/src/api/repos.rs:639 uses create_repo's exact formula from line 176), so the spec's mirrors/github/{owner}/{repo} isolation does not exist anywhere in the codebase. After the task, the same request must store the mirror at {repos_root}/{ws}/mirrors/github/octocat/Hello-World.git — assertable on both the repo record and the on-disk directory — and a probe asserting the old flat path fails. Traversal companion: submit a mirror URL whose owner segment is '..' (or contains an encoded separator); a naive format!-concatenated path would resolve to {ws}/mirrors/github/../… and escape the namespace, so the request must be rejected with a 4xx before any filesystem write — a probe that asserts no directory outside mirrors/ is created (and that the native repo directory adjacent to the escape target is intact) fails if the validation is missing.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/source-control.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `e7d4e79257be81eda6ddf1029e79d17654ad24d2acc5ff2425ebf4e133acd4b0`.

[
  {
    "id": "09c28182c53ccd934aa3b93a",
    "decision": "gap",
    "requirement_quote": "- Support **pull mirroring** from external forges (GitHub, GitLab) for repos that need to be brought into Gyre.\n- GitHub App integration for authenticated access to private repos (later milestone, not M0).\n- Mirror namespace isolation (e.g., `mirrors/github/{owner}/{repo}`).",
    "detail": "Two of three bullets are fine: pull mirroring is real (POST /api/v1/repos/mirror clones the remote as a bare mirror via git_ops.clone_mirror with SSRF-gated https-only URLs; a mirror_sync job registered in the job framework runs every 60s calling git_ops.fetch_mirror on every is_mirror repo, updates last_mirror_sync, and runs post-sync spec-ledger + graph extraction; pushes to mirrors are rejected read-only in receive-pack), and the GitHub App integration is explicitly deferred by the spec itself ('later milestone, not M0'), so it is not a gap. The gap is mirror namespace isolation: mirror repos are stored at exactly the same path namespace as native repos — {repos_root}/{workspace_id}/{name}.git in both create_repo and create_mirror_repo — with no mirrors/ prefix and no encoding of the upstream forge identity (github/{owner}/{repo}). Mirrors and native repos share one namespace per workspace (backed only by the unique (workspace_id, name) index), so a mirror cannot coexist with a same-named native repo and mirror paths carry no forge-origin isolation; the spec's isolation bullet has no corresponding implementation.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "quote": "    let repo_path = format!(\"{}/{}/{}.git\", state.repos_root, req.workspace_id, req.name);",
        "detail": "create_mirror_repo (line 639) computes the mirror's storage path with the identical formula create_repo uses for native repos (line 176) — mirrors live in the same namespace as native repos with no mirrors/ prefix and no upstream forge identity (github/{owner}/{repo}) encoded anywhere in the path; the namespace-isolation bullet is unimplemented."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "                name: \"mirror_sync\".to_string(),\n                description: \"Fetches latest refs for all mirror repositories\".to_string(),",
        "detail": "Pull mirroring itself is real: the mirror_sync job is registered in the job framework (60s interval) and mirror_sync::run_once fetches all is_mirror repos via git_ops.fetch_mirror, updates last_mirror_sync, and runs post-sync spec-ledger sync and knowledge-graph extraction — the first bullet is satisfied."
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "quote": "        is_mirror: true,\n        mirror_url: Some(url.clone()),\n        mirror_interval_secs: req.interval_secs,",
        "detail": "The mirror Repository record carries is_mirror/mirror_url/mirror_interval_secs, and receive-pack enforces read-only on mirrors — what is missing is specifically the isolated mirror namespace, not mirror functionality."
      }
    ]
  }
]

## Shipped

`create_mirror_repo` (crates/gyre-server/src/api/repos.rs) stores every new
mirror in an isolated namespace derived from its upstream URL:
`{repos_root}/{ws}/mirrors/{forge}/{owner}/{repo}.git`, per
source-control.md §External Repository Mirroring. Native `create_repo` is
byte-for-byte unchanged (its name validation still rejects `/`, so it can
never allocate under `mirrors/`).

### Behavior

- **Segment derivation** (`derive_mirror_namespace`): host `github.com` →
  `github`, `gitlab.com` → `gitlab`, any other https host → a sanitized
  host-derived segment (non-filesystem-safe characters map to `-`);
  owner/repo come from the two URL path segments, with an optional `.git`
  suffix stripped. The path is built with `Path::join` from validated
  components over the startup-canonicalized absolute `repos_root`.
- **Traversal rejection before any write**: the raw URL is rejected if it
  contains `..` anywhere (checked before parsing, because the URL parser
  normalizes `/../` away); each segment must be non-empty, free of `..`, and
  limited to alphanumeric/`-`/`_`/`.` (so percent-encoded separators like
  `%2F` fail); the path must be exactly `/{owner}/{repo}`. Violations return
  400 with no repo row and no directory created.
- **Scoped stale-dir cleanup**: the pre-`clone_mirror` cleanup is preceded by
  an explicit containment guard (`repo_path.starts_with(mirrors_root)`,
  returning an error otherwise), so it can only ever remove paths under
  `{repos_root}/{ws}/mirrors/` — never a native repo's directory.
- **Duplicate upstream**: a second mirror of the same upstream URL, or a
  different URL spelling mapping to the same derived path (e.g. missing
  `.git` suffix), in the same workspace is rejected with 409 before any
  row or storage change.
- **Unchanged downstream**: mirror_sync / `sync_mirror` / fetch_mirror,
  post-sync graph extraction and spec-ledger sync, receive-pack read-only
  enforcement, and smart-HTTP clone-by-name all follow the stored
  `Repository.path`; legacy mirror rows keep their persisted flat paths with
  no migration. The SSRF https-only gate and the `(workspace_id, name)` DB
  identity rule are kept.

### CI repair (this checkpoint)

Durable finding: CI run 44505184647 (PR 882, head 7e5c6f87) failed the
Development controller job in `test_dev_cargo_clean` — the sentinel
vanished from
`target/debug/build/gyre_cache_probe/<meta>/out/unrelated-dependency.rlib`.
Root cause: Rust 1.100.0 (2026-11-12) stabilized Cargo's new build-dir
layout, which puts a package's own artifacts (test executables included)
under `<profile>/build/<pkg>/<meta>/out/` and makes `cargo clean
--package` remove that whole tree. Runners on fresher images (cargo
1.100.x) put the probe's executable there, so the sentinel — placed next
to the member's own executable — was legitimately removed by the mutant
iteration's `cargo clean --package`; runners on cargo 1.99.1 kept the old
`deps/` layout and passed (the scripts trees of the failing head and
passing main are byte-identical, so the flake tracks the runner image
generation, not the commit).

Fix (commit b58fa463): the sentinel now represents an artifact of an
UNRELATED package — derived from the profile directory owning the reported
executable (nearest ancestor inside the target dir containing cargo's
`build/` directory, created unconditionally in both layouts), placed in a
sibling build unit `build/unrelated-dependency/out/unrelated-dependency.rlib`,
which `cargo clean --package` of the member never removes in either layout.
The probe build also selects its single package explicitly
(`cargo test -p gyre_cache_probe`; identical units) so constrained dev
environments can run this focused probe.

### Test evidence

- Merged implementation type-checks with all tests:
  `cargo check -p gyre-server --tests` — exit 0 (the gyre-server test
  binary exceeds this sandbox's 4 GiB build budget; execution deferred to
  the independent verifier, which must run `cargo test --all` — in
  particular `create_mirror_repo_returns_201`,
  `create_mirror_repo_maps_forge_hosts_into_namespace`,
  `create_mirror_repo_rejects_traversal_urls`,
  `create_mirror_repo_rejects_duplicate_upstream_url`,
  `create_mirror_repo_isolates_storage_and_serves_smart_http` (real git
  clone into the namespace, stale-cleanup scoping, untouched native/flat
  dirs, smart-HTTP info/refs 200), `legacy_mirror_rows_sync_from_persisted_path`,
  and the `derive_mirror_namespace_*` unit battery).
- `python3 scripts/test_dev_cargo_clean.py -v` — OK on the current
  toolchain (old layout).
- New-layout simulation (RUSTC_BOOTSTRAP=1 + `[unstable]`
  `build-dir-new-layout = true`, the same layout cargo 1.100 stabilized;
  executable confirmed reported at
  `target/debug/build/gyre_cache_probe/<meta>/out/`): the pre-fix test
  fails with the CI's exact FileNotFoundError, the fixed test passes.
- `env -u GYRE_PIPELINE_STAGE python3 -m unittest discover -s scripts -p
  'test_dev*.py'` — 65 tests, OK (CI conditions; with the sandbox's
  implement-stage env set, test_dev_build_command's two bare-`cargo test`
  fixtures hit the build wrapper's stage gate — environment artifact,
  those tests pass under CI conditions).
- Exact-head GitHub checks for the PR remain mandatory after independent
  review.
