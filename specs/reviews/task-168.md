# Review — task-168 (Repo lifecycle — archive push rejection, admin repos tab, gate config UI)

Spec: `specs/system/repo-lifecycle.md` §"Admin → Workspace Scope → Repos Tab", §"Gates (Admin → Repo Scope → Gates)", §"6. Domain Changes — API Summary".
Candidate under review: `15ebaff019ac411155c235d7124dcc642cbe3d3b` (assigned base `1d9547de`).
Verdict: **needs-revision** (one finding).

## Round 1

Probes (evidence under `/tmp/stage/review-evidence/`; Rust probes via `/tmp/stage/bin/cargo`, tree restored clean at `15ebaff0` afterwards):

- `cargo test -p gyre-server --lib archived_repo` → **3 passed** (info/refs ERR pkt-line, receive-pack 403, upload-pack still advertises for archived repos) — `cargo-lib-archived.log`.
- `cargo test -p gyre-server --test git_integration push_to_archived_repo_rejected` → **1 passed** — a real `git clone` + `git push` against a live server; push rejected with an "archived" reason — `cargo-integration-archived-push.log`.
- **Mutation probe** (`mutation-probe-archived-push.md`): disabled both archived checks in `git_http.rs` (`if false && ...`); the integration test **fails** ("push to archived repo should be rejected, but it succeeded"). The test genuinely detects the production behavior — not self-confirming. Source restored.
- `cargo test -p gyre-server --lib api::gates` → **11 passed** (position ordering, update validation, per-type field rejection, 404) — `cargo-lib-gates.log`.
- `cd web && npm ci && npx vitest run RepoSettings/WorkspaceSettings` → **114 passed** — `vitest-task168.log`.
- Mechanical invariants (logs saved): `check-fabricated-scope-defaults` (frozen count 4; the deleted `git_http.rs` entry is genuinely fixed by the base merge — `resolve_repo_by_slug`/skip-and-log verified in source), `check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`, `check-abac-route-registry`, `check-arch`, `check-task-commit-attribution` — all **pass**; `check-rustfmt-diff.py 1d9547de` → changed lines clean (13 Rust files).

Verified working (no findings):

- **Archived push rejection (§API Summary)**: `git_info_refs` rejects the `git-receive-pack` advertisement with a 200 whose pkt-line stream carries `ERR push rejected: repository is archived` — the correct git-smart-HTTP convention (a bare 403 body is never shown by `git push`); `git_receive_pack` keeps a defense-in-depth 403 before the packfile; clone/fetch on archived repos still works (read-only, not invisible). The unit test pins the exact wire framing (`001f# service=git-receive-pack\n0000` + ERR pkt-line). Auth equivalence preserved: `resolve_repo_by_slug` is the same tenant-scoped lookup `resolve_repo_path_by_slug` already wrapped.
- **Gates API (§Gates)**: new `PUT /api/v1/repos/:id/gates/:gate_id` (`api/mod.rs` adds `put` to the already-ABAC-registered route; PUT→"write" action) with per-gate-type field validation (command only for command gates, persona only for agent gates, non-empty name), repo mismatch → 404 — mirrors the pre-existing `delete_gate` handler pattern. Gate ordering via `position` enforced identically in SQLite, Postgres, and the mem store (`(position, created_at)`); migration `2026-10-08-000057_gate_position` uses the next free sequence number and portable SQL. The two `000056` dirs present in the tree both come from the base (pre-existing on main), not this candidate.
- **UI source (§Repos Tab, §Gates)**: `WorkspaceSettings.svelte` repos tab (name/status badge/agent count/last activity, "+ New Repo", "Import Repo" with mirror form, click-through via `goToRepo` context) and `RepoSettings.svelte` gates panel (enabled/disabled toggle, per-type config forms, add gate, drag-to-reorder persisting 1-based positions with server reload on failure) are real and behaviorally tested. Mapping the toggle to `required` (blocking vs advisory) and keeping parallel gate execution match `agent-gates.md`, which `repo-lifecycle.md` §Gates defers to for execution semantics.

Findings:

- **F1 (major): committed `web/dist` is stale — the served SPA bundle contains none of the task-168 UI.** `git diff 1d9547de 15ebaff0 -- web/dist/` is **empty**: the candidate's committed dist is byte-identical to the assigned base, meaning the merge-resolution commit `219780c` resolved the `1d9547de` merge's web/dist conflicts by taking the base side and silently reverted the dist rebuild done at `4443b66`. Grep of the committed bundle (`index-BfIDQm4g.js`) for `ws-repos-tab`, `gate-toggle`, `gate-edit-form`, `updateRepoGate` → all 0; a fresh `npm ci && npm run build` from the candidate's own `web/src` produces a different bundle (`index-BQYVRA1x.js`) containing all of them (`stale-dist-probe.log`). `gyre-server` embeds `web/dist` via RustEmbed (`crates/gyre-server/src/spa.rs:5-6`), so the documented `SKIP_WEB_BUILD=1 cargo build -p gyre-server` deployment (AGENTS.md quick start) ships a dashboard **without the workspace Repos tab and without the repo Gates panel** — the UI acceptance criteria are not delivered in the committed artifact, and the shipped notes' claim that dist was rebuilt from the merged source is false at the final head. This is precisely the defect class the task's own repair round `4443b66` fixed ("Stale web/dist ... Rebuilt dist from the merged source"); the final merge reintroduced it. The recorded verification failure at `8309cc9b` (checks.sh exit 1, durable findings) is consistent with this class of check. Remedy: rebuild `web/dist` from the merged `web/src` and commit it (same one-command fix as `4443b66` item 2), then re-verify the committed bundle contains the task-168 UI strings.

Minor (not blocking, recorded for completeness):

- The base itself carries two migrations sharing version `20261008000056`/`20261010000056` sequence number `000056` (`spec_assertion_results`, `meta_spec_approved_content_hash`) — pre-existing on main, out of this candidate's scope, but one of the two silently never runs under Diesel's one-migration-per-version rule; worth a main-side fix.
- `update_gate` cannot clear `timeout_secs` back to the system default (no `None` representation in `UpdateGateRequest`) — minor API asymmetry, not specced.
