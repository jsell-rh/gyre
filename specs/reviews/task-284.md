# Review — task-284 (AIBOM: dependency-graph snapshot and stale-dependency flags in release reports)

Spec: `specs/system/dependency-graph.md` §Version Drift Tracking, third bullet ("The AIBOM flags stale dependencies in release reports").
Candidate under review: `2ee32568` (branch `pipeline/task-284/1221a17a6e3243e4a7ce0032c11d3909-1`, base `bacba2b2`; product commits `f7a475ee`/`a5d42c8b`, docs `a5bd2953`, shipped-notes repair `2ee32568`).
Verdict: **approved** — no findings. The dependencies section is built from the real dependency store at AIBOM build time, embedded in both output shapes, and flagged stale edges as supply-chain risk; the probe tests drive the real router against the live store and fail when the section is absent.

## Probes run (checkout at candidate `2ee32568`, working tree clean)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0** (8m59s) — type-checks the handler and both new tests.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo fmt --check -p gyre-server` → exit 0.
- `git diff --check bacba2b2..HEAD` → clean (previous verification finding about trailing whitespace in the task notes is repaired).
- `bash scripts/check-task-commit-attribution.sh` → exit 0 (both task-labeled product commits are in the `commits:` frontmatter; `a5bd2953` docs commit and the process commits are correctly scoped).
- `bash scripts/check-abac-route-registry.sh` → exit 0 (no route changes; no registry edits in the diff).
- Full read of the complete diff (4 files: `api/aibom.rs` +424, `docs/api-reference.md` one row, coverage row 13, task notes).
- Evidence: `/tmp/stage/review-evidence/task-284-review.md`.

## What was verified

- **Real store, no re-derivation.** `build_dependencies_section` (aibom.rs:572) reads `state.dependencies.list_by_repo` (outgoing edges) and `list_dependents` (incoming count) and copies the stored `status`/`version_drift`/`version_pinned`/`target_version_current` verbatim — the AIBOM never recomputes drift, so the graph and the AIBOM cannot disagree, and an edge marked stale (or resolved) in the store changes the next build for the same release inputs. Storage errors propagate (`map_err(ApiError::Internal)`), so the artifact never silently under-reports.
- **Adapter semantics confirmed for all three implementations** (PG `dependency.rs:140/197/211`, SQLite `dependency.rs:140/197/211`, mem `mem.rs:2339-2371`): `save` is upsert-on-id, `list_by_repo` filters `source_repo_id` (outgoing only), `list_dependents` filters `target_repo_id`. The domain enums are `snake_case` unit variants, so the `enum_snapshot_str` serialization and the test assertions (`"stale"`, `"active"`, `"code"`, `"cargo_toml"`) are exact.
- **Both shapes carry the section.** It is built once per request in `get_aibom` before the format branch and embedded in `AibomResponse.dependencies` (default summary) and `AibomReleaseResponse.dependencies` (`format=release`). Section contains per-edge snapshot (target repo, type, detection method, pinned/current versions, drift, status, explicit `stale` flag), `dependent_count`, `stale_dependencies` risk list naming each stale edge's drift and both versions, `stale_count`, and a `note` for the no-edge case — always present, never omitted; an all-current set shows edges with empty flags.
- **Partition respected.** No release-MR/tag/publish machinery added; only the dependency/stale content of the AIBOM shapes (the staged `release-artifacts-aibom-sbom` task owns artifact generation). Docs row updated accurately; coverage row 13 extended with concrete source evidence; no other rows touched.
- **Tests are real probes, not inflation.** `aibom_release_format_flags_stale_dependency` (aibom.rs:2141) uses the real-git `release_repo_with_commits` fixture and the real `api_router()`: saves a stale edge A→B (1.2.0→1.5.0, drift 3, Stale), a current edge A→C, and an incoming edge D→A through the same `Arc<dyn DependencyRepository>` the handler reads; asserts the release format lists both outgoing edges (`edges.len() == 2` — catches a bidirectional `list_by_repo`), flags the stale edge, names it in the risk list with versions and drift, `stale_count` 1, `dependent_count` 1; then resolves B's drift in the store and re-GETs the SAME release inputs → empty risk list with the section still present (AC 2 and AC 3 in one probe). `aibom_summary_carries_stale_dependency_flags` (aibom.rs:2288) covers the default-format flags and the no-edge repo's empty snapshot with a stating `note`. Pre-fix, `json["dependencies"]["edges"].as_array().unwrap()` panics on the missing key — the tests fail when the behavior is absent.

## Deferred to verifier (test-binary compilation exceeds this sandbox's 4 GiB budget)

- `cargo test -p gyre-server --lib aibom` — runs both new tests plus the pre-existing aibom tests for regression.
- Full workspace suite + Clippy per standard verification.
