# Review: task-083 — Canonical Navigation — Scope-Aware Content Routing

Candidate: `9414eaf6` (base `3c39ab1f`). Independent review round; evidence in
`/tmp/stage/review-evidence/task-083-review-round3.txt` (frontend probes,
byte-identical dist rebuild, cargo logs in `cargo-graph-tests.log`).

## Prior-round findings (F1–F3 from the earlier review of this code)

All three repaired and independently verified:

- [x] **F1 (NodeDetailPanel import dropped).** `import NodeDetailPanel` is
  restored in `ExplorerView.svelte:8` and used at :1684. Stronger than a grep:
  `vite build` of the committed source into `/tmp/dist-check` is
  **byte-for-byte identical** to the committed `web/dist` (`diff -r` clean), so
  the shipped bundle provably contains the current (fixed) source and no
  dangling reference.
- [x] **F2 (dead Architecture sub-tab navigation).** The Graph|Briefing
  sub-tab is now controlled from App: `repoArchSubTab` state, `archSubTab`
  prop through `RepoMode` into `ExplorerView` (`activeArchSubTab` derived from
  the prop when provided), `onArchSubTabChange` → App `setRepoArchSubTab`
  mirrors `?subTab=` on every change; drawer Briefing at repo scope calls
  `goToRepoTab('architecture', { subTab: 'briefing' })` and stays in the repo.
  The two regression tests assert the real
  `getWorkspaceBriefing('ws-1', null, 'repo-1')` fetch and the rendered
  `[data-testid="arch-briefing"]` content.
- [x] **F3 (merge-queue sort lost).** `.sort((a, b) => (a.position ??
  a.priority ?? 0) - (b.position ?? b.priority ?? 0))` restored
  (`WorkspaceHome.svelte:967`).

## Required behavior verification

- **§10 Cross-workspace:** `CrossWorkspaceHome` renders Decisions
  (`myNotifications`, `{notifications:[...]}` shape fixed), Workspaces,
  Specs (`specsForWorkspace(null)` with workspace/repo attribution rows),
  Briefing (per-workspace `getWorkspaceBriefing` aggregation), Agent Rules
  (Global scope). Item clicks go through `goToEntityDetail`, which resolves
  the entity's repo, derives the owning workspace, and switches
  `currentWorkspace` from the caller's membership list — unknown workspace →
  error toast, no fabricated scope identity. Tenant settings/agent-rules stay
  admin-gated.
- **§2 Workspace home:** all section loaders query with the selected
  workspace's id behind one `wsLoadGen` generation guard; Agent Rules merges
  workspace + tenant Global rules with error + Retry (no empty-success
  masking); settings/rule management use canonical routes
  `/workspaces/:slug/settings`, `/workspaces/:slug/agent-rules`.
- **§3/§7 Repo mode:** `goToRepo` defaults to Specs; tabs URL-addressable;
  popstate restores workspace/repo/tab (repoIdCache + API fallback). Repo
  Briefing narrowed server-side (`?repo_id=` on GET and in the ask body);
  `assemble_briefing` filters MRs, tasks, cross-workspace links,
  spec-assertion notifications, and completed agents (by `agent.repo_id`);
  the REST handler 404s unknown repos, 403s cross-workspace repos, and the
  ask handler additionally evaluates repo ABAC against JWT claims (G6
  pattern, test-proven 403). Repo Decisions (`Inbox` scope=repo) filters on
  exact `repo_id`, so `repo_id: NULL` workspace notifications never appear.
- **§4 Scope transitions:** generation guards in `WorkspaceHome`, `Inbox`,
  and `Briefing`; lookup failures surface as errors/toasts, not empty data.
- **Canonical shell:** no sidebar; mobile drawer workspace-section nav intact
  (`NoSidebar`, `WorkspaceDrawerSectionNav` tests pass).

## Probes run (focused, smallest meaningful)

1. `npm ci` (locked) then `npx vitest run` over the 15 navigation-related
   test files: **358 passed / 0 failed / 41 pre-existing skips** (superset of
   the shipped 10-file claim).
2. `npx vite build --outDir /tmp/dist-check`: EXIT=0; `diff -r` with committed
   `web/dist`: **identical** (dist freshness / F1).
3. `cargo test -p gyre-server api::graph::tests`: **28 passed / 0 failed**
   (includes all 5 new repo-scope tests); `cargo test -p gyre-server briefing`:
   **24 + 3 passed / 0 failed** (incl. 4 MCP briefing-resource parity tests);
   `cargo test -p gyre-server --test graph_integration`: **37 passed /
   0 failed**. Environment note: the repo-config default linker (mold) wedged
   in D-state I/O in this sandbox; the probe was rerun with
   `RUSTFLAGS="-C link-arg=-fuse-ld=lld"` (LLD 18.1.3 is the runtime-compatible
   linker). Linker choice does not affect test semantics; no source or config
   was modified and the working tree stayed clean.

`graph.rs`/`mcp.rs` at the candidate are byte-identical to the prior
independent review checkpoint (`7645390d`) and to `a387ad98`, so this round's
test execution covers exactly the previously reviewed server-side source. The
candidate's product source is identical across `f46b2f8f`, the merge commit,
and the candidate head.

## Non-blocking observations (cosmetic; no required behavior affected)

- `WorkspaceHome.svelte` Agent Rules markup uses `class="rule-version"` while
  the new CSS defines `.rules-version` — the version badge renders unstyled.
- `ExplorerView`'s mount effect reads then deletes `?subTab=` — matches the
  pre-existing Code tab convention, so deep-link entry works and the param is
  consumed on mount as before.

## Verdict

Approved. All acceptance criteria are satisfied by production code verified
with focused probes; prior findings F1–F3 are fixed; no new defects found.
