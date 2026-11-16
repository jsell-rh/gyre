# Review — task-166 (Supply chain: SLSA provenance mapping, AIBOM release artifacts, laptop problem)

Spec: `specs/system/supply-chain.md` §AIBOM Contents, §SLSA Provenance Integration, §The Laptop Problem.
Candidate under review: `7b083fab` (product commit `39a0aaec` + status commit), base `6082be2f`.
Verdict: **needs-revision** — one blocking finding (F1: the AIBOM release-artifact feature is non-functional); two secondary findings.

## Round 1

Probes (evidence in `/tmp/stage/review-evidence/`):

- Standalone git probe replicating `git_rev_list`'s exact command shape against a
  real bare repo (git 2.43.0): `git -C <repo> rev-list -- <range>` exits 129 for
  both range forms; the same ranges without the misplaced `--` succeed. See
  `probe-rev-list.md`.
- `cargo test -p gyre-domain` → 572 passed, 0 failed.
- `cargo test -p gyre-cli` → 223 passed, 0 failed.
- `cargo check -p gyre-server` → clean (server test-target compilation and the
  server test list deferred to the verifier per the 4 GiB budget note).
- `scripts/check-abac-route-registry.sh` → OK; `scripts/check-arch.sh` → OK.

### Findings

**F1 (blocking, code — aibom.rs `git_rev_list`):** the `--` end-of-options
separator is placed before the range argument
(`.args(["-C", repo_path, "rev-list", "--"]).arg(range)`), so git treats the
range as a pathspec with zero revisions and exits 129 for every input.
`git_rev_list` always returns `None`, `build_release_aibom` always returns
`NotFound`, and `GET /repos/:id/aibom?format=release` **always 404s** — the
structured release artifact (§AIBOM Contents, coverage row 14) can never be
produced. The new tests `aibom_release_format_full_attribution` and
`aibom_release_format_range_and_unknown_format` assert 200 OK on `?format=release`
and fail, so acceptance criteria "AIBOM release artifact format generates
structured JSON" and `cargo test --all` are not met. Coverage row 14 is marked
`implemented` on a path that cannot execute successfully. The Shipped notes
deferred exactly these tests to the verifier, so the defect was never
exercised in-sandbox. Fix: drop the `--` (from/to are already validated by
`is_valid_ref`, which rejects leading dashes and `--`, and the range is a
single argv element).

**F2 (code — provenance.rs `get_slsa_provenance`):** the per-commit
attribution and weakest-link `slsaBuildLevel` are computed over
`agent_commits.find_by_repo` — every agent commit ever recorded for the repo —
not commits reachable from the subject ref. Commits on unrelated/deleted
branches appear in the statement and cap the subject's level (a fully
Level-3-attested subject reports SLSA 0 because of one unattested commit on a
deleted branch). Over-conservative rather than inflated, but it misrepresents
the subject's provenance for compliance review. Scope to the subject's history
or document the repo-wide semantics.

**F3 (minor — stack_attest.rs `get_stack_drift`):** docs claim "default 100,
cap 500" events, but with no `?limit` the handler applies no truncation and
returns up to 1000 events (the underlying query cap).

### Verified clean

- **Laptop problem (§The Laptop Problem) is genuinely implemented**:
  warn-mode drift without `X-Gyre-Stack-Override-Reason` → 403 + refs undone +
  blocked drift event; with a reason → accepted + fully-contextualized accepted
  drift event; block-mode → 403 + blocked drift event; the no-policy
  lockfile-present warn-default path requires the reason too. Events go through
  `emit_server_audit_event` → real audit repository; `GET /repos/:id/stack-drift`
  is repo-scoped and newest-first (sqlite adapter orders by timestamp desc).
- **SLSA envelope shape** (in-toto Statement v1, predicateType
  `https://slsa.dev/provenance/v1`, subject = resolved ref with gitCommit
  digest) and the level mapping (L1→0; L2→1/2 with verified lockfile;
  L3→3) are structurally correct and evaluated against real state; the
  push-time label mapping (`server-verified`/`self-reported`/`unattested`,
  git_http.rs:1017-1023) matches `attestation_level_from_label`.
- Ref validation (`is_valid_ref`, `--verify --end-of-options`), ABAC registry
  entries for both new routes, CLI `--stack-override-reason` plumbing, SIEM
  severity, and the AuditEventType round-trip all check out.

### Verifier must run

```
cargo test -p gyre-server --lib aibom_release_format        # expected FAIL (F1)
cargo test -p gyre-server --lib slsa_provenance
cargo test -p gyre-server --lib slsa_level2_maps
cargo test -p gyre-server --lib warn_mode_drift
cargo test -p gyre-server --lib block_mode_drift_rejection
cargo test -p gyre-server --lib lockfile_drift_without_policy
cargo test -p gyre-server --lib stack_drift_endpoint
cargo test -p gyre-server --lib stack_policy
cargo test -p gyre-server --lib push_below_min
cargo test --all
```
