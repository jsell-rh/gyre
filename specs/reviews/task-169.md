# Review — task-169 (Spec registry — supply chain AIBOM integration)

Spec: `specs/system/spec-registry.md` §"Supply Chain (supply-chain.md)".
Candidate under review: `b2acc968c5f8e63d85242dc09c737482b8b233c6` (base `6082be2f`).
Product commit: `a4c24fc1` (feat), `ad473d3c` (verification-repair: rustfmt + clippy `useless_format`, whitespace-only otherwise); `6b0503c2`/`f20437bd`/`b2acc968` process-only.
Verdict: **approved** — no findings.

## Round 1 (2026-11-16)

Probes run (evidence in `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → exit 0 (6m13s).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0, no warnings on task files (3m29s). Full test-binary codegen/link is deferred to the verifier per the 4 GiB executor budget; commands recorded in `verifier-commands.txt`.
- Structural gates against the extracted candidate tree (full history): attribution, arch, ABAC route registry, mem port contracts, inert enforcement, unbounded HTTP, byte-slice truncation, fabricated/lookup-fallback scope defaults, lossy secret conversion, fail-open ref resolution — all OK. Attribution passes at the candidate (both product commits in `commits:` frontmatter); the working-tree failure observed first was caused solely by the harness-staged `commits: []` reset of task-169.md, not the candidate.
- `rustfmt --check` on all four touched Rust files — clean (round-0 verification finding repaired in `ad473d3c`).
- Diff inspection of `a4c24fc1`/`ad473d3c` for all eight touched files; source-level verification of every supporting production path the new module depends on.

Verified against the task contract:

- **`spec_registry` AIBOM section with approval data** — `aibom_spec_registry.rs` builds one shared section embedded verbatim by both surfaces: `GET /api/v1/repos/:id/aibom` (`AibomResponse.spec_registry`, aibom_version bumped 1.1→1.2) and `POST /api/v1/release/prepare` (`ReleasePrepareResponse.spec_registry`), so release metadata and the standalone AIBOM cannot disagree. Additive fields only — CLI and web clients consume the JSON opaquely; no breakage.
- **All ledger statuses included** — every `ApprovalStatus` variant (approved/pending/deprecated/revoked/rejected) with per-status counts and `total_specs`, from real `spec_ledger.list_all()` filtered to `entry.repo_id == repo` (repo_id is persisted by both the SQLite and Postgres adapters and backfilled by the ledger sync on push; cross-repo exclusion is asserted in tests).
- **Approval chain (who, when)** — for approved specs, active approval events for the current SHA from `spec_approval_history.list_by_path` (same path form the approve endpoint records): earliest `approved_at`, `approved_by` ids, and full `approval_chain` (approver_id/type/persona/approved_at). Stale-SHA and revoked events excluded. Specs with `requires_approval: false` (spec §17) legitimately have no events; the empty chain is the honest ledger state and is documented as such.
- **Release preparation includes spec registry metadata** — verified above; storage failures propagate via `ApiError::Internal` (the section cannot silently under-report).
- **Spec-to-code bindings** — every repo MR with a `spec_ref`, evaluated against the same per-SHA approval ledger (`spec_approvals.find_by_spec_sha`) the merge gate `verify_spec_ref` enforces: an approval counts only when `approved_at <= merge_ts` and not revoked/rejected at or before the merge — exactly `is_active()` reconstructed at the historical merge moment (boundary cases `==` match the gate's present-tense semantics). `merged_at` uses the Merged MR's `updated_at`, which the merge processor stamps at the merge transition (merge_processor.rs:1477/2591) and which no post-merge writer mutates except revert; merged-then-reverted MRs honestly report `None` rather than evaluating against the revert timestamp. `manifest_version` is a real `git rev-parse HEAD:specs/manifest.yaml` blob SHA via the injection-safe `resolve_blob_sha` (pre-existing), `null` when absent — never fabricated.
- **Drift risk + manifest version** (spec-registry.md §Supply Chain bullets) — `drift_status` per spec, `drifted` count, manifest blob SHA.
- **Tests are real, not self-confirming** — the bindings test is a 7-scenario truth table including the cases a "currently approved" check would get wrong (approval granted after merge → `Some(false)`; revoked before merge → `Some(false)`; open MR → `None`; reverted → `None`; cross-repo MR excluded; no-spec_ref MR produces no binding). The manifest-version test creates a real git repo and compares against an independent `git rev-parse` oracle. The two API tests drive the real router through the ABAC middleware and assert concrete values (counts, chain contents, `bindings_summary`). Disable `build_spec_registry_section` or weaken the merge-time predicate and these fail.
- **Coverage matrix** — row 18 `task-assigned` → `implemented` with an accurate description; row 14 updated; not prematurely marked `verified`. `docs/api-reference.md` updated for both endpoints.
- **No new routes** (both endpoints pre-existed with ABAC mappings at abac_middleware.rs:225/363), no new migrations, no frontend requirement in the task.

Test-inflation check: none found. Scope check: no out-of-scope edits — diff touches only the four server files, docs, coverage, and the task file.

Verifier must run (recorded in `/tmp/stage/review-evidence/verifier-commands.txt`): `cargo test -p gyre-server aibom_spec_registry`, `cargo test -p gyre-server aibom_includes_spec_registry_section`, `cargo test -p gyre-server release_prepare_includes_spec_registry_metadata`, `cargo test --all`, `cargo clippy --all-targets`.
