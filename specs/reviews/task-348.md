# Review — task-348 (Publish reviewed spec backlog for specs/development/agent-experience.md)

Spec: `specs/development/agent-experience.md` (plan `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`, audit base `87377fe1`, requirements `fd38a2fca0f8…`).
Candidate under review: `80bc59d2` (branch `pipeline/task-348/99a5124b22d349c29c44ae3027fee958-1`, base `973068a8`).
Verdict: **approved** — no findings. The candidate is a faithful, additive-only publication of the independently reviewed missing-work plan; no product code, spec text, coverage status, gate, or existing task/registry entry was changed.

## Scope of the diff

`git diff --name-status 973068a8..80bc59d2`: six added files (`specs/tasks/task-343.md` … `task-348.md`) and one additive edit to `specs/task-spec-inputs.json` (5 `backlog/<section-id>` `sections` entries + 5 `bindings` entries, insertions only — no existing entry modified). Task creation is not implementation: nothing executable changed, so no build/test probe is meaningful for the candidate itself; the verifier's standard suite covers the unchanged tree.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- **Verbatim plan-vs-docs comparison** (programmatic, `comparison-log.txt`): for each of the five planned tasks the published doc's title, `required_behavior`, every acceptance item, `negative_probe` (published as "Verification intent"), discovery-evidence JSON (id / decision / requirement_quote / detail / evidence), `spec_ref`, `depends_on`, `progress: not-started`, and empty `commits:` all match the assigned plan exactly — 46/46 PASS. `task-348.md` embeds the entire plan JSON verbatim (`json.loads` equality PASS).
- **Canonical-spec binding accuracy**: every gap-section `requirement_quote` matches `specs/development/agent-experience.md` text verbatim; the registry heading arrays (`Day-One Agent Experience Infrastructure` → each H2) match the spec's actual headings; the registry binding-key convention (`Spec gap spec-8f4edcf188b12e2a350050e2 / <key> / fd38a2fca0f8`) matches the 16 pre-existing spec-gap families, with the third component equal to the plan audit's requirements-hash prefix.
- **Audit spot-checks (challenge the audit)** — all confirmed against the tree:
  - `specs/index.md` has zero rows for `ui-navigation` / `repo-lifecycle` (grep -c → 0) and only the superseded `development/ralph-loops.md` ralph row, while `specs/system/ui-navigation.md`, `specs/system/repo-lifecycle.md`, `specs/system/ralph-loop.md` all exist on disk — the cache-miss gap is real.
  - `patrol_spec_links` (api/specs.rs ~2137) takes only `State` + `Json` — no caller identity — and `/api/v1/patrol/spec-links` sits in `scripts/abac-route-registry-exemptions.txt`; task-346 correctly demands per-handler authorization for the new fix-up write path.
  - No `fixup`/`fix-up` anywhere in `crates/`; no `garden` anywhere in `scripts/`, `crates/`, `.github/` — both claimed gaps are real.
  - `api/agent_logs.rs` query surface is limit/offset only; `seed_builtin_meta_specs` (lib.rs:2726) seeds exactly one Principle (`conventional-commits`); task-292 exists as not-started.
- **Duplicate-work / dependency analysis**: each of the five backlog section ids is bound exactly once across all 277 registry binding keys (no duplicate active assignment); no other task references `agent-experience.md`. Dependencies are correct and minimal: task-344 → task-343 (per-task observability stack builds on the telemetry store), task-347 → task-346 (doc-garden uses `ensure_fixup_task`); task-343/345/346 independent. Overlaps are explicitly delineated in the task texts: task-346 vs task-292 (per-handler containment additive to task-292's middleware-ABAC/exemption-shrink; no exemption-file changes; task-292 not a dependency), and task-345/347's ci.yml additions coordinate with the ci-docs-release and architecture plans.
- **Decomposition challenge**: five gap sections → five tasks, one-to-one; the parent heading is correctly classified `context` and Agent Legibility `satisfied` (its concrete obligations — lint remediation messages, boring dependency stack, map-style AGENTS.md — are met), so neither generates a task, consistent with the goal's out-of-scope rules.
- **Commit-attribution rule**: the candidate commit (`chore: publish reviewed spec backlog 06fdb4a49c8e`) carries no task-NNN label and touches only `specs/` — out of scope for `check-task-commit-attribution.sh` on both of its gating conditions. The full-history run of that script timed out at 120 s in this sandbox (history-iteration cost — infrastructure, not a code signal); the verifier runs it in CI with `fetch-depth: 0`.

## Verifier must run (standard, unchanged-tree confirmation)

- Full workspace `cargo test --all` and Clippy per standard verification (no code changed; confirms no accidental breakage).
- The mechanical fleet in CI, including `check-task-commit-attribution.sh` (needs full history) and the frozen-exemption-count checks — no exemption file was touched by this candidate.
- `specs/task-spec-inputs.json` JSON validity (locally verified with `python3 -m json.tool`).
