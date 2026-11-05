# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3, §Policy per Level.
Candidate under review: `cd9a44abef2ba44d7d37e2134d39376bd4a7165d` (assigned base `110180fbb`; the candidate merges the base into the task branch, so the net diff base→candidate is the task-165 implementation: +778/-16 in git_http.rs, new gyre-domain/src/stack.rs, stack_attest.rs policy extension, CLI `stack lock`, migration 000056, adapters).
Verdict: **needs-revision** (two findings; both narrow, neither touches the core enforcement logic, which is verified real).

## Round 1

All probes at the exact candidate, clean tree, via `/tmp/stage/bin/cargo` (evidence under `/tmp/stage/review-evidence/`, index in SUMMARY.md):

- `cargo test -p gyre-domain stack` → **9 passed** (round-trip, integrity, drift naming, malformed TOML, missing persona, fingerprint discrimination).
- `cargo test -p gyre-server --lib api::stack_attest` → **14 passed** (policy set/get/clear, min-level + warn mode, bad-level/mode 400s, legacy parse fallbacks, agent-role 403).
- git_http task-165 push tests (7) → **all passed**: below-min blocked (403 + ref undone + level not stored), warn-mode flagged-and-accepted (ConstraintViolation event + level 1 stored), min-met accepted + level 2 stored, L3 from workload attestation, drift blocked, lockfile match accepted, tampered lockfile rejected by integrity check.
- **Mutation probe**: temporarily setting `let mode_block = false;` (git_http.rs:1357) made all three block-mode tests FAIL (200 vs 403) — the tests kill real bugs, they do not mirror implementation logic. Source restored; `git status --porcelain` empty at candidate afterwards.
- `cargo test -p gyre-cli --bin gyre` → **129 passed**; `--test ws_integration` → **1 passed**.
- Invariant gates at candidate: arch, migration versions, migration SQL portability, ABAC route registry, mem port contracts, byte-slice, relative-path defaults, scope-literal defaults, lossy secrets, inert enforcement, in-memory state stores, dead message kinds, forged scope fields, forwarded headers, unbounded external HTTP, fail-open ref resolution, task attribution — **all OK** — plus **check-fabricated-scope-defaults.sh FAILS** (finding F1 below).
- `cargo test --all` deferred to the verification stage (workspace-wide invocations are rejected for review; focused suites above cover every task-165 behavior).

Verified working (no findings):

- **Lockfile schema** (`gyre-domain/src/stack.rs`): real TOML schema with all specced components; `parse`/`to_toml` round-trip, `verify_integrity` recomputes the composite SHA-256 fingerprint, `check_drift` names differing components. Canonical-JSON fingerprint inserts keys in sorted order, so it is deterministic regardless of serde_json map ordering.
- **CLI generation**: `gyre stack lock [--output]` fetches the registered stack via `GET /api/v1/agents/:id/stack` and writes the lockfile; thin, correct, compiles and unit-tested.
- **Level resolution**: L3 = server-recorded spawn-time workload attestation carrying container_id+image_hash (spawn.rs writes it for M19.4 container targets) or verified server-minted JWT claims; L2 = registered stack in KV; L1 = none. JWT claims come from `agent_signing_key.validate()` (EdDSA, issuer-checked), not caller-supplied headers. Level stored on the agent record through a real migration (000056, next sequence, portable SQL, sqlite/postgres/mem all round-trip) and exposed on `GET /api/v1/agents/:id`.
- **Per-level policy**: `RepoStackPolicy.min_attestation_level` (1..=3, validated, 400 on bad input) + `enforcement` block/warn; legacy KV entries derive the new field; `parse_stack_policy` normalization keeps `required_level` semantics for the pre-existing CEL constraint path.
- **Push enforcement**: `enforce_stack_policy` runs synchronously on every push to a policy-bearing repo, after receive-pack applies refs and before the response, undoing refs on Block rejection — same established pattern as TASK-008. Warn mode accepts + emits ConstraintViolation. Proven by the mutation probe above.
- No regression surface: no pre-existing push tests seed `repo_stack_policies`, and policy-less pushes take a fast KV-miss path.

Findings:

- **F1 (major, mechanical): `check-fabricated-scope-defaults.sh` fails at the candidate — line-anchored exemption invalidated by this task's insertions.** The pre-existing exempted site (git_http.rs `ws_id` fallback for push event emission) sits at line 1495 at the base — exactly matching the frozen exemption entry — but task-165's +778 lines above it moved it to 1727, so the gate now reports an unexempted violation and exits 1 (pre-commit runs it, `.pre-commit-config.yaml:509`). Proven against the base by running the script's own perl matcher on `git show 110180fb:.../git_http.rs` (reports 1495); the entire git_http.rs delta base→candidate is task-165's (upstream ed57c723 is an ancestor of the base). The task updated the sibling `relative-path-defaults-exemptions.txt` line number for precisely this line-shift class (main.rs:1737→1793, "line moved by task-165 insertions") but missed this file; the shipped notes' "invariant gates at HEAD: all OK" list omits this gate. Remedy: update the exemption entry's line number to 1727 (entry count stays 5; FROZEN_EXEMPTION_COUNT unchanged) — no production code change.
- **F2 (moderate, coverage accuracy): the JWT Level-3 detection branch is dead code, but coverage row 9 and the shipped notes present it as live.** `resolve_push_attestation_level`'s JWT path (wl_container_id+wl_image_hash) can never fire today: `mint_with_workload` (auth.rs:189, the only minter accepting these claims) has zero callers, and spawn.rs:429-441 pre-mints with `mint(...)` — its own comment says "Pre-mint a JWT without workload claims". Level 3 is actually detected via the KV workload-attestation path (real, spawn-written, test-proven), so the acceptance criteria hold; but row 9's claim "wl_container_id/wl_image_hash embedded in agent JWT claims (auth.rs:93-96)" overstates the mechanism and should be corrected (or `mint_with_workload` actually wired at container spawn) so the `implemented` row is not partially hollow on that sub-bullet.

Minor (not blocking, recorded for completeness):

- Drift check inspects only the first ref update carrying a lockfile ("one lockfile per tree"); a multi-ref push where later refs point at trees with different lockfiles checks only the first. Acceptable for the single-lockfile-per-repo model the spec describes.
- `resolve_push_attestation_level` does not consult `WorkloadAttestation.alive` when conferring L3 from the KV record; a stale (dead-container) record still resolves L3 until overwritten. The spec's level definition does not require liveness; noting for a future hardening round.
- Shipped notes claim `ws_integration::test_auth_and_ping_roundtrip` fails in this sandbox (`tcp_listener_probe.supported=false`); the current capabilities file says `supported: true` and the test passes here — stale claim in the conservative direction, no defect.

Conclusion: the implementation itself is real, enforced, and well-tested (mutation-proven), but the candidate cannot be approved as-is: F1 fails a mechanically enforced invariant gate at the exact candidate, and F2 requires a coverage-record correction. Both fixes are narrow; with F1's one-line exemption line-number update and F2's note correction (or mint wiring), this is approvable without re-running the behavioral suites.
