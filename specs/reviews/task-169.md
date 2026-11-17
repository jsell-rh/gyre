# Review — task-169 (Spec registry — supply chain AIBOM integration)

Spec: `specs/system/spec-registry.md` §Supply Chain (supply-chain.md §AIBOM).
Candidate under review: `9e16f6b6` (base `876c77b7`); product commits `a4c24fc1`
(implementation), `ad473d3c` (rustfmt/clippy repair), `4bec0af5` (round-2 review
repair), plus process commits.
Verdict: **approved** — round-2 finding verified repaired by independent probe;
all acceptance criteria evidenced; no new findings.

## Round 3 (this review)

### Probes run

- `cargo check -p gyre-server --lib` → exit 0 (8m50s cold; production code
  type-checks).
- `cargo check -p gyre-server --lib --tests` → exit 0 (4m26s; the shipped test
  code type-checks).
- Standalone behavioral probe (gyre-server test compilation is rejected by the
  review sandbox's 4 GiB wrapper): extracted `merge_time_approval` verbatim
  from `crates/gyre-server/src/aibom_spec_registry.rs` lines 344-395 (saved at
  `/tmp/stage/review-evidence/merge_time_approval.extracted.rs`), included it
  into a temporary gyre-domain integration test, and drove it with REAL
  `SpecApproval` domain transitions (approve/revoke/reject):
  `cargo test -p gyre-domain --test task169_review_probe` → **10/10 passed**.
  Covered: revoke clears `approved_at` (the premise); granted@900 /
  merged@1000 / revoked@2000 → `None` (round-2 defect, now fixed); same via
  the reject path → `None`; withdrawn@1800 before merged@2000 → `Some(false)`;
  pending→rejected@500 before merge → `Some(false)`; active granted before
  merge → `Some(true)`; active granted after merge → `Some(false)`; pending
  row → `Some(false)`; active row wins over post-merge-withdrawn sibling →
  `Some(true)`; granted-after-merge + post-merge-withdrawn → `None`.
  Temporary probe files deleted; working tree restored (only the stage-setup
  task-file reset remained, which predates this review).
- Full diff inspection `876c77b7..9e16f6b6` (1431 lines, saved at
  `/tmp/stage/review-evidence/candidate-diff.patch`).
- Domain-source verification of the three-valued logic's premises
  (`gyre-domain/src/spec_approval.rs`): revoke requires status Approved
  (so grant ≤ withdrawal always holds for revoked rows); reject allows
  Pending→Rejected (never-granted rows are determinably false once withdrawn
  pre-merge); Revoked/Rejected are terminal (no re-approval can hide an
  earlier active period); `verify_spec_ref` (api/gates.rs:819) keys on the
  same `find_by_spec_sha(path, sha)` store `build_code_bindings` consults.
- Merge-processor claim verified: `updated_at = now` is stamped at the Merged
  transition (merge_processor.rs:1482, :2593) and revert overwrites
  `updated_at` (:1712) — so `merged_at` for Merged MRs is the real merge
  moment, and merged-then-reverted MRs correctly report `merged_at: None` and
  `approved_at_merge_time: None` rather than being evaluated against the
  revert timestamp.
- ABAC: no new routes; both touched routes (`GET /repos/:id/aibom`,
  `POST /release/prepare`) have pre-existing `RouteResourceMapping` entries
  (abac_middleware.rs:225, :373).

### Verified against acceptance criteria

- **AIBOM response includes `spec_registry` with approval data** —
  `AibomResponse.spec_registry` (aibom_version bumped 1.1 → 1.2 for the schema
  extension); handler test `aibom_includes_spec_registry_section` drives the
  real router and asserts section contents from seeded real storage.
- **All ledger statuses included** — approved/pending/deprecated/revoked/
  rejected, each counted, from `spec_ledger.list_all()` filtered to the
  repo's entries (cross-repo isolation tested with a repo-b entry); plus
  drift risk (`drifted` count + per-spec `drift_status`) and
  `manifest_version` = git blob SHA of `specs/manifest.yaml` at HEAD via the
  injection-safe pre-existing `resolve_blob_sha` (reported `null` when
  absent — never fabricated; proven against a real git repo in
  `manifest_version_is_the_real_git_blob_sha`).
- **Approval chain (who, when)** — for approved specs, `approved_at`
  (earliest), `approved_by`, and full `approval_chain` (approver_id,
  approver_type, persona, approved_at) from the real approval event history,
  filtered to current SHA and active events; stale-SHA and revoked events
  excluded (tested).
- **Release preparation includes spec registry metadata** —
  `ReleasePrepareResponse.spec_registry` built by the SAME
  `build_spec_registry_section`, so release metadata and the standalone AIBOM
  cannot disagree; handler test
  `release_prepare_includes_spec_registry_metadata` drives the real route.
- **Spec-to-code binding status** — every repo MR with a `spec_ref`, evaluated
  against the same per-SHA ledger the merge gate enforces, three-valued
  `approved_at_merge_time` (see round-2 repair below); `bindings_summary`
  counts. Tests cover 11 distinct MR shapes including unmerged, no-spec_ref,
  cross-repo, merged-then-reverted, and all three truth values.
- **Storage failures propagate** (`map_err(ApiError::Internal)`) — the
  section cannot silently under-report spec provenance; no fail-open.

### Round-2 durable finding — repaired and independently verified

The round-2 defect (post-merge approval withdrawals misreported as
`Some(false)` because `revoke()`/`reject()` clear `approved_at`) is fixed in
`4bec0af5`: `merge_time_approval` now treats a withdrawal timestamp AFTER the
merge as indeterminable (`None`), keeps `Some(false)` only for withdrawals at
or before the merge, and keeps `Some(true)` for a determinably-active row.
The shipped tests add mr-8 (revoke after merge), mr-9 (reject after merge),
mr-10 (active row wins over withdrawn sibling), mr-11 (no determinable row +
post-merge withdrawal → `None`). My verbatim-extraction probe confirms the
shipped function's behavior on all these shapes against real domain
transitions.

### Notes for the verifier (not findings)

- Review sandbox could not compile gyre-server test binaries (4 GiB wrapper
  rejects; belongs to the verifier). Required probes:
  `cargo test --all` (the task's own criterion), in particular
  `cargo test -p gyre-server --lib aibom_spec_registry`,
  `cargo test -p gyre-server --lib aibom_includes_spec_registry_section`,
  `cargo test -p gyre-server --lib release_prepare_includes_spec_registry_metadata`,
  and the pre-existing aibom/release tests (aibom_version is now "1.2").
- Durable CI finding (category `ci`, head `d100b576`, run 44076006080,
  "Development controller" FAILURE, logs not available) predates this
  candidate head; the verifier's remote checks on the integrated tree cover
  it.
- Generated-output observation (non-blocking, per review policy): the diff
  touches no frontend source and no `web/dist/`; a deterministic
  `npm run build` triggered by build.rs during `cargo check` reproduced
  byte-identical committed assets (working tree stayed clean).
- `specs/coverage/system/spec-registry.md` row 18 correctly moves
  `task-assigned` → `implemented` with landed evidence; row 14's Supply Chain
  note updated consistently.
