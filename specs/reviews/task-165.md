# Review — task-165: Supply chain — gyre-stack.lock parsing and attestation level enforcement

Candidate: `64cecf5fd5cdb6fa3d4a47c252371504d0acbb22` (base `722179f03aef141e9e546285e3abef724086d25b`).
Repair round for prior findings 4492822e (both items verified resolved). Evidence:
`/tmp/stage/review-evidence/` (commands.txt indexes every probe with exit codes).

## What was independently verified

- **Lockfile schema** (`gyre-domain/src/stack.rs`): real TOML schema with all
  specced components; parse/to_toml round-trip, `verify_integrity()` recomputes
  the fingerprint from components, `check_drift()` names differing components.
  Focused suite: 9/9 passed.
- **CLI `gyre stack lock`**: real implementation — fetches the registered stack
  via GET /api/v1/agents/:id/stack and writes the TOML lockfile. gyre-cli
  suite: 129+1 passed.
- **Attestation levels 1/2/3**: `resolve_push_attestation_level` (git_http.rs)
  resolves L3 from the server-side workload-attestation KV record
  (`attest_agent_with_container`, spawn.rs), L2 from a registered stack, L1
  otherwise; level stored on the agent record (migration
  2026-10-10-000056, sqlite/postgres/mem round-trip) and exposed via
  GET /agents/:id; mapped to provenance labels (server-verified /
  self-reported / unattested) on the post-receive path. The L3 JWT-claim
  branch is honestly recorded as defense-in-depth with no production minting
  site (coverage row 9 corrected in the repair round — verified accurate
  against auth.rs/spawn.rs).
- **Per-level policy**: `RepoStackPolicy.min_attestation_level` (1..=3,
  validated) + block/warn enforcement; Admin-only PUT/GET; legacy KV entries
  derive the min level. stack_attest suite: 14/14 passed.
- **Push enforcement**: `enforce_stack_policy` runs synchronously on every push
  to a policy-bearing repo; below-minimum → 403 + `undo_ref_updates` +
  PushRejected (block) or ConstraintViolation + accept (warn); lockfile read
  from the pushed tree via `read_git_file`, integrity-checked, drift-checked.
  The 7 task-165 git_http tests exercise real receive-pack pushes against real
  temp repos and assert status codes, ref-undo effects, emitted events, and
  stored levels — they cannot pass with the enforcement removed (compliant
  pushes return 200 in the same harness, e.g. lockfile_match_accepted).
  7/7 passed.
- **Static gates** at candidate HEAD: relative-path-defaults,
  fabricated-scope-defaults, migration-versions, migration-sql-portability,
  task-commit-attribution, arch, inert-enforcement — all OK. Full battery from
  the interrupted round preserved under /tmp/stage/review-resume.
- **Repair-round items**: the fabricated-scope exemption deletion (5→4) is
  backed by a real skip-and-log fix at git_http.rs:1730-1737; remaining 4
  entries verified live. SUMMARY.md re-sync is mechanical. Migration sequence
  000056 reuse vs upstream `2026-10-08-000056_spec_assertion_results` was
  investigated: task-165 picked 000056 correctly when 000055 was latest; the
  collision arrived via a later upstream merge; Diesel versions are distinct
  (date prefix differs) so both migrations run — not a defect.
- `cargo test --all` deferred to the verification stage (workspace-wide runs
  are rejected for the review stage); focused suites above all green.

## Findings

One finding (see /tmp/stage/verdict.json):

1. **Dead exemption entry** — `scripts/relative-path-defaults-exemptions.txt`
   pins `crates/gyre-cli/src/main.rs:1793`, but at candidate HEAD line 1793 is
   a closing brace and the described
   `.unwrap_or_else(|| std::path::PathBuf::from(&repo_name))` site no longer
   exists anywhere in main.rs: the merge with the assigned base (68c0be4e)
   adopted the upstream main.rs where that fallback became
   `args.repo_path.as_deref().map(PathBuf::from)` (line 1869). The entry was
   correctly re-anchored when live (19a3d5af:1793 held the site), but the
   merge that removed the site left the entry behind. The file's own header
   requires every entry to be a real hazard and mandates deleting entries for
   fixed sites. Verified harmless-but-dead (gate passes with the entry
   removed — `relpath-probe-without-entry.log`); same flaw class the prior
   repair round fixed for the sibling fabricated-scope file. Fix: delete the
   entry line.

Not approved pending that one-line repair. Everything else in the task
contract is real, enforced, and test-anchored.

— Review round, 2026-11-05
