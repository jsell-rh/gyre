# Review — task-083 (Canonical Navigation — Scope-Aware Content Routing)

Spec: `specs/system/ui-navigation.md` §2, §3, §7, §10 (plus §4 stale-response
rule, HSI §9 Briefing amendment), against assigned base `5df14f344`.
Candidate: `270b8fdf1b00d448adb2097c2639f2d548e8d397` (rebase round: merge
commit `38134ff8` onto the base, dist rebuilt, plus process commits
`3eff386b`/`270b8fdf` recording the rebase and restoring a dropped test
assertion line).
Verdict: **approved** (round 4; rounds 1–3 findings F1–F3 verified fixed in
prior evidence at heads `2ff2e596`/`a387ad98`, and their fixes are present in
the merged tree inspected this round).

Evidence: `/tmp/stage/review-evidence/review-summary.txt` plus the probe
files referenced in it (fresh runs: 19 navigation-related frontend test files
→ 414 passed / 0 failed / 41 pre-existing skips; two mutation probes; ABAC
registry check; carried exact-candidate cargo `api::graph::tests` → 28
passed / 0 failed).

## Required behavior — verified

- **Cross-workspace (§10):** `CrossWorkspaceHome` renders all five sections
  from real API calls (tests assert the exact arguments:
  `myNotifications()`, `workspaces()`, `specsForWorkspace(null)`,
  per-workspace `getWorkspaceBriefing` via `Promise.allSettled`,
  `getMetaSpecs({scope:'Global'})`). The `{notifications:[...]}` response
  shape matches the real `users.rs::get_my_notifications` handler (fixed in
  this diff). Item clicks go through `goToEntityDetail`, which resolves the
  entity's repo, derives the owning workspace from `workspace_id`/repo
  lookup, and switches `currentWorkspace` only to a workspace in the caller's
  membership list — unknown workspace surfaces an error toast, no fabricated
  scope identity. Tenant settings and tenant agent-rules remain admin-gated
  (`userIsAdmin` gates `onSettings`/`onManageAgentRules`, drawer has no
  settings escape at `/all`; server enforces admin on the underlying
  endpoints).
- **Workspace home (§2):** every section loader in `WorkspaceHome.svelte`
  queries with the selected workspace's id behind a single shared generation
  guard (`wsLoadGen`, bumped once per workspace change). Mutation-proven:
  forcing `stale()` to `false` makes the "keeps workspace B rules when
  workspace A finishes loading after navigation" regression fail. Agent Rules
  merges `getMetaSpecs({scope:'Workspace', scope_id})` with all
  `scope:'Global'` tenant rules, grouped by kind with Tenant/Workspace scope
  badges; a failed lookup surfaces as an error alert with Retry (test asserts
  the error text and the retry cycle — never an empty successful set).
  Settings/rule management use the canonical `/workspaces/:slug/settings`
  and `/workspaces/:slug/agent-rules` routes.
- **Repo mode (§3/§7):** `goToRepo` defaults to the Specs tab; `parseUrl`/
  `urlFor` (reused unchanged from base) implement the §7 table including
  multi-segment spec paths and entity detail routes; popstate restores
  workspace, repo (repoIdCache + API resolution fallback), tab, and entity
  detail. Drawer Briefing at repo scope stays in the repo (Architecture tab
  Briefing sub-tab — AppShell test asserts the URL/tab). Repo Briefing is
  narrowed server-side: `get_workspace_briefing` 404s unknown repos and 403s
  repos of other workspaces; `assemble_briefing` filters MRs, tasks,
  cross-workspace links, spec-assertion notifications, and completed agents
  by repo; `briefing_ask` runs `check_repo_abac` on the body's repo_id and
  applies the same validation — all covered by real HTTP-level Rust tests
  (28 passed, carried exact-candidate evidence; a rerun was infeasible here —
  no warm cargo target, ~3h cold build). Repo Decisions (`Inbox` scope=repo)
  filters client-side on exact `repo_id`, so `repo_id: NULL`
  workspace-scoped notifications never appear under a repo — test asserts
  real API arguments and rendered card counts, and mutation-proven (test
  fails when the filter is disabled).
- **Scope transitions:** client-side navigation with pushState/popstate
  throughout; `WorkspaceHome`, `Inbox`, and `Briefing` each carry generation
  guards; workspace/repo lookup failures surface as error toasts/states
  rather than empty success or fallback identities.

## Merge/rebase integrity

- `Inbox.test.js` conflict resolution kept both sides' additions; the final
  process commit restored the `const cards` line dropped during resolution
  (strengthening, not weakening). `web/dist` was rebuilt from the merged
  source (dist bundle contains the merged source's distinctive strings).
- `scripts/check-abac-route-registry.sh` on the merged tree → OK.
- Task contract prose in `specs/tasks/task-083.md` is byte-for-byte the
  assigned text (only `progress:`, `commits:`, and the appended `## Shipped`
  operational section differ); acceptance checkboxes remain unchecked.

## Non-blocking observation (recorded for future polish, not a contract gap)

`ExplorerView.svelte`'s mount effect consumes `?subTab=briefing` and then
strips the param from the URL. Verified empirically: a fresh deep link to
`/workspaces/:slug/r/:repo/architecture?subTab=briefing` renders the Briefing
content correctly, but the URL afterwards no longer carries the param, so a
refresh of that URL lands on the Graph sub-tab. This does not violate
ui-navigation.md §7 (the sub-tab param is an implementation extension beyond
the spec's URL table, and tab/workspace/repo scope retention is unaffected),
but it contradicts the code's own documented invariant ("mirrored into the
?subTab= URL param so deep links always match the rendered content"), and
the AppShell test asserting `window.location.search === '?subTab=briefing'`
passes only because RepoMode is stubbed in jsdom (ExplorerView, whose effect
strips the param, never mounts in that test). Suggested future fix: keep the
param in the URL after consumption (App's `setRepoArchSubTab` already
maintains it), or drop the param-stripping cleanup.

## Delegated to verification

Full frontend suite, full cargo workspace suite, architecture checks, and
exact-head CI. Playwright e2e requires a TCP listener (errno 95 in this
sandbox) — infrastructure restriction, not a code defect.
