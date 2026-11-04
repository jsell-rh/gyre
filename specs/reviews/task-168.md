# Review — task-168: Repo lifecycle (archive push rejection, admin repos tab, gate config UI)

Candidate: `d1ae653f7bf4cde79182d264d2d1b0e609a4ad5e` (base `5196f44488fab3a7e1e2fcb25186ee4ea116ffcb`)
Verdict: **NOT approved** — 1 blocking finding.

## What was verified (passes)

### §API Summary — archived push rejection
- `git_info_refs` rejects the `git-receive-pack` advertisement for archived
  repos with a 200 + `ERR push rejected: repository is archived` pkt-line;
  `git_receive_pack` (POST) returns 403 before processing the packfile;
  clone/fetch advertisements still served.
- Focused unit run: 40/40 `git_http::tests` pass, including all three new
  archived tests (`cargo-lib-git_http.log`).
- **Mutation probe**: with both archived checks disabled (`if false && ...`),
  `info_refs_receive_pack_archived_repo_returns_err_pkt_line` and
  `receive_pack_archived_repo_returns_403` both FAIL — the tests detect
  disabled enforcement (source restored to clean afterwards; `git status`
  empty).
- Integration: `push_to_archived_repo_rejected` runs a real `git push`
  against a live server and fails with an "archived" reason
  (`cargo-integration-archived-push.log`, 1 passed).

### §Admin → Workspace Scope → Repos Tab
- `WorkspaceSettings.svelte` gains a `repos` tab (8 tabs total): repo table
  with name, Active/Archived status badge, active-agent count, last
  activity; "+ New Repo" / "Import Repo" forms; repo row navigates to repo
  scope via the new `goToRepo` context. `fmtDate` and `api.workspaceRepos`
  both exist. 114 vitest tests pass across WorkspaceSettings + RepoSettings
  after `npm ci` (`vitest-task168.log`, `npm-ci.log`).

### §Gates (Admin → Repo Scope → Gates)
- `RepoSettings.svelte`: gate list with per-gate enabled toggle, per-gate
  config form (command / persona / approvals / timeout by gate type),
  "+ Add Gate" with type selector (pre-existing), drag-to-reorder
  persisting 1-based positions via the new
  `PUT /api/v1/repos/:id/gates/:gate_id` (`api.updateRepoGate`).
- Backend `update_gate`: repo-scope containment (gate whose repo_id ≠ path
  repo → 404), per-gate-type field validation (command on non-command gate
  → 400), 11/11 `api::gates::tests` pass including position ordering and
  update validation (`cargo-lib-gates.log`).
- Migration `2026-10-08-000056_gate_position` is the next unused sequence
  number; `position` enforced in SQLite, Postgres, and mem store (same
  `(position, created_at)` ordering contract).

### Invariants
`check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`,
`check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
`check-inert-enforcement.sh`, `check-dead-message-kinds.sh`,
`check-scope-literal-defaults.sh`, `check-abac-exempt-handlers.sh`,
`check-mcp-write-tools.sh`, `check-byte-slice-truncation.sh`,
`check-forged-scope-fields.sh`, `check-in-memory-state-stores.sh`,
`check-relative-path-defaults.sh`, `check-lossy-secret-conversion.sh`,
`check-unbounded-external-http.sh`, `check-fail-open-ref-resolution.sh` →
all exit 0 (logs under /tmp/stage/review-evidence/).

## Blocking finding

**F1 (CI-breaking): `check-fabricated-scope-defaults.sh` fails on the
candidate** — `scripts/fabricated-scope-defaults-exemptions.txt:5` retargets
the pre-existing git_http entry to `git_http.rs:1510`, but the exempted
`let ws_id = … unwrap_or_else(|| Id::new("default"))` statement actually sits
at `git_http.rs:1535` on the candidate. The retarget (+15) was correct at
commit `df5b3246`, but merge commit `61947824` pulled in base changes that
shifted the statement another +25 lines; no later commit updated the
exemption. The check (wired into `.pre-commit-config.yaml` and
`scripts/dev-check.sh`) exits 1:

```
FAIL: fabricated "default" tenant/workspace scope identity on lookup failure:
crates/gyre-server/src/git_http.rs:1535: let ws_id = …
```

Reproduction: `bash scripts/check-fabricated-scope-defaults.sh` on the
candidate (exit 1; base `5196f444` exits 0 — see
`check-fabricated-scope-defaults.log`). The "Shipped" notes claim this check
passes on the tree, which is inaccurate at the candidate SHA. Fix is a
one-line retarget of the exemption entry to line 1535 (a line-number update
of a pre-existing entry, not a new entry), but per review rules the source
was not modified here.

## Notes for the verifier
- Full `cargo test --all` was not run here (focused probes only, per review
  scope); the focused Rust runs (lib git_http, lib gates, git_integration
  archived-push) and the two frontend suites all pass.
- The two durable "interrupted assignment" checkpoints are process history
  only; implementation content at the candidate is complete for all three
  coverage sections aside from F1.
