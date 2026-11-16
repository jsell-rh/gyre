# Review — task-206 (meta-spec-reconciliation.md §5 Preview Mode: real agent preview runs)

Spec: `specs/system/meta-spec-reconciliation.md` §5 "Preview Mode: The Fast Iteration Loop".
Candidate under review: `5208bb95a7bdd9d1c2fca9e987778aff06c1f411` ("implement(task-206): pipeline checkpoint") on branch `pipeline/task-206/2c0af0e5a83e4a00bee941247a78842c-4`, diffed against assigned base `c3c7a522b721d266843b9590b14b847ac323593c`.
Verdict: **needs-revision** (reject — candidate does not build).

## Round 1

Evidence retained at `/tmp/stage/review-evidence/` (summary.md + raw probe outputs).

### F1 (blocking): the candidate commit contains 63 unresolved merge-conflict hunks — the tree cannot compile, parse, or run any test

The checkpoint `5208bb95` captured a merge of the task-206 branch onto the assigned base `c3c7a522` that was never resolved: `<<<<<<< HEAD` … `>>>>>>> c3c7a522b721d266843b9590b14b847ac323593c` marker pairs are committed as-is across 22 files — 10 Rust sources in gyre-server (`jobs.rs`, `api/meta_specs.rs` ×7 hunks, `api/spawn.rs` ×7, `api/budget.rs` ×3, `api/admin.rs`, `mcp.rs`, `mem.rs` ×6, `main.rs`, `abac_middleware.rs`, `merge_processor.rs` ×16), authored web sources (`MetaSpecs.svelte`, `EditorSplit.svelte`, `MetaSpecs.test.js`, `locales/en.json` ×4), `docker/gyre-agent/agent-runner.mjs` + its README, both task-mandated doc updates (`docs/api-reference.md`, `docs/server-config.md`), `specs/coverage/system/ui-layout.md`, and three `scripts/*-exemptions.txt` files. The base itself is clean (`git grep -c '^<<<<<<<' c3c7a522` → 0).

Hard probes (see `/tmp/stage/review-evidence/` for full outputs):

- `rustfmt --edition 2021 --check crates/gyre-server/src/jobs.rs` → exit 1, "mismatched closing delimiter" at the `meta_spec_preview_gc` registration wrapped in markers (jobs.rs:568-698: candidate's preview-GC job registration vs. base's `search_code_index`/`meta_spec_stale_pin_check` registrations, both sides left in).
- Same for `api/meta_specs.rs` (unclosed delimiter at the `post_meta_spec_preview` signature region) and `api/spawn.rs`.
- `python3 -m json.tool web/src/locales/en.json` → exit 1, invalid JSON at the marker on line 887.
- `node --check docker/gyre-agent/agent-runner.mjs` → exit 1, `SyntaxError: Unexpected token '<<'` at line 308.

Consequence: `cargo check -p gyre-server` fails at parse; the frontend test suite cannot even load (`en.json` is imported by the i18n module); the agent runner is syntactically invalid. **No acceptance criterion — spawn, draft injection, 202 shape, skip-ceremony, status+diff, DELETE teardown, GC TTL boundary — could be exercised at all.** A checkpoint whose tree cannot build is not a reviewable candidate, regardless of the quality of the work underneath. Note these conflicts are in *authored* source (Rust/Svelte/JS/JSON/docs/scripts), not generated `web/dist/` output, so the generated-output carve-out does not apply.

Remedy: actually resolve the merge against base `c3c7a522` (all 63 hunks), rebuild, rerun the full test battery, and re-submit. The HEAD side does contain a real §5 implementation (`provision_preview_agent`, `preview_branch_name`, `finish_preview_agent`, global routes in `api/mod.rs`, the `meta_spec_preview_gc` job), so this is a merge-completion failure, not a hollow-implementation failure — but the candidate as committed is unpublishable.

### F2 (blocking): the conflicted exemption files would re-freeze violations if resolved to the candidate side

The three `scripts/*-exemptions.txt` conflicts must not be resolved by taking the candidate (HEAD) side: it re-adds exemption entries the base deliberately removed or froze — `fail-open-ref-resolution-exemptions.txt` +2 entries against a frozen **0** ("It must STAY empty, never grow"), `inert-enforcement-exemptions.txt` +4 entries (merge_processor/orchestrator sites fixed on base), `relative-path-defaults-exemptions.txt` +1 entry. Keeping the candidate side silently grandfathers real violations past the frozen-baseline CI checks; the conflicts must be resolved to the base (empty) sides. Recorded so the resolution round does not "resolve" these by picking HEAD.

### Not verified (consequence of F1)

All behavioral claims — including the previous revision's "Shipped" notes and its earlier rebase round ("all six conflicts resolved" against a *different* base, 3c39ab1f) — carry no evidentiary weight for this candidate. The verifier must run, on the re-submitted candidate: `cargo check -p gyre-server`; `cargo test -p gyre-server --lib` (preview suite); `node --check docker/gyre-agent/agent-runner.mjs`; `python3 -m json.tool web/src/locales/en.json`; `cd web && npm ci && npx vitest run src/__tests__/MetaSpecs.test.js`; `grep -rn '^<<<<<<<'` over the tree (must be empty); plus the standing invariant checks (arch, abac-route-registry, frozen exemption counts, mcp-write-tools).

Scope note: the working-tree modification to `specs/tasks/task-206.md` (frontmatter reset, "Shipped" section removed) was made by the pipeline before this review; no source was modified during review.
