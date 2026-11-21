# Review — task-284 (AIBOM dependency-graph snapshot and stale-dependency flags)

Spec: `specs/system/dependency-graph.md` §Version Drift Tracking, third bullet — "The AIBOM flags stale dependencies in release reports".
Candidate under review: `992bba5a` (branch `pipeline/task-284/1aa0b37412cc47c2a8df6ad37e3d41ba-1`, base `58affdc6`). Product commits: `f7a475ee` (implementation), `a5d42c8b` (pure rustfmt pass — verified semantically identical), `0ffd039d`/`4328b061` (CI repair), plus docs `a5bd2953` (docs-only, out of attribution scope by the check's own definition).
Verdict: **approved** — no findings. All four product commits are recorded in the task's `commits:` frontmatter.

## Probes run

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0, 10m28s (cold). Fresh `libgyre_server-*.rmeta` and `test-lib-gyre_server` fingerprints under `/tmp/gyre-build/target` confirm the lib and the test targets (the `mod tests` blocks in aibom.rs, meta_specs.rs, gate_executor.rs) were type-checked. Log: `/tmp/stage/review-evidence/task-284-cargo-check-review.txt`.
- `bash scripts/check-arch.sh`, `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh` → all exit 0 (logs in review-evidence).
- Full diff read of all six touched files; grep sweeps for the AIBOM shape consumers (no other constructor of `AibomResponse`/`AibomReleaseResponse` exists outside `api/aibom.rs`), the staleness writers, and the env-dump readers.
- `scripts/check-task-commit-attribution.sh` timed out in this sandbox (full-history scan) — attribution was audited manually instead (see below); the verifier should run the script.

## Verified working

- **Real store, no re-derivation.** `build_dependencies_section` (`api/aibom.rs:572`) reads `state.dependencies.list_by_repo` (outgoing) and `list_dependents` (incoming) through the `DependencyRepository` port — the same store the push-time pass (`git_http.rs:4804`) and the daily `dep_staleness_check` job (`dep_staleness.rs:155`/`259`) maintain. The section copies the stored `status`/`version_drift`/`version_pinned`/`target_version_current` verbatim and flags `stale: e.status == DependencyStatus::Stale`, so the graph and the AIBOM cannot disagree; storage errors propagate (`?` — a compliance artifact never silently under-reports).
- **Both shapes, always present.** The section is built once in `get_aibom` after repo resolution and embedded in `AibomResponse.dependencies` (working summary) and `AibomReleaseResponse.dependencies` (`format=release`). An all-current set shows the edges with an empty risk list; a repo with no edges shows an empty snapshot with a stating `note` (`serde(skip_serializing_if)`). The risk list (`stale_dependencies`) names each stale edge with drift and both versions plus a `supply_chain_risk` string; `stale_count` is the list length; `dependent_count` from `list_dependents`.
- **Tests are real, not structural.** `aibom_release_format_flags_stale_dependency` drives the HTTP router against the real store (real-git release fixture + API-created repos B/C/D, edges saved through the port): asserts both edges listed with full stored state, the stale edge flagged in the risk list with drift/versions, `stale_count` 1, `dependent_count` 1 — then resolves B's drift in the store and re-GETs the SAME release inputs: empty risk list, section still present, resolved edge unflagged (AC 3: flags track the live store; the second GET is the all-current case). `aibom_summary_carries_stale_dependency_flags` covers the default-format stale list/count and the no-edges empty-snapshot note (AC 4). Assertions are on serialized JSON field values — no mirrored logic; removing the section or the flag breaks them. Enum string forms (`stale`/`active`/`code`/`cargo_toml`) are asserted, so a serde regression is caught.
- **Partition respected.** No release-MR/tag/publish machinery added; the fields compose with the staged `release-artifacts-aibom-sbom` task. No route, migration, MCP tool, or message-kind changes (relevant mechanical checks pass).
- **Coverage/docs accurate.** Row 13 of `specs/coverage/system/dependency-graph.md` is the only row touched and its line references check out (`aibom.rs:97/268/572`, tests at `:2141`/`:2288`); `docs/api-reference.md` documents the section. `aibom_version` stays 1.2 — consistent with the spec_registry shape addition precedent (task-169).
- **CI repair is sound and assertion-neutral.** Retained log for run 44902138294 shows `preview_spawns_real_agents_with_branches_worktrees_and_draft` failing "preview id must be injected" on a partial env dump in which `GYRE_AGENT_ID` (same injection path) is present — a torn multi-block stdio flush, not a missing injection. The fix (per-PID temp file + `mv` rename in `env_dump_script`, and the same pattern for the gate-executor per-agent dump in `4328b061`) makes each dump observable only complete; every dump reader polls for content (`soon(... !s.is_empty())`), no other `env_dump_script` user reads the dump, and no assertion was weakened or changed. The `spawn.rs` kubectl-stub capture was correctly left alone (single `cat` write).

## Verifier must run

gyre-server test-binary compilation exceeds this sandbox's budget (wrapper blocks `cargo test -p gyre-server`):

- `cargo test -p gyre-server aibom_release_format_flags_stale_dependency aibom_summary_carries_stale_dependency_flags` (this task's probes; names select two filters, run separately if the runner takes only one).
- `cargo test -p gyre-server --lib aibom` (pre-existing aibom regression).
- `cargo test -p gyre-server preview_spawns_real_agents` (repaired CI flake, incl. the CI-failed variant) and `cargo test -p gyre-server trigger_gates_launches_gate_agent_runtime_with_token` (gate_executor dump made atomic).
- `bash scripts/check-task-commit-attribution.sh` (needs full history; timed out here — manual audit found all four crates-touching task-284 commits in the frontmatter).
- Standard full workspace suite + Clippy.
