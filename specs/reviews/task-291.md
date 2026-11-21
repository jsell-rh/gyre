# Review — task-291 (Spec Links: API emits link direction; UI renders inbound vs outbound)

Spec: `specs/system/spec-links.md` §Querying the Graph (UI) — "Spec detail page: link section showing all outbound/inbound links with staleness indicators".
Candidate under review: `b749f865` (branch `pipeline/task-291/9f14d2ffaaf04496b3774647f054090a-1`, base `8e2e7458`; product commit `b3ab4778`, rustfmt-only repair `9fda212d`; `b62ba310`/`b749f865` touch only the task file).
Verdict: **approved** — no findings. The gap is closed at the source exactly as the task requires, with regression-killing tests on both sides of the API boundary.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `rustfmt --edition 2021 --check crates/gyre-server/src/api/specs.rs` → exit 0 (the prior verification failure is repaired; `git diff b3ab4778..9fda212d` confirmed formatting-only).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (cold, 9m49s) — type-checks the new field, helper, handlers, and the new test code.
- `cd web && npm ci && npx vitest run src/__tests__/DetailPanel.test.js` → exit 0, 41/41 passed.
- **Negative control:** base `DetailPanel.svelte` (8e2e7458) + the new tests → 2/3 fail (column split, row label/target); the staleness-badge test passes on base, correctly, since staleness was never broken. Candidate source restored bit-identical afterwards. The UI tests kill the base rendering defect rather than merely confirming new code.
- Rust tests were not executed here (gyre-server test-binary link exceeds the review budget); deferred to the verifier — see below. They are genuine regression killers by construction: base `SpecLinkResponse` emits no `direction` key, so `obj["direction"]` is `Null` and `assert_eq!(Null, "inbound")` fails.
- Incidental: `npm ci` runs the project `prepare` script which regenerated `web/dist/`; restored to the committed state. dist is verifier-owned generated output (pure observation, not a finding).

## Verified working

- **AC 1 (API emits direction).** `SpecLinkResponse` gains `direction: Option<String>` (`#[serde(skip_serializing_if = "Option::is_none")]`, so tenant-wide stale-links/conflicts listings are unchanged — no queried path applies there). `with_direction_relative_to(queried_path)` classifies `source_path == queried → "outbound"`, else `"inbound"`; a self-link is outbound. Applied in `get_spec_links` (filter guarantees source or target equals the queried path, so the classification is total), `get_spec_dependents` (target-side links → inbound), and `get_spec_dependencies` (source-side → outbound) — exactly the task's required semantics. Rust tests: extended `get_spec_links_returns_outbound_and_inbound` (b with inbound a→b and outbound b→c, asserting `"inbound"`/`"outbound"` per object), new `get_spec_links_self_link_is_outbound`, new `get_spec_dependents_and_dependencies_populate_direction` (dependents → inbound, dependencies → outbound). All drive the real `api_router()` via oneshot against real in-memory state, not handler-internal calls.
- **AC 2 (UI renders the distinction).** `DetailPanel.svelte` keeps its existing direction-based column filter and `<- from`/`-> to` labels (now fed real data) and additionally shows the *counterpart* spec for inbound links: the inbound diagram column and inbound list rows use `source_path` (base showed `target_path`, i.e. the queried spec itself). The three new tests assert the inbound link's source renders in `.link-column-inbound` (and not the outbound column), the `<- from`/`-> to` row labels, and pass on candidate / fail on base.
- **AC 3 (staleness unchanged).** The status-badge code (`link.approval_status ?? link.status` → Badge) is untouched by the diff; the stale-badge test passes on both base and candidate, i.e. no regression.
- **No collateral damage.** The field is additive; `gyre-cli` parses link responses as `Vec<serde_json::Value>` and no `deny_unknown_fields` exists in CLI/server, so no strict consumer breaks. No routes, migrations, ABAC registry entries, or MCP tools added, so the mechanical invariants are unaffected. Coverage rows 13/15 updated with concrete source evidence and honestly keep the pre-existing, out-of-scope `?repo=` graph-filter gap (task-290) open. Commit attribution lists both task-labeled product commits (`b3ab4778`, `9fda212d`).

## Verifier must run

- `cargo test -p gyre-server get_spec_links_returns_outbound_and_inbound`
- `cargo test -p gyre-server get_spec_links_self_link_is_outbound`
- `cargo test -p gyre-server get_spec_dependents_and_dependencies_populate_direction`
- `cd web && npm ci && npx vitest run` (full frontend suite)
