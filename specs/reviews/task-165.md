# Review: task-165 — Supply chain — gyre-stack.lock parsing and attestation level enforcement

**Candidate:** `867f411cb97544dfe70a884c15f8d4c90ca55a99` (branch `pipeline/task-165/d0ff073574574e6f88e2c757e4fee6d5-1`)
**Base:** `0cb71015861e41b082183b89e9a1924808ad114a`
**Round:** 8 (independent review; prior rounds 4492822e, ccc54b26 repairs verified resolved)
**Verdict:** APPROVED — evidence under `/tmp/stage/review-evidence/`

## What was reviewed

Full diff base..candidate (23 files, +1650/−128): `gyre-domain/src/stack.rs`
(new, 377 lines), `git_http.rs` (+802), `api/stack_attest.rs` (+311/−128),
`gyre-cli` stack-lock command + client method, migration
`2026-10-10-000056_agent_attestation_level`, adapters (sqlite/postgres agent
`attestation_level`), docs, coverage rows 7/8/9/12, and the round-7 repair
(dead relative-path exemption entry deletion, commit `12a0d0e9`).

## Evidence (all at candidate HEAD, logs in /tmp/stage/review-evidence/)

**Focused test suites (all green):**
- gyre-domain `stack` — 9 passed (lockfile round-trip, integrity, drift naming,
  tamper detection, optional persona).
- gyre-server `api::stack_attest` — 14 passed (policy set/get/clear, level
  validation 1..=3, warn/block modes, NEW-39 agent-role 403, legacy parse).
- gyre-server git_http task-165 push tests — 7 passed
  (`push_below_min_attestation_level_blocked` asserts 403 + body string + ref
  deletion via `git rev-parse`; warn-mode asserts ConstraintViolation event +
  stored level 1; L3 via workload attestation; drift/integrity rejections).
  These run the real HTTP receive-pack path against real on-disk git repos —
  not stub-backed.
- gyre-server `constraint_check` — 31 passed (adjacent level taxonomy intact).
- gyre-adapters sqlite `agent` — 17 passed (new column round-trips through the
  real migration set).
- gyre-cli `ws_integration::test_auth_and_ping_roundtrip` — **passes in this
  sandbox** (listener supported here), resolving the prior round's deferred
  infrastructure check.

**Invariant gates (all exit 0):** check-relative-path-defaults,
check-fabricated-scope-defaults (4 entries = FROZEN_EXEMPTION_COUNT=4),
check-arch, check-migration-versions, check-mem-port-contracts,
check-abac-route-registry, check-task-commit-attribution.

**Repair-round verification:** commit `12a0d0e9` touches only
`scripts/relative-path-defaults-exemptions.txt` (−5 lines); the deleted
`main.rs:1793` entry anchors a closing brace at HEAD (site genuinely removed
upstream); remaining 3 entries hand-verified live. Candidate HEAD `867f411c`
removed the non-product-surface `12a0d0e9` from the `commits:` frontmatter —
permitted: the attribution script only requires product-surface
(crates/web/src/web/tests) commits, and the repair remains documented in the
shipped notes.

**Security probes:**
- `resolve_push_attestation_level`'s JWT L3 branch reads `auth.jwt_claims`,
  which is populated only from cryptographically validated tokens
  (`agent_signing_key.validate` / Keycloak JWKS / federation validation) — no
  client-forgeable claim path.
- L3 KV path is production-real: `spawn.rs:1071` writes
  `attest_agent_with_container(container_id, image_hash)` to
  `workload_attestations` for container targets.
- `set_stack_policy` is Admin-only with 1..=3 and block/warn validation;
  routes registered in the ABAC resolver (`abac_middleware.rs:134,196`).
- Migration `2026-10-10-000056` shares sequence 000056 with
  `2026-10-08-000056_spec_assertion_results`, but Diesel versions derive from
  the full date+sequence prefix (20261010… ≠ 20261008…), so both run —
  check-migration-versions passes and confirms.

**CLI probe:** built `gyre` from candidate source
(`SKIP_WEB_BUILD=1 cargo build -p gyre-cli`); `gyre stack lock --help` parses
and documents `--output`. The handler is thin glue over `get_agent_stack`
(standard reqwest GET) and the 9-test domain lockfile code.

## Deferred check (transport restriction, not a code defect)

A live CLI→server loopback smoke test of `gyre stack lock` (server up, stack
registered, verify written TOML) is blocked: the review sandbox's tool layer
SSRF-blocks loopback HTTP (127.0.0.0/8 and ::1). Compensating evidence: the
command parses in the built binary, `GET /agents/:id/stack` is covered by
`register_and_get_stack`, and lockfile construction is covered by the domain
suite. Verifier may run this E2E in an unrestricted environment.

## Notes (non-blocking)

- No `cli_stack_lock_parses` clap test in main.rs (other subcommands have
  parse tests) — cosmetic gap for thin glue; behavior verified via the built
  binary and domain tests.
- Lockfile drift/integrity enforcement runs only on policy-bearing repos —
  documented in shipped notes and coverage row 7; consistent with the task's
  placement of drift under policy enforcement.
- Coverage rows 7/8/9/12 accurately record partials (no approver signature on
  the lockfile, L3 trust basis is server-observed container identity, JWT
  workload-claim minting path unwired) — not hollow.
