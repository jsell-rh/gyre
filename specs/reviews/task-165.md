# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3:
Gyre-Managed Runtime (Highest), §Policy per Level.
Candidate under review: `b2e48b4d1b8c3c9df3cdea23ae0e2b629467c16e` (assigned base
`ed57c723631e7ad99b8983cf127fdca3eefb1a9a`; implementation checkpoints `96dfd8b8`,
`19a3d5af`; merge of the base into the branch resolved without task-165 changes).
Verdict: **approved**.

## Evidence (all logs under /tmp/stage/review-evidence/)

Focused suites, re-run independently at the candidate commit (via /tmp/stage/bin/cargo):

- `cargo test -p gyre-domain stack` → **9 passed** (round-trip, integrity, drift,
  malformed TOML, missing-persona, fingerprint discrimination).
- `cargo test -p gyre-server --lib -- api::stack_attest` → **14 passed** (register/get
  stack, policy set/get/clear, min-level + warn mode, bad-level/mode 400s, legacy
  fallbacks, agent-role 403).
- `cargo test -p gyre-server --lib` (task-165 git_http push tests) → **7 passed**
  (below-min blocked w/ ref undo, warn-mode flagged + accepted, meeting-min accepted +
  level stored, L3 from workload attestation, drift blocked, lockfile match accepted,
  tampered lockfile rejected).
- Full-package sweeps at the candidate: `gyre-server --lib` **1253 passed**,
  `gyre-cli` **129 + 1 passed**, `gyre-domain`/`gyre-adapters`/`gyre-common`
  **345 / 111 / 380 passed**. `cargo test --all` itself is rejected by the executor's
  wrapper for workspace-wide invocations in this sandbox — the verifier should run the
  actual workspace command in CI (recorded as a verification item, not a defect).
- Mutation probes (each mutation reverted; tree confirmed clean afterwards):
  1. Disable the below-minimum branch in `enforce_stack_policy` →
     `push_below_min_attestation_level_blocked` AND
     `push_below_min_level_warn_mode_flagged_and_accepted` **fail** (log:
     task165-mutation-below-min.log).
  2. Disable `StackLockfile::check_drift` → `lockfile_drift_blocked_in_block_mode`
     **fails** while the integrity test still passes (log: task165-mutation-drift.log).
  3. Disable `StackLockfile::verify_integrity` →
     `tampered_lockfile_rejected_by_integrity_check` **fails** (log:
     task165-mutation-integrity.log).
  All three acceptance behaviors are kill-covered — the tests are not self-confirming.
- Live end-to-end probe of the CLI command (task165-cli-stack-lock-e2e2.log): started
  the real `gyre-server` (SQLite), `gyre init` registered an agent, POSTed a stack,
  then `gyre stack lock` wrote a `gyre-stack.lock` whose fingerprint
  (`986e26e5…`) matches the server-computed one — CLI and server agree on the
  fingerprint via the shared `gyre-domain` type. With no stack registered the command
  fails cleanly with the server's 404 (task165-cli-stack-lock-e2e.log).
- Invariant gates at the candidate: arch, migration versions (000056 is next unused,
  portable SQL both dialects), ABAC route registry, mem port contracts,
  inert-enforcement, scope-literal-defaults, lossy-secret, relative-path-defaults —
  all OK. `check-fabricated-scope-defaults.sh` fails on `git_http.rs:1727`
  (`Id::new("default")` in spec-lifecycle automation), but that violation is present
  verbatim at the assigned base (`ed57c723` line 1502), is untouched by the candidate
  diff, and predates the task — pre-existing, not a candidate defect.

## Verified working

- **Lockfile schema** (`gyre-domain/src/stack.rs`): real TOML schema with all specced
  components; `parse`/`to_toml` round-trip; `verify_integrity` recomputes the
  composite fingerprint; `check_drift` names differing components. Single shared type
  re-exported into the server (no duplicated fingerprint logic).
- **Push enforcement** (`git_http.rs::enforce_stack_policy`): runs synchronously on
  every push to a policy-bearing repo, after refs are applied and before the response —
  Block mode returns 403 + `undo_ref_updates` + PushRejected event; Warn mode accepts +
  ConstraintViolation event. Same structural pattern as the pre-existing TASK-008
  enforcement.
- **Level resolution**: L3 basis is server-side — container identity written at spawn
  from the server's own container-launch handle (`spawn.rs:768`), JWT claims minted by
  the server's Ed25519 key (callers cannot forge either). L2 = registered stack
  (self-reported, matching the spec's Level-2 definition). L1 = nothing. Level stored
  on the agent record via migration 000056 (sqlite/postgres/mem all round-trip) and
  exposed on `GET /api/v1/agents/:id`.
- **Policy**: `min_attestation_level` (1..=3) + `enforcement` (block|warn) validated on
  PUT; Admin-only; legacy KV entries derive the new fields. Note (design, documented in
  code): `required_level` is kept in sync with `min_attestation_level`, so the legacy
  merge-constraint field cannot diverge from push-time enforcement.
- **Drift**: lockfile read from the pushed tree via `read_git_file` at the new SHA,
  integrity-checked, then compared against the pushing agent's attested stack.

## Non-blocking observations

- `run_stack_lock` (CLI) has no automated test; its core logic (`from_stack`/`to_toml`)
  is covered in gyre-domain and the command was verified live against a real server in
  this review. A CLI integration test would be nice-to-have, not a gap in the specced
  behavior.
- The shipped note claims `ws_integration::test_auth_and_ping_roundtrip` is blocked by
  `tcp_listener_probe.supported=false`; the current `/tmp/stage/capabilities.json` says
  `supported=true` and the test passes in this sandbox. Inaccurate justification, no
  impact on the candidate.
- Known partials are honestly recorded in the coverage rows (unsigned lockfile, L3
  trust basis = server-observed container identity rather than SPIFFE/eBPF) and are
  consistent with the task's implementation plan, which scoped L3 detection to the
  container-claim check.
