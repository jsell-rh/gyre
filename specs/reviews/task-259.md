# Review — task-259 (Mirror namespace isolation: mirrors/{forge}/{owner}/{repo})

Spec: `specs/system/source-control.md` §External Repository Mirroring
(mirror-namespace-isolation bullet).
Candidate under review: `0c941afffd04123229e084a8fcb34359299929c4`
(base `25497b9e3cc16c7e405c641870ff3c3b1905cf6b`; product commit `0bf540ae`,
docs `5678d534`, `0c941aff`).
Verdict: **needs-revision** — one material finding (F1: a new test that
deterministically fails at runtime; the candidate never executed the test
suite, which its own "Deferred verification" section acknowledges).
Everything else verified clean and correct.

## Round 1

### Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → exit 0 (12m28s cold).
- `cargo check -p gyre-server --tests` → exit 0, no warnings (4m22s).
- Standalone `url` 2.5.8 parser-semantics probe over 18 URL shapes →
  confirms every traversal/percent-encoding vector the isolation relies on
  (dot-segment normalization, literal `%2F`, IPv6 `host_str` brackets,
  backslash conversion, empty-segment preservation).
- **Verbatim-logic probe**: the candidate's `derive_mirror_namespace` /
  `validate_mirror_namespace` segment validation extracted byte-identical
  from candidate repos.rs:621-721 (diff-verified) and executed with the
  handler's exact path-join + `starts_with(mirrors_root)` containment
  guard → **all assertions pass**, including the acceptance path shape
  `{repos_root}/{ws}/mirrors/github/octocat/Hello-World.git`, forge mapping
  (github/gitlab/sanitized host, IPv6 → `---1-`), 16 unsafe-URL vectors all
  rejected before any write, and `.git`-suffix spellings mapping to the same
  path (the 409-by-path-equality case).
- `scripts/check-arch.sh`, `check-relative-path-defaults.sh`,
  `check-byte-slice-truncation.sh` → OK.
- `scripts/check-task-commit-attribution.sh` in a scratch worktree at
  candidate HEAD → OK (the working-tree failure is the controller's reset of
  specs/tasks/task-259.md, not the candidate).

gyre-server test execution is blocked by the review build wrapper (4 GiB
budget) — recorded for the verifier; see evidence README for the exact
command list.

### Verified working (no findings)

- **Namespace isolation is real and sound.** `create_mirror_repo` now stores
  mirrors at `{repos_root}/{ws}/mirrors/{forge}/{owner}/{repo}.git`, derived
  from the URL (github.com→github, gitlab.com→gitlab, other https hosts
  sanitized; owner/repo from exactly two path segments, optional `.git`
  stripped). Every segment is validated (non-empty; no `.`/`..`; charset
  limited to alphanumeric/`-`/`_`/`.`) BEFORE any row insert or filesystem
  write; the raw-URL `..` pre-check closes the URL-parser dot-segment
  normalization hole, and the charset check closes percent-encoded
  separators (`%2F` stays literal in `url`'s path). The pre-clone stale-dir
  cleanup is additionally guarded by `repo_path.starts_with(mirrors_root)`
  (component-wise), so it can only ever remove paths under
  `{repos_root}/{ws}/mirrors/`. Unsafe URLs → 400 (`InvalidInput`),
  duplicates → 409 (`Conflict`) — both mappings confirmed in error.rs.
- **Native namespace untouched.** `create_repo` (repos.rs:176) is
  byte-for-byte unchanged; its name validation still rejects `/` and `..`,
  so it can never allocate under `mirrors/`.
- **Downstream follows `Repository.path` unchanged**: mirror_sync.rs:22
  `fetch_mirror(&repo.path)`, `sync_mirror` handler, git_http smart-HTTP
  (repo.path), receive-pack read-only enforcement. No other mirror-path
  construction exists (grep sweep of `repos_root` uses).
- **Legacy rows**: no migration; sync and serving use the persisted path —
  covered by `legacy_mirror_rows_sync_from_persisted_path` (real git clone
  at the flat path, real `git fetch --all` via the recording double).
- **Tests are substantive, not self-confirming**: the e2e test performs a
  real `git clone --mirror` into the derived namespace, proves the stale-dir
  cleanup removes only the in-namespace stale dir while a pre-isolation flat
  dir and an adjacent native repo dir (with refs marker) stay untouched, and
  serves smart-HTTP `/git/{slug}/{name}/info/refs` → 200 through the real
  route (same path pattern as lib.rs:1020; `api_router()` registers no
  `/git/` routes, so the test's `.route(...)` addition cannot panic).
- **Docs/coverage updates are accurate**: source-control.md row 13 moved to
  `implemented` with task-259 attribution and a factual note (no
  independent-verification overclaim); api-reference.md describes the new
  request shape and error codes.

### Findings

**F1 (blocking, test): `create_mirror_repo_maps_forge_hosts_into_namespace`
deterministically fails.** The test POSTs `/api/v1/repos/mirror` twice with
the same `(workspace_id: "ws-mirror-hosts", name: "m")` for its two URL
cases and asserts 201 for both. The second request passes the new
duplicate-upstream guard (different URL and derived path), then hits
`MemRepoRepository::create`'s in-code (workspace_id, name) uniqueness guard
(mem.rs:742-763, the port contract) → `anyhow::bail!` → `From<anyhow::Error>`
→ `ApiError::Internal` → HTTP 500 (error.rs:65-67, 96-100). Expected 201,
got 500 → assertion failure on
`url https://git.example.com/team/tooling`. The candidate's own task file
records that the gyre-server test binary could not be built in the
implementation sandbox ("Deferred verification"), so this was never
observed. This violates the acceptance criterion "cargo test --all passes".
Production behavior is correct (a same-name mirror must be rejected; the
handler returning 500 rather than 409 for name collisions is pre-existing
`create_repo` behavior, out of scope). Fix: distinct `name` (or workspace)
per loop case. Full proof and verifier reproduction command:
`/tmp/stage/review-evidence/F1-maps-forge-hosts-test-fails.md`.

### Non-blocking observations (recorded, not findings)

- Concurrent duplicate mirror creates have a check-then-insert window (no
  DB unique constraint on path); the task explicitly keeps the
  `(workspace_id, name)` identity rule, and the sequential guard satisfies
  the acceptance criteria. Inherent to the store port, pre-existing pattern.
- URLs with >2 path segments (e.g. GitLab subgroups
  `group/subgroup/repo.git`) are now rejected 400 — required by the task's
  exact `/{owner}/{repo}` shape and its "embedded `/`" rejection criterion;
  note it narrows mirrorable URL shapes.
- The raw `url.contains("..")` gate also rejects `..` occurring in a query
  string or fragment (irrelevant to the path) — conservative, fails closed.
- Generated `web/dist/`: `cargo check` ran build.rs which rebuilt web
  assets; `git status` shows no tracked `web/dist/` modification — no
  generated-output observation for the verifier beyond the standard rebuild.

### Verifier must run (deferred by build wrapper)

`cargo test -p gyre-server --lib` for: `create_mirror_repo_returns_201`,
`create_mirror_repo_maps_forge_hosts_into_namespace` (expected FAIL per
F1), `create_mirror_repo_rejects_traversal_urls`,
`create_mirror_repo_rejects_duplicate_upstream_url`,
`create_mirror_repo_isolates_storage_and_serves_smart_http`,
`legacy_mirror_rows_sync_from_persisted_path`, `derive_mirror_namespace*`;
then `cargo test --all` (acceptance criterion).
