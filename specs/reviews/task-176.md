# Review — task-176 (Meta-specs Preview Loop layout — editor split states and preview workflow)

Spec: `specs/system/ui-layout.md` §9 "Meta-specs Preview Loop Layout".
Assigned base: `cba416b46a73d3568f745d7b4eae49a0700a90a6`. Candidate: `c41e24a763a68028b1d5faf76b3594fe4eb784af`.
Verdict: **needs-revision** — one material finding (F1, stale shipped bundle, fifth consecutive round).

## Round 5

Probes (evidence: `/tmp/stage/review-evidence/task176-r5-dist.log`):

- `git diff --shortstat cba416b46 c41e24a76 -- web/dist` → **empty** (byte-identical to base).
- `git grep -c locked-diff|metaSpecsAssist|pending_changes c41e24a76 -- web/dist` → **0 matches** each.
- `git grep -c locked-diff c41e24a76 -- web/src` → 43 matches (EditorSplit.svelte ×18, tests ×25).
- `git log --oneline -3 c41e24a76 -- web/dist` → last dist-touching commit is `5454c2d8e` (task-175 build, before the assigned base).
- `git log cba416b46..c41e24a76 --grep='build(web)'` → **no build commit since base**.
- `git diff --shortstat 5b1d8540 c41e24a76 -- web/src` → **empty** — web/src is byte-identical to the round-4 candidate `5b1d8540`, whose src-side implementation was already vetted across rounds 1–4 (Iterate retention via `showResultTabs` at `EditorSplit.svelte:249`, workspace-scoped `metaSpecsAssist` at `api.js:677`, server endpoint in `specs_assist.rs` with ABAC mapping, publish-baseline refresh `6e3bb1a`). The only delta this round is the base merge `61d5c50` (server-side task-118 crates, no `web/src` overlap) and a docs-only tip commit `c41e24a76` (task-176.md notes only, 26+/15−).

### F1 (blocker) — stale shipped bundle, fifth consecutive round

`web/dist` at candidate `c41e24a76` is byte-identical to the assigned base's task-175 bundle (`5454c2d8e`). None of the §9 preview-loop UI is in the shipped artifact: `git grep` for `locked-diff`, `pending_changes`, and `metaSpecsAssist` against the committed dist returns 0 matches, while `web/src/lib/EditorSplit.svelte`, `lineDiff.js`, `api.js`, and `MetaSpecs.svelte` carry the entire §9 surface (State 2 locked diff, Iterate retention, workspace-scoped assist chat). `gyre-server` RustEmbeds `web/dist` (`crates/gyre-server/src/spa.rs:6`, `#[folder = "../../web/dist"]`) and `SKIP_WEB_BUILD=1` builds serve the committed bundle — the documented Rust-only deployment (AGENTS.md quick start) renders none of this task's UI.

The original State 2 commit `eee86b3e6` did ship a regenerated dist (`index-BsanhgHD.js`, locked-diff ×2), but every merge (`5b0e5b491`, `671873eea`, `4361d5501`, and now `61d5c50`) resolved `web/dist` back to the base side, dropping it. The repair prescribed in rounds 1–4 was again not performed: there is no `build(web)` commit in `cba416b46..c41e24a76`.

**Repair:** `cd web && npm run build`, commit as `build(web): regenerate verified assets (task-176)`, append that SHA to `specs/tasks/task-176.md` `commits:` frontmatter, and re-grep the dist for `locked-diff`/`metaSpecsAssist`/`pending_changes`. When merging future bases, resolve `web/dist` to the branch side (or rebuild after the merge) — `git diff <base> HEAD -- web/dist` must not be empty.

### Verified clean (unchanged from round 4)

- `web/src` is byte-identical to the round-4 candidate; all previously fixed src-side findings remain in place (`showResultTabs` Iterate retention, workspace-scoped §9 LLM chat, publish-baseline refresh).
- Commits frontmatter at the candidate lists the four task product commits (`f51cc2296`, `91fe1e86d`, `6e3bb1a7a`, `eee86b3e6`).
- The base merge `61d5c50` does not touch `web/src`, so no src-side regression could be introduced by it; the only candidate-vs-round-4 changes are in `crates/` (task-118's own work, out of scope here) and `specs/tasks/task-176.md`.
