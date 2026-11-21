# Review — task-291 (Spec Links: spec-detail link direction)

Spec: `specs/system/spec-links.md` §Querying the Graph (UI) — "Spec detail page: link section showing all outbound/inbound links with staleness indicators" (gap id `5e761a34ffed5b87f0b401c1`).
Candidate under review: `d688a1eb2426876e9ad9215a5164d33aa0a9ae2a` (branch `pipeline/task-291/59f59d947e7943029c4f32801c6a5942-3`, base `8e2e7458`; product commit `b3ab4778`, notes commit `d688a1eb`).
Verdict: **approved** — no findings. The direction field is emitted by the API exactly as contracted, the UI renders the inbound/outbound distinction it was already coded to render, staleness is unchanged, and the new tests fail on base production code.

## Round 1

Probes run (checkout at candidate `d688a1eb`; working tree carried only the executor's protected-task-text restoration of `specs/tasks/task-291.md`, which is guard-excluded and not part of the product diff; evidence under `/tmp/stage/review-evidence/`):

- Full read of `git diff 8e2e7458..d688a1eb` (5 files: api/specs.rs, DetailPanel.svelte, DetailPanel.test.js, coverage matrix, task file).
- `/tmp/stage/bin/cargo check -p gyre-server -p gyre-cli` → exit 0 (8m16s).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 (4m34s) — the new/extended test code type-checks. Full server test compilation deferred to the verifier per the assignment budget.
- `npx vitest run src/__tests__/DetailPanel.test.js` → 41/41 passed (candidate), including the 3 new "Link direction in spec Links tab" tests.
- **Mutation probe**: base `DetailPanel.svelte` + candidate tests → the two direction tests FAIL (inbound link renders in the outbound column, wrong row labels) while the stale-badge test passes (correct — AC 3 requires that pre-existing behavior unchanged). Candidate file restored byte-identical; re-run 41/41; `git status` clean afterwards.

Verified against the task contract:

- **API emits direction (AC 1).** `SpecLinkResponse` gains `direction: Option<String>` (skip-when-none); `with_direction_relative_to` sets `"outbound"` when `source_path` equals the queried path, else `"inbound"` — a self-link is outbound since source matches first. `get_spec_links` applies it relative to the requested path on the already-correct `source_path == spec || target_path == spec` filter. `get_spec_dependents` (filter: `target_path == spec`) therefore populates `inbound`, `get_spec_dependencies` (filter: `source_path == spec`) populates `outbound` — consistent, and both are exercised by the new `get_spec_dependents_and_dependencies_populate_direction` test. Tenant-wide `get_stale_links`/`get_conflicts` leave it `None` (omitted on the wire) — correct, no queried path applies there.
- **UI renders the distinction (AC 2).** DetailPanel's pre-existing direction-based filtering/labels (unchanged logic) now receive real data; additionally the inbound column/rows now show `source_path` (the counterpart spec) instead of `target_path` (which for an inbound link is the queried spec itself) — a correct, necessary repair. Proven live by the mutation probe above.
- **Staleness unchanged (AC 3).** Status badge path (`link.approval_status ?? link.status`) untouched; the stale-badge test passes on both base and candidate, as required for an "unchanged" criterion.
- **No collateral breakage.** gyre-cli consumes the endpoints as `Vec<serde_json::Value>` and prints selected fields — an additive optional field cannot break it. DetailPanel is the sole consumer of `api.specLinks`. No routes added or changed (no ABAC registry impact), no migrations, no exemption files touched, coverage rows 13/15 updated accurately (the pre-existing graph `?repo=` gap in rows 12/13 is honestly left open as task-290 scope, not silently closed).
- **Test honesty.** The Rust assertions compare the response's `direction` JSON value; on base the key is absent (`Value::Null`) so they fail — not self-confirming. The UI tests fail on base production code (demonstrated), and the one that passes on base asserts exactly the "unchanged" AC.

Verifier must run (not executable within the review build budget; recorded in `/tmp/stage/review-evidence/README.md`):

    cargo test -p gyre-server specs::tests::get_spec_links_returns_outbound_and_inbound
    cargo test -p gyre-server specs::tests::get_spec_links_self_link_is_outbound
    cargo test -p gyre-server specs::tests::get_spec_dependents_and_dependencies_populate_direction
    cargo test -p gyre-server spec_link   (regression: links/stale-links/conflicts suites)
