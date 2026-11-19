# Review — task-259: Mirror namespace isolation

Candidate: `eb2c2ddac9e05d7090e8a222f1ad2ee5e3411d4b` (base `25497b9e3cc16c7e405c641870ff3c3b1905cf6b`)
Round: 2 (prior round rejected on F1, repaired in 9057fc8f)
Verdict: **approved** — no findings.

## Scope inspected

`git diff 25497b9e..eb2c2dda`: production change confined to
`crates/gyre-server/src/api/repos.rs` (`derive_mirror_namespace`,
`validate_mirror_segment`, `create_mirror_repo`); plus new tests, the
`docs/api-reference.md` mirror-endpoint row, the source-control.md §13
coverage row (`implemented`, accurate, not `verified`), and task notes.
Native `create_repo` is untouched (all removed lines are in the mirror
handler/tests); its name validation still rejects `/`, so it cannot allocate
under `mirrors/`.

## What was verified

1. **Namespace derivation** — host `github.com`→`github`, `gitlab.com`→
   `gitlab`, other https hosts sanitized; owner/repo from exactly two URL
   path segments with optional `.git` stripped. Proven by a standalone
   verbatim-logic probe (functions extracted byte-for-byte, `url = "=2.5.8"`
   matching the lockfile): all mapping, fail-closed and path-containment
   assertions pass (`/tmp/stage/review-evidence/nsprobe-output.txt`).
2. **Traversal rejection before any write** — raw `..` rejected pre-parse
   (the URL parser would normalize `/../` away); segment charset whitelist
   (alphanumeric/`-`/`_`/`.`) rejects percent-encoded separators (`%2F`,
   `%2e%2e`, `%00`) and Unicode; exactly-2-segment and non-empty rules;
   `ApiError::InvalidInput` → 400. Derivation runs before the DB create and
   before any filesystem op.
3. **Stale-dir cleanup containment** — explicit
   `repo_path.starts_with(mirrors_root)` fail-closed guard plus validated
   single-component joins; the cleanup can only remove under
   `{repos_root}/{ws}/mirrors/`. The e2e test proves a native repo dir (with
   refs marker) and the pre-isolation flat dir are untouched while a stale
   dir inside `mirrors/` is removed.
4. **Duplicate upstream** — pre-create guard on same `mirror_url` or same
   derived path → `ApiError::Conflict` → 409 (error.rs:62); first mirror's
   row and storage intact (test asserts single row + exact path).
5. **Legacy rows** — `sync_mirror` fetches the persisted `repo.path`; no
   migration. Legacy test clones a real mirror at a flat path and proves
   `fetch_mirror` receives that path unchanged.
6. **Downstream unchanged** — mirror_sync job, receive-pack read-only,
   smart-HTTP clone-by-name all follow `Repository.path`; git_http.rs is not
   in the diff.
7. **F1 repair (round-1 finding)** — 9057fc8f gives each forge-host test case
   its own mirror name, so the second POST no longer hits the mem adapter's
   `(workspace_id, name)` uniqueness bail (500) and returns the asserted 201;
   verified in the committed source.
8. **Mechanical gates** — `cargo check -p gyre-server --tests` exit 0,
   `cargo fmt --check` clean; arch, mem-port-contracts,
   relative-path-defaults, byte-slice-truncation, in-memory-state-stores,
   forged-scope-fields, inert-enforcement, lossy-secret-conversion,
   scope-literal-defaults, abac-route-registry all pass. Commit attribution:
   the candidate's frontmatter lists both product-surface commits
   (0bf540ae, 9057fc8f); the working-tree script failure observed during
   review is the controller's uncommitted review-round reset of the task
   frontmatter, not candidate content.

## Deferred to the verifier (4 GiB build budget)

- `cargo test -p gyre-server --lib create_mirror_repo`
- `cargo test -p gyre-server --lib derive_mirror_namespace`
- `cargo test --all`
- `cargo test --test git_integration mirror_endpoint_with_full_middleware_returns_201`

## Non-blocking observations

- TOCTOU: concurrent duplicate-upstream POSTs could both pass the linear
  pre-create guard (no DB constraint on `mirror_url`/path). Sequential
  contract per the acceptance criteria is satisfied; same race class as the
  pre-existing `create_repo`. Not blocking.
