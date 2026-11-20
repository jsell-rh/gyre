# Review — task-206: meta-spec preview mode (rebase round)

**Candidate:** `07acaf04631b2f039b9c02d6ddd52816b3fd9fe6`
**Base:** `0a70b31effd9e6e85392c840fec915b1d5dc8567`
**Verdict:** approved (fresh verdict for this candidate; no earlier approval
was carried forward)
**Evidence:** `/tmp/stage/review-evidence/task-206-rebase-review.md`

## Scope of this round

The candidate is the task-206 §5 preview-mode implementation (previously
reviewed through multiple rounds) rebased onto base 0a70b31 (task-270 PR
hygiene + agent-principal default-branch push protection) via merge 97531ee3,
plus repair commit 4c85e4d7 addressing the 29 gyre-server test regressions
verification found at 886da02b, plus process commits.

## What was independently verified

1. **Merge resolution (97531ee3):** `start_job_registry` registers both the
   §5 `meta_spec_preview_gc` job (real handler bound to `meta_specs::run_once`,
   default interval 1 h) and base's `pr_hygiene_check`; main.rs spawns both
   loops; both registration tests kept. Both-sides resolution, no semantic
   loss. Base's agent-principal default-branch push guard does not interact
   with `preview/{id}/{slug}` branches.
2. **Repair commit (4c85e4d7):** audited change-by-change. The test auth
   headers match production authn (base already resolver-mapped
   `/api/v1/agents`, so the branch's extractors change no production auth
   behavior; they enable the preview-agent identity-laundering refusals). The
   one production change — removing the blanket Admin gate from
   `delete_meta_spec_registry` — restores base semantics (base delete auth is
   scope-derived via `authorize_meta_spec_delete`; the blanket gate made the
   workspace-scope branch dead code). The weakened orchestrator assertion still
   proves neither refusal minted an orchestrator. The ssh-docker fake's quote
   stripping matches the production `remote_docker` argument quoting (a real
   hardening). No test weakening found.
3. **§5 contract (merged tree):** global `POST/GET/DELETE
   /api/v1/meta-specs/preview[/:id]` routes per spec with ABAC resolver
   entries (exemption file shrank by 2 — no grandfathering); real per-target
   agent spawn (Active rows, `preview/{preview_id}/{slug}` branches, real
   worktrees, short-lived JWTs, rollback + persist-before-launch); budget
   (workspace default, `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` independent cap,
   fail-closed enumeration); draft reaches the agent env and the runner prompt
   (node test passes: draft in prompt, gates never executed, no upload, no
   completion); skip-ceremony enforced on MCP and REST (no MR/task/provenance/
   refs — asserted against real stores); status reports real agent state plus
   the produced diff; completion tears down worktree/token/slot (never Idle);
   DELETE kills agents and removes branches/worktrees/record (404 unknown);
   GC job registered with a real TTL-boundary test (aged deleted, fresh
   retained, orphan reclaimed). Hollow structural-impact implementation fully
   removed (0 occurrences).
4. **Probes:** `node --test agent-runner.test.mjs` (12/12 pass);
   `SKIP_WEB_BUILD=1 cargo check -p gyre-server` (clean); check-arch,
   check-abac-route-registry, check-abac-exempt-handlers and 15 further
   mechanical invariant scripts all pass.

## Non-defect observations

- The workspace-scoped preview paths remain registered as resolver-enforced
  twins backed by the same §5 machinery (settled in a prior round; documented
  accurately in api-reference.md). The coverage-matrix phrase "workspace-scoped
  routes removed" refers to the hollow handlers; the resolver-enforced
  replacements are stricter than the base's exempted routes.
- `delete_meta_spec_registry` now 404s missing rows before scope checks where
  the base 403'd first for non-admins; the base's GET already 404'd missing
  rows to any authenticated caller with no scope check, so this is no
  regression and the candidate is stricter overall on GET.
- Pipeline overlay commit d28e4a32 (exemption-data-follows-the-merge) is
  outside the attribution rule's scope (no task label in subject, scripts/docs
  only) and was substantively reviewed: bidirectionally tested, cannot grow
  exemptions, controller ROOT overlay copies unaffected.

## Required verifier tests

gyre-server test compilation exceeds this sandbox's budget; the verifier must
run `cargo test --all`, focused `preview_`/`registry_tests`/the 29 repaired
tests listed in the evidence file, `meta_spec_preview_gc_job_registered`,
`pr_hygiene_check_registered_on_interval_not_daily`, `cargo clippy
--all-targets`, `cd web && npm ci && npm test`, and
`bash scripts/check-task-attribution.sh` with full history.
